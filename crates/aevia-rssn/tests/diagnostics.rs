//! Why a call did not reduce: the kernel's explanation reaches the host as a
//! failure with a stable code, the argument at fault and a message.
//!
//! The assertions are on the code, the kind and the argument position; the
//! message text is the kernel's and is only checked where the Aevia-side
//! rendering adds to it.

use aevia_rssn::Failure;
use aevia_rssn::KernelError;
use aevia_rssn::Num;
use aevia_rssn::Val;
use aevia_rssn::domains::wrapper_params;
use aevia_rssn::kernel;
use aevia_rssn::registry;
use num_bigint::BigInt;
use rssn::kernel::Diag;
use rssn::kernel::DiagKind;

fn float(x: f64) -> Val {
    Val::Num(Num::Float(x))
}

fn int(n: i64) -> Val {
    Val::Num(Num::Int(BigInt::from(n)))
}

fn matrix(rows: &[[f64; 2]]) -> Val {
    Val::Tensor {
        shape: vec![rows.len(), 2],
        data: rows.iter().flatten().map(|x| float(*x)).collect(),
    }
}

fn list(items: Vec<Val>) -> Val {
    Val::Tensor {
        shape: vec![items.len()],
        data: items,
    }
}

#[test]
fn a_finding_becomes_a_failure_with_the_stable_code_and_the_structured_reason() {
    let diag = Diag::new(DiagKind::Incompatible, "fluxes do not balance")
        .at(1)
        .in_op("poisson_fd_bc");
    let error = KernelError::from_diag(diag);
    assert_eq!(error.code, "rssn.diag.incompatible");
    assert_eq!(error.operator.as_deref(), Some("poisson_fd_bc"));
    assert_eq!(error.variant(), Some("HostDiagnostic"));
    let found = error.diag.as_deref().expect("the finding is kept");
    assert_eq!((found.kind, found.arg), (DiagKind::Incompatible, Some(1)));
}

#[test]
fn describe_names_the_wrapper_and_the_parameter() {
    let error = KernelError::from_diag(
        Diag::new(DiagKind::Domain, "axis 2 out of range for rank 2")
            .at(2)
            .in_op("tensordot"),
    );
    let params = wrapper_params("tensordot_2_2_1", "tensordot").expect("a curated wrapper");
    assert_eq!(params, ["a", "b", "axes_a", "axes_b"]);
    assert_eq!(
        error.describe(Some("tensordot_2_2_1"), Some(&params)),
        "tensordot_2_2_1: argument `axes_a` (#3): axis 2 out of range for rank 2"
    );
    // Without a wrapper the operator speaks for itself and the argument is a
    // position.
    assert_eq!(
        error.describe(None, None),
        "tensordot: argument #3: axis 2 out of range for rank 2"
    );
    // A failure that is not a finding keeps its message.
    let plain = KernelError::new("E0899", "kernel exploded");
    assert_eq!(plain.describe(Some("f"), None), "kernel exploded");
}

#[test]
fn a_wrapper_of_another_operator_is_not_blamed() {
    assert!(wrapper_params("tensordot_2_2_1", "advect_fd").is_none());
    assert!(wrapper_params("no_such_wrapper", "tensordot").is_none());
}

#[test]
fn tensordot_with_an_axis_past_the_rank_is_refused_with_the_argument() {
    let tensordot = registry().op("tensordot").expect("tensordot is registered");
    let args = [
        matrix(&[[1.0, 2.0], [3.0, 4.0]]),
        matrix(&[[5.0, 6.0], [7.0, 8.0]]),
        list(vec![int(2)]),
        list(vec![int(0)]),
    ];
    let error = kernel::kernel()
        .call_key(&tensordot.keys[0], &args)
        .expect_err("axis 2 does not exist in a matrix");
    assert_eq!(error.code, "rssn.diag.domain");
    assert_eq!(error.kind, Failure::Other);
    let found = error.diag.as_deref().expect("the kernel's reason");
    assert_eq!(found.op, "tensordot");
    assert_eq!(found.arg, Some(2), "`axes_a` is the third argument");
}

#[test]
fn a_valid_call_is_untouched_by_the_channel() {
    let tensordot = registry().op("tensordot").expect("tensordot is registered");
    let args = [
        matrix(&[[1.0, 2.0], [3.0, 4.0]]),
        matrix(&[[5.0, 6.0], [7.0, 8.0]]),
        list(vec![int(1)]),
        list(vec![int(0)]),
    ];
    let value = kernel::kernel()
        .call_key(&tensordot.keys[0], &args)
        .expect("a matrix product");
    assert!(matches!(value, Val::Tensor { .. }), "{value:?}");
}
