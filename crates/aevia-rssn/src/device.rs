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
pub use rssn::backend::device::DeviceOps;
pub use rssn::backend::device::Dtype;
use rssn::backend::device::KernelIr;
pub use rssn::backend::device::ReduceOp;
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

/// Resolves the preferred execution device for the given GPU/parallelism
/// preference. Falls back to CPU when `prefer_gpu` is set but no GPU adapter
/// is registered.
///
/// # Errors
/// Returns an error if the CPU device itself is missing from the registry.
pub fn resolve_default(
    prefer_gpu: bool,
    parallel: bool,
) -> Result<Arc<dyn Device>, String> {
    if prefer_gpu && has_gpu() {
        return resolve(Selector::Gpu(0));
    }
    resolve(if parallel {
        Selector::CpuParallel
    } else {
        Selector::Cpu
    })
}

/// Device preference for eligible Aevia matrix products.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MatmulPolicy {
    /// Consider native f64 hardware once large work can amortize transfers.
    #[default]
    Auto,
    /// Use the host's dense kernels.
    Cpu,
    /// Also try smaller products and the exact subset on emulated devices.
    PreferGpu,
}

/// Try a registered GPU when it can preserve the Float calculation.
///
/// A device that emulates f64 is eligible only when every input is an exact
/// integer and the sum of absolute products is bounded by 2^24. That also
/// makes its f32 indexing parameters exact for the bounded launch shape.
/// Device failures permit replay on the host; the inputs are unmodified.
#[must_use]
pub fn try_gpu_matmul(
    policy: MatmulPolicy,
    left: &[f64],
    right: &[f64],
    rows: usize,
    inner: usize,
    cols: usize,
) -> Option<Vec<f64>> {
    if policy == MatmulPolicy::Cpu
        || rows == 0
        || inner == 0
        || cols == 0
        || !rssn::backend::gpu_allowed()
    {
        return None;
    }
    let output = rows.checked_mul(cols)?;
    let work = output.checked_mul(inner)?;
    if left.len() != rows.checked_mul(inner)? || right.len() != inner.checked_mul(cols)? {
        return None;
    }
    // The GPU operation copies both host inputs and the result. Keep small
    // operations on the host in automatic mode, including skinny products.
    if policy == MatmulPolicy::Auto && (work < (1 << 24) || inner < 64) {
        return None;
    }
    let inputs = left.len().checked_add(right.len())?;
    let mut exact_f32 = None;
    let devices = registry();
    let candidates = devices.list();
    let mut finite = None;
    for info in candidates {
        if !matches!(info.kind, DeviceKind::Gpu(_))
            || !info.ops.matmul
            || !info.dtypes.contains(&Dtype::F64)
            || inputs.max(output) > info.max_buffer_elems
            || (policy == MatmulPolicy::Auto
                && (info.emulated_f64 || info.name.contains(" [software]")))
        {
            continue;
        }
        let dtype = if info.emulated_f64 {
            if inputs.max(output) > (1 << 24)
                || !*exact_f32.get_or_insert_with(|| exact_f32_matmul(left, right, inner))
            {
                continue;
            }
            if info.dtypes.contains(&Dtype::F32) {
                Dtype::F32
            } else {
                Dtype::F64
            }
        } else {
            if !*finite.get_or_insert_with(|| left.iter().chain(right).all(|x| x.is_finite())) {
                continue;
            }
            Dtype::F64
        };
        if let Some(device) = devices.get(&info.id)
            && let Ok(data) = matmul_with_dtype(&*device, dtype, left, right, rows, inner, cols)
        {
            return Some(data);
        }
    }
    None
}

#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "isolates the precision bound for emulated GPU work"
    )
)]
fn exact_f32_matmul(
    left: &[f64],
    right: &[f64],
    inner: usize,
) -> bool {
    const MAX_EXACT: f64 = 16_777_216.0;
    let bounds = |values: &[f64]| {
        values.iter().try_fold(
            (0.0_f64, false, false),
            |(max, has_zero, has_negative), &x| {
                let abs = x.abs();
                // A negative zero can already distinguish the formats.
                (abs <= MAX_EXACT && x.fract() == 0.0 && !(x == 0.0 && x.is_sign_negative()))
                    .then_some((max.max(abs), has_zero || x == 0.0, has_negative || x < 0.0))
            },
        )
    };
    let (
        Some((left_max, left_zero, left_negative)),
        Some((right_max, right_zero, right_negative)),
        Ok(terms),
    ) = (bounds(left), bounds(right), u32::try_from(inner))
    else {
        return false;
    };
    // Different accumulators can retain or erase the minus sign of a zero
    // product. Keep those cases on Aevia's host path.
    !(left_zero && right_negative || right_zero && left_negative)
        && left_max * right_max * f64::from(terms) <= MAX_EXACT
}

/// Multiplies two row-major matrices on `device`. The left shape is
/// `rows × inner` and the right shape is `inner × columns`.
///
/// # Errors
/// Returns a message on shape mismatch, overflow, or when the device does not
/// support `matmul` or fails to execute.
pub fn matmul(
    device: &dyn Device,
    left: &[f64],
    right: &[f64],
    rows: usize,
    inner: usize,
    columns: usize,
) -> Result<Vec<f64>, String> {
    matmul_with_dtype(
        device,
        preferred_dtype(device.info()),
        left,
        right,
        rows,
        inner,
        columns,
    )
}

fn matmul_with_dtype(
    device: &dyn Device,
    dtype: Dtype,
    left: &[f64],
    right: &[f64],
    rows: usize,
    inner: usize,
    columns: usize,
) -> Result<Vec<f64>, String> {
    let expected_left = rows
        .checked_mul(inner)
        .ok_or_else(|| "device matmul: matrix `a` dimensions overflow".to_string())?;
    let expected_right = inner
        .checked_mul(columns)
        .ok_or_else(|| "device matmul: matrix `b` dimensions overflow".to_string())?;
    let out_len = rows
        .checked_mul(columns)
        .ok_or_else(|| "device matmul: output dimensions overflow".to_string())?;
    if left.len() != expected_left || right.len() != expected_right {
        return Err(format!(
            "device matmul: expected lengths {expected_left} and {expected_right}, got {} and {}",
            left.len(),
            right.len()
        ));
    }
    if out_len == 0 {
        return Ok(Vec::new());
    }
    let buf_a = device
        .upload(left, dtype)
        .map_err(|err| format!("device `{}` upload failed: {err}", device.info().id))?;
    let buf_b = device
        .upload(right, dtype)
        .map_err(|err| format!("device `{}` upload failed: {err}", device.info().id))?;
    let mut buf_c = device
        .alloc(out_len, dtype)
        .map_err(|err| format!("device `{}` alloc failed: {err}", device.info().id))?;
    device
        .matmul(&buf_a, &buf_b, &mut buf_c, rows, inner, columns)
        .map_err(|err| format!("device `{}` matmul failed: {err}", device.info().id))?;
    let mut host = vec![0.0; out_len];
    device
        .download(&buf_c, &mut host)
        .map_err(|err| format!("device `{}` download failed: {err}", device.info().id))?;
    Ok(host)
}

/// Reduces a host slice on `device` using `op`.
///
/// # Errors
/// Returns a message when `data` is empty for `Min`/`Max`, or when the device
/// does not support `reduce` or fails to execute.
pub fn reduce(
    device: &dyn Device,
    op: ReduceOp,
    data: &[f64],
) -> Result<f64, String> {
    if data.is_empty() {
        return match op {
            | ReduceOp::Sum | ReduceOp::Prod => Ok(op.identity()),
            | ReduceOp::Min | ReduceOp::Max => {
                Err("device reduce: min/max of an empty buffer is undefined".to_string())
            },
        };
    }
    let dtype = preferred_dtype(device.info());
    let buf = device
        .upload(data, dtype)
        .map_err(|err| format!("device `{}` upload failed: {err}", device.info().id))?;
    device
        .reduce(op, &buf)
        .map_err(|err| format!("device `{}` reduce failed: {err}", device.info().id))
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

    #[test]
    fn emulated_gpu_matmul_requires_exact_operands_and_accumulation() {
        assert!(exact_f32_matmul(&[4_096.0], &[4_096.0], 1));
        assert!(!exact_f32_matmul(&[4_096.0], &[4_096.0], 2));
        assert!(exact_f32_matmul(&[0.0], &[1.0], 1));
        assert!(!exact_f32_matmul(&[0.0], &[-1.0], 1));
        assert!(!exact_f32_matmul(&[-1.0], &[0.0], 1));
        for bad in [f64::NAN, f64::INFINITY, -0.0, 0.25, 16_777_217.0] {
            assert!(!exact_f32_matmul(&[bad], &[1.0], 1));
            assert!(!exact_f32_matmul(&[1.0], &[bad], 1));
        }
        assert!(try_gpu_matmul(MatmulPolicy::Cpu, &[1.0], &[1.0], 1, 1, 1).is_none());
        assert!(try_gpu_matmul(MatmulPolicy::Auto, &[1.0], &[1.0], 1, 1, 1).is_none());
        assert!(try_gpu_matmul(MatmulPolicy::PreferGpu, &[1.0], &[1.0], 1, 2, 1).is_none());
        assert!(try_gpu_matmul(MatmulPolicy::PreferGpu, &[], &[], usize::MAX, 2, 2).is_none());
    }

    #[test]
    #[cfg(feature = "gpu-wgpu")]
    fn preferred_gpu_executes_an_exact_product_when_an_adapter_is_registered() {
        if !has_gpu() {
            return;
        }
        assert_eq!(
            try_gpu_matmul(
                MatmulPolicy::PreferGpu,
                &[1.0, -2.0, 3.0, 4.0],
                &[5.0, 6.0, 7.0, 8.0],
                2,
                2,
                2,
            ),
            Some(vec![-9.0, -10.0, 43.0, 50.0])
        );
    }

    #[test]
    fn device_matmul_and_reduce_agree_across_cpu_devices() {
        let a = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let b = [7.0, 8.0, 9.0, 10.0, 11.0, 12.0];
        let expected = [58.0, 64.0, 139.0, 154.0];
        for selector in [Selector::Cpu, Selector::CpuParallel] {
            let device = resolve(selector).unwrap();
            let got = matmul(&*device, &a, &b, 2, 3, 2).unwrap();
            assert_eq!(got, expected);
            assert_eq!(reduce(&*device, ReduceOp::Sum, &a).unwrap(), 21.0);
            assert_eq!(reduce(&*device, ReduceOp::Prod, &a).unwrap(), 720.0);
            assert_eq!(reduce(&*device, ReduceOp::Min, &a).unwrap(), 1.0);
            assert_eq!(reduce(&*device, ReduceOp::Max, &a).unwrap(), 6.0);
            assert_eq!(reduce(&*device, ReduceOp::Sum, &[]).unwrap(), 0.0);
            assert_eq!(reduce(&*device, ReduceOp::Prod, &[]).unwrap(), 1.0);
            assert!(reduce(&*device, ReduceOp::Min, &[]).is_err());
        }
        let default_par = resolve_default(false, true).unwrap();
        assert_eq!(default_par.info().id, "cpu-parallel");
        let default_seq = resolve_default(false, false).unwrap();
        assert_eq!(default_seq.info().id, "cpu");
    }
}
