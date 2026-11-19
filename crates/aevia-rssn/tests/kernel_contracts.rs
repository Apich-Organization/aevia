//! Run the kernel's focused ABI/AD/callback contracts in the host build profile.
//!
//! Both repositories use the same source and binary dependencies here. This
//! avoids rebuilding the scientific engine under a second test profile just
//! to validate the bridge; the kernel still owns the actual regression tests.

#![allow(
    clippy::duplicate_mod,
    reason = "independent RSSN integration tests intentionally share small support modules"
)]

#[path = "../../../../rssn/tests/callbacks.rs"]
mod callbacks;
#[path = "../../../../rssn/tests/numerical_abi.rs"]
mod numerical_abi;
#[path = "../../../../rssn/tests/tape_autodiff.rs"]
mod tape_autodiff;

#[path = "../../../../rssn/tests/dimensioned_coverage.rs"]
mod dimensioned_coverage;
#[path = "../../../../rssn/tests/pde_adaptive.rs"]
mod pde_adaptive;

#[path = "../../../../rssn/tests/ode_adaptive_geometry.rs"]
mod ode_adaptive_geometry;

#[path = "../../../../rssn/tests/portable_model_validation.rs"]
mod portable_model_validation;
#[path = "../../../../rssn/tests/spline_descriptor_validation.rs"]
mod spline_descriptor_validation;

#[path = "../../../../rssn/tests/source_policy_math.rs"]
mod source_policy_math;

#[path = "../../../../rssn/tests/contraction_contract.rs"]
mod contraction_contract;

#[path = "../../../../rssn/tests/native_models.rs"]
mod native_models;
#[path = "../../../../rssn/tests/native_objects.rs"]
mod native_objects;
#[path = "../../../../rssn/tests/native_source_families.rs"]
mod native_source_families;

#[path = "../../../../rssn/tests/native_reverse_ad.rs"]
mod native_reverse_ad;

#[path = "../../../../rssn/tests/native_homology.rs"]
mod native_homology;

#[path = "../../../../rssn/tests/native_backend.rs"]
mod native_backend;

#[path = "../../../../rssn/tests/native_callbacks.rs"]
mod native_callbacks;

#[path = "../../../../rssn/tests/native_linear.rs"]
mod native_linear;

#[path = "../../../../rssn/tests/native_grids.rs"]
mod native_grids;

#[path = "../../../../rssn/tests/native_mechanics.rs"]
mod native_mechanics;

#[path = "../../../../rssn/tests/native_discrete.rs"]
mod native_discrete;

#[path = "../../../../rssn/tests/analysis_native.rs"]
mod analysis_native;

#[path = "../../../../rssn/tests/native_execution.rs"]
mod native_execution;

#[path = "../../../../rssn/tests/native_fdm_callbacks.rs"]
mod native_fdm_callbacks;

#[path = "../../../../rssn/tests/native_fem.rs"]
mod native_fem;
#[path = "../../../../rssn/tests/structured_linear_validation.rs"]
mod structured_linear_validation;

#[path = "../../../../rssn/tests/native_particles.rs"]
mod native_particles;

#[path = "../../../../rssn/tests/spectral_odd_grid.rs"]
mod spectral_odd_grid;

#[path = "../../../../rssn/tests/native_legacy_physics.rs"]
mod native_legacy_physics;

#[path = "../../../../rssn/tests/native_continuum.rs"]
mod native_continuum;

#[path = "../../../../rssn/tests/native_flow_conservation.rs"]
mod native_flow_conservation;
