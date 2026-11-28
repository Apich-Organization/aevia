//! Source-of-truth for standard-library generation, independent of generated files.
#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(
        missing_docs,
        clippy::arithmetic_side_effects,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::nursery,
        clippy::panic,
        clippy::pedantic,
        clippy::single_call_fn,
        clippy::unwrap_used
    )
)]
pub mod guides;
pub mod spec;
pub mod stdlib;
