//! Source-of-truth for standard-library generation, independent of generated files.
#![allow(clippy::type_complexity)] // the generation tables are nested tuples by design
pub mod spec;
pub mod stdlib;
