//! Cumulative probabilities carry the dimension of a scalar probability.

use aevia_dim::Dim;
use aevia_rssn::Num;
use aevia_rssn::Val;
use aevia_rssn::kernel;
use aevia_rssn::registry;
use aevia_rssn::witness::DimensionWitness;
use rssn::kernel::signature::DimRuleInfo;

#[test]
fn graph_cdf_proves_a_unitless_float_for_a_dynamic_caller() {
    let entry = registry().op("cdf").unwrap();
    assert_eq!(entry.dim_rule, Some(DimRuleInfo::CompatibleDimensionless));
    let host = kernel();
    let unitless = DimensionWitness::Scalar(Dim::one());
    let (normal, proof) = host
        .call_with_witnesses(
            "normal",
            &[Val::Num(Num::Float(0.0)), Val::Num(Num::Float(1.0))],
            &[unitless.clone(), unitless.clone()],
        )
        .unwrap();
    let point = Val::Num(Num::Float(-10.0));
    let (probability, dimension) = host
        .call_with_witnesses(
            "cdf",
            &[normal.clone(), point.clone()],
            &[proof.clone(), unitless.clone()],
        )
        .unwrap();
    assert_eq!(dimension, unitless);
    let Val::Num(Num::Float(probability)) = probability else {
        panic!("expected a concrete floating probability")
    };
    assert!((probability / 7.619_853_024_160_526e-24 - 1.0).abs() < 3e-13);

    assert!(
        host.call_with_witnesses(
            "cdf",
            &[normal, point],
            &[proof, DimensionWitness::Scalar(Dim::base("Time"))],
        )
        .is_err()
    );
}
