//! Run the kernel's focused ABI/AD/callback contracts in the host build profile.
//!
//! Both repositories use the same source and binary dependencies here. This
//! avoids rebuilding the scientific engine under a second test profile just
//! to validate the bridge; the kernel still owns the actual regression tests.
#[path = "../../../../rssn/tests/callbacks.rs"]
mod callbacks;
#[path = "../../../../rssn/tests/numerical_abi.rs"]
mod numerical_abi;
#[path = "../../../../rssn/tests/tape_autodiff.rs"]
mod tape_autodiff;
