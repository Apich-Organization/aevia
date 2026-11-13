//! Safe access to RSSN's scalar machine-code compiler.
//!
//! Instructions retain their original order: no algebraic reassociation is
//! performed, so floating-point evaluation follows the source program.

use rssn::backend::CompiledMulti;
use rssn::backend::jit::JitFunction;
use rssn::backend::jit::TailLoop;
pub use rssn::backend::tape::Cmp;
pub use rssn::backend::tape::Inst;
pub use rssn::backend::tape::Reg;
use rssn::backend::tape::Tape;

pub use rssn::backend::Intrinsic;
pub use rssn::backend::lir;

/// A LIR module ([`lir`]) compiled to machine code; its code lives until this
/// value is dropped.
pub struct NativeModule(rssn::backend::lir_jit::LirModule);

impl NativeModule {
    /// Verifies and compiles `module`; `None` when it is malformed or the
    /// host cannot compile it.
    #[must_use]
    pub fn compile(module: &lir::Module) -> Option<Self> {
        rssn::backend::lir_jit::LirModule::compile(module).ok().map(Self)
    }

    /// Calls `func`; `None` when the arguments do not fit its signature.
    pub fn call(
        &self,
        func: lir::FuncId,
        args: &mut [lir::Value<'_>],
    ) -> Option<lir::Outcome> {
        self.0.call(func, args).ok()
    }
}

/// A compiled scalar program whose code stays alive until this value is dropped.
pub struct Program {
    function: JitFunction,
    inputs: usize,
}

impl Program {
    /// Compile a well-formed SSA instruction sequence.
    /// Returns `None` when the backend cannot compile for this host.
    #[must_use]
    pub fn compile(
        insts: Vec<Inst>,
        inputs: usize,
        outputs: Vec<Reg>,
    ) -> Option<Self> {
        // RSSN's low-level tape accepts raw registers and input offsets. Check
        // them here before code generation, so malformed callers cannot make
        // generated code read beyond its input buffer.
        if outputs.is_empty() || outputs.iter().any(|output| *output as usize >= insts.len()) {
            return None;
        }
        for (index, inst) in insts.iter().enumerate() {
            let valid = |reg: Reg| usize::try_from(reg).is_ok_and(|reg| reg < index);
            match *inst {
                | Inst::Const(_) => {},
                | Inst::Input(input)
                    if usize::try_from(input).is_ok_and(|input| input < inputs)
                        && i32::try_from(input)
                            .is_ok_and(|input| input.checked_mul(8).is_some()) => {},
                | Inst::Neg(a) if valid(a) => {},
                | Inst::Add(a, b)
                | Inst::Sub(a, b)
                | Inst::Mul(a, b)
                | Inst::Div(a, b)
                | Inst::Cmp(_, a, b)
                    if valid(a) && valid(b) => {},
                | Inst::Select(a, b, c) if valid(a) && valid(b) && valid(c) => {},
                | _ => return None,
            }
        }
        let tape = Tape {
            insts,
            args: Vec::new(),
            inputs,
            params: 0,
            outputs,
        };
        JitFunction::compile(&tape)
            .ok()
            .map(|function| Self { function, inputs })
    }

    /// Evaluate when the input count matches the compiled program.
    #[must_use]
    pub fn call(
        &self,
        inputs: &[f64],
    ) -> Option<Vec<f64>> {
        if inputs.len() != self.inputs {
            return None;
        }
        let mut outputs = vec![0.0; self.function.outputs()];
        self.function.eval(inputs, &[], &mut outputs);
        Some(outputs)
    }
}

/// A pure scalar tail-recursive program with a native back edge.
pub struct LoopProgram {
    function: TailLoop,
    inputs: usize,
}

/// A completed step and its remaining tail-call budget.
pub struct LoopResult {
    /// The completion flag, result, and next arguments.
    pub outputs: Vec<f64>,
    /// The number of jumps that were not used.
    pub remaining: u64,
}

impl LoopProgram {
    /// Compile a validated multi-output step and a native loop around it.
    #[must_use]
    pub fn compile(
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
        let function = TailLoop::compile(&tape).ok()?;
        Some(Self { function, inputs })
    }

    /// Run with a bound on the number of tail jumps.
    #[must_use]
    pub fn run(
        &self,
        inputs: &[f64],
        fuel: u64,
    ) -> Option<LoopResult> {
        if inputs.len() != self.inputs {
            return None;
        }
        let result = self.function.run(inputs, fuel)?;
        Some(LoopResult {
            outputs: result.outputs,
            remaining: result.remaining,
        })
    }
}
