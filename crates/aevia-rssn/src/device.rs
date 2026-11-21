//! Safe access to RSSN's execution devices (`rssn::backend::device`).
//!
//! This is the Aevia side of the device architecture in `spec/10-backend.md`
//! §10.6.5: discovery ([`available`], [`has_gpu`]), resolution of an Aevia
//! `Device` value to an RSSN device ([`resolve`]), and a batch kernel
//! ([`BatchProgram`]) that runs one pure scalar program over many rows on
//! that device.
//!
//! Nothing here needs the Cranelift `jit` feature or a GPU: the CPU devices
//! are always in the registry. A GPU appears only when RSSN is built with
//! `gpu-wgpu` and an adapter exists.
//!
//! Transfers are explicit at this boundary as well: [`BatchProgram::run`]
//! uploads its columns, launches once and downloads once; no buffer outlives
//! the call.

use std::sync::Arc;

pub use rssn::backend::device::Device;
pub use rssn::backend::device::DeviceInfo;
pub use rssn::backend::device::DeviceKind;
pub use rssn::backend::device::Dtype;
use rssn::backend::device::KernelIr;
use rssn::backend::device::map_columns;
use rssn::backend::device::preferred_dtype;
use rssn::backend::device::registry;
use rssn::backend::tape::Inst;
use rssn::backend::tape::Reg;
use rssn::backend::tape::Tape;

/// Which device an Aevia program asked for.
///
/// `Cpu` and `CpuParallel` are always available; `Gpu(n)` is the `n`th GPU
/// of the registry (in registration order) and is absent without the
/// `core.gpu` capability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selector {
    /// One host thread (`Device.Cpu`).
    Cpu,
    /// The host thread pool (`Device.cpu_parallel`).
    CpuParallel,
    /// The `n`th GPU (`Device.Gpu(n)`).
    Gpu(usize),
}

/// Every device the process can use now, registry order.
#[must_use]
pub fn available() -> Vec<DeviceInfo> {
    registry().list()
}

/// Whether any GPU is registered: the run-time half of the `core.gpu`
/// capability (the build half is `rssn_manifest().features.devices`).
#[must_use]
pub fn has_gpu() -> bool {
    available()
        .iter()
        .any(|d| matches!(d.kind, DeviceKind::Gpu(_)))
}

/// The device behind a selector.
///
/// # Errors
/// A message naming the missing capability, for `E1001("core.gpu")`.
pub fn resolve(selector: Selector) -> Result<Arc<dyn Device>, String> {
    let reg = registry();
    match selector {
        | Selector::Cpu => reg.get("cpu").ok_or_else(|| "no `cpu` device".to_string()),
        | Selector::CpuParallel => {
            reg.get("cpu-parallel")
                .ok_or_else(|| "no `cpu-parallel` device".to_string())
        },
        | Selector::Gpu(n) => {
            let gpus: Vec<DeviceInfo> = reg
                .list()
                .into_iter()
                .filter(|d| matches!(d.kind, DeviceKind::Gpu(_)))
                .collect();
            gpus.get(n)
                .and_then(|info| reg.get(&info.id))
                .ok_or_else(|| {
                    format!(
                        "`core.gpu`: GPU {n} is not available ({} GPU device(s) registered)",
                        gpus.len()
                    )
                })
        },
    }
}

/// A pure scalar program prepared for batch execution on a device.
///
/// The program is the same SSA instruction sequence the scalar JIT takes
/// (`Inst`), so a function that lowers for the JIT lowers for a device. Host
/// calls cannot be expressed and are refused at construction.
#[derive(Clone, Debug)]
pub struct BatchProgram {
    ir: KernelIr,
}

impl BatchProgram {
    /// Prepares a well-formed program. Returns `None` for a malformed one
    /// (forward references, out-of-range inputs/outputs) or one that uses a
    /// host call.
    #[must_use]
    pub fn new(
        insts: Vec<Inst>,
        inputs: usize,
        outputs: Vec<Reg>,
    ) -> Option<Self> {
        let tape = Tape {
            insts,
            args: Vec::new(),
            inputs,
            params: 0,
            outputs,
        };
        KernelIr::from_tape(&tape).ok().map(|ir| Self { ir })
    }

    /// Number of input columns.
    #[must_use]
    pub const fn inputs(&self) -> usize {
        self.ir.inputs
    }

    /// Number of outputs.
    #[must_use]
    pub const fn outputs(&self) -> usize {
        self.ir.outputs.len()
    }

    /// The backend-neutral program (for emitters and tests).
    #[must_use]
    pub const fn kernel(&self) -> &KernelIr {
        &self.ir
    }

    /// Runs the program over `columns[i][row]` on `device`; returns one
    /// vector per output. Fails (`None`) on a column-count or length
    /// mismatch, or when the device reports an error. The CPU devices return
    /// the interpreter's `f64` results bit for bit. A wgpu device computes
    /// `f64` columns in double-single arithmetic (pairs of `f32`, relative
    /// error around `1e-14`, the exponent range of `f32`; see rssn
    /// `docs/BACKENDS.md`), and `f32`-rounded values when it offers no `F64`.
    #[must_use]
    pub fn run(
        &self,
        device: &dyn Device,
        columns: &[&[f64]],
    ) -> Option<Vec<Vec<f64>>> {
        if columns.len() != self.ir.inputs {
            return None;
        }
        if let [column] = columns
            && self.ir.outputs.len() == 1
        {
            // The single input is already in upload order, and the download
            // can fill the only output column directly.
            let rows = column.len();
            let dtype = preferred_dtype(device.info());
            let kernel = device.compile(&self.ir).ok()?;
            let input = device.upload(column, dtype).ok()?;
            let mut output = device.alloc(rows, dtype).ok()?;
            device
                .launch(&*kernel, rows, &input, &[], &mut output)
                .ok()?;
            let mut host = vec![0.0; rows];
            device.download(&output, &mut host).ok()?;
            return Some(vec![host]);
        }
        let rows = columns.first().map_or(0, |c| c.len());
        if columns.iter().any(|c| c.len() != rows) {
            return None;
        }
        map_columns(device, &self.ir, columns, &[]).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rssn::backend::device::Buffer;
    use rssn::backend::device::CompiledKernel;
    use rssn::backend::device::DeviceError;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;

    fn square_plus_one() -> BatchProgram {
        // out = x * x + 1
        BatchProgram::new(
            vec![
                Inst::Input(0),
                Inst::Mul(0, 0),
                Inst::Const(1.0),
                Inst::Add(1, 2),
            ],
            1,
            vec![3],
        )
        .unwrap()
    }

    #[test]
    fn the_cpu_devices_are_always_listed() {
        let ids: Vec<String> = available().into_iter().map(|d| d.id).collect();
        assert!(ids.iter().any(|i| i == "cpu"));
        assert!(ids.iter().any(|i| i == "cpu-parallel"));
        assert_eq!(
            has_gpu(),
            available()
                .iter()
                .any(|d| matches!(d.kind, DeviceKind::Gpu(_)))
        );
    }

    #[test]
    fn a_missing_gpu_names_the_capability() {
        if has_gpu() {
            return;
        }
        let Err(message) = resolve(Selector::Gpu(0)) else {
            panic!("no GPU should be registered")
        };
        assert!(message.contains("core.gpu"), "{message}");
    }

    #[test]
    fn batches_agree_on_every_cpu_device() {
        let program = square_plus_one();
        let xs: Vec<f64> = (0..10_000).map(|i| f64::from(i) * 0.01 - 3.0).collect();
        let seq = program
            .run(&*resolve(Selector::Cpu).unwrap(), &[&xs])
            .unwrap();
        let par = program
            .run(&*resolve(Selector::CpuParallel).unwrap(), &[&xs])
            .unwrap();
        assert_eq!(seq, par);
        for (x, y) in xs.iter().zip(&seq[0]) {
            assert_eq!((x * x + 1.0).to_bits(), y.to_bits());
        }
    }

    #[test]
    fn malformed_programs_and_shapes_are_refused() {
        assert!(BatchProgram::new(vec![Inst::Neg(0)], 0, vec![0]).is_none());
        assert!(BatchProgram::new(vec![Inst::Input(2)], 1, vec![0]).is_none());
        assert!(BatchProgram::new(vec![Inst::Const(1.0)], 0, vec![]).is_none());
        let program = square_plus_one();
        let cpu = resolve(Selector::Cpu).unwrap();
        assert!(program.run(&*cpu, &[]).is_none());
        assert!(program.run(&*cpu, &[&[1.0], &[2.0]]).is_none());
        assert_eq!(program.run(&*cpu, &[&[]]).unwrap(), vec![Vec::<f64>::new()]);
    }

    #[test]
    fn direct_batches_preserve_every_float_bit_on_both_cpu_devices() {
        let identity = BatchProgram::new(vec![Inst::Input(0)], 1, vec![0]).unwrap();
        let samples = [
            0.0,
            -0.0,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::from_bits(1),
            f64::from_bits(0x7ff8_0000_0000_1234),
            -37.5,
        ];
        for selector in [Selector::Cpu, Selector::CpuParallel] {
            let device = resolve(selector).unwrap();
            for rows in [0, 1, 129, 4097] {
                let data: Vec<f64> = samples.iter().copied().cycle().take(rows).collect();
                let columns = [data.as_slice()];
                let reference = map_columns(&*device, identity.kernel(), &columns, &[]).unwrap();
                let direct = identity.run(&*device, &columns).unwrap();
                assert_eq!(direct.len(), 1);
                assert_eq!(direct[0].len(), rows);
                for ((input, expected), actual) in data.iter().zip(&reference[0]).zip(&direct[0]) {
                    assert_eq!(actual.to_bits(), expected.to_bits());
                    assert_eq!(actual.to_bits(), input.to_bits());
                }
            }
        }
    }

    #[test]
    fn multiple_columns_keep_their_order_and_require_equal_lengths() {
        let program = BatchProgram::new(
            vec![Inst::Input(0), Inst::Input(1), Inst::Sub(0, 1)],
            2,
            vec![2, 0],
        )
        .unwrap();
        for selector in [Selector::Cpu, Selector::CpuParallel] {
            let device = resolve(selector).unwrap();
            assert_eq!(
                program.run(&*device, &[&[2.0, -1.0], &[5.0, -4.0]]),
                Some(vec![vec![-3.0, 3.0], vec![2.0, -1.0]]),
            );
            assert!(program.run(&*device, &[&[2.0], &[]]).is_none());
        }
    }

    struct RefusesLaunch {
        info: DeviceInfo,
        cpu: Arc<dyn Device>,
        upload_address: usize,
        launched: AtomicBool,
    }

    impl Device for RefusesLaunch {
        fn info(&self) -> &DeviceInfo {
            &self.info
        }

        fn uid(&self) -> u64 {
            self.cpu.uid()
        }

        fn alloc(
            &self,
            len: usize,
            dtype: Dtype,
        ) -> Result<Buffer, DeviceError> {
            assert_eq!(dtype, Dtype::F32);
            self.cpu.alloc(len, Dtype::F64)
        }

        fn upload(
            &self,
            host: &[f64],
            dtype: Dtype,
        ) -> Result<Buffer, DeviceError> {
            assert_eq!(host.as_ptr().addr(), self.upload_address);
            assert_eq!(dtype, Dtype::F32);
            self.cpu.upload(host, Dtype::F64)
        }

        fn download(
            &self,
            _buffer: &Buffer,
            _output: &mut [f64],
        ) -> Result<(), DeviceError> {
            panic!("a failed launch must not be downloaded");
        }

        fn compile(
            &self,
            ir: &KernelIr,
        ) -> Result<Arc<dyn CompiledKernel>, DeviceError> {
            self.cpu.compile(ir)
        }

        fn launch(
            &self,
            _kernel: &dyn CompiledKernel,
            _rows: usize,
            _input: &Buffer,
            _params: &[f64],
            _output: &mut Buffer,
        ) -> Result<(), DeviceError> {
            self.launched.store(true, Ordering::Relaxed);
            Err(DeviceError::Unsupported("test launch".into()))
        }
    }

    #[test]
    fn a_direct_batch_borrows_its_column_and_propagates_a_launch_error() {
        // Exercise an F32-only device's dtype negotiation without probing hardware.
        let cpu = resolve(Selector::Cpu).unwrap();
        let mut info = cpu.info().clone();
        info.dtypes = vec![Dtype::F32];
        let input = [1.0, 2.0];
        let device = RefusesLaunch {
            info,
            cpu,
            upload_address: input.as_ptr().addr(),
            launched: AtomicBool::new(false),
        };
        assert!(square_plus_one().run(&device, &[&input]).is_none());
        assert!(device.launched.load(Ordering::Relaxed));
    }
}
