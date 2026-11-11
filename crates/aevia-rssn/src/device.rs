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
    /// mismatch, or when the device reports an error. A device that computes
    /// in `f32` returns `f32`-rounded values; the CPU devices return the
    /// interpreter's `f64` results bit for bit.
    #[must_use]
    pub fn run(
        &self,
        device: &dyn Device,
        columns: &[&[f64]],
    ) -> Option<Vec<Vec<f64>>> {
        if columns.len() != self.ir.inputs {
            return None;
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
}
