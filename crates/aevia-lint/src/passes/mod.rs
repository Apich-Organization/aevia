//! Built-in syntactic and semantic lint passes.

pub mod dimension_literal;
pub mod duplicate_import;
pub mod float_equality;
pub mod missing_doc;
pub mod naming;
pub mod redundant_annotation;
pub mod redundant_if;
pub mod self_comparison;
pub mod shadowed_prelude;
pub mod unused_import;

use crate::pass::SemanticPass;
use crate::pass::SyntaxPass;

/// Returns all built-in syntactic passes.
#[must_use]
pub fn syntax_passes() -> Vec<Box<dyn SyntaxPass + Sync>> {
    vec![
        Box::new(naming::NamingPass),
        Box::new(missing_doc::MissingDocPass),
        Box::new(redundant_annotation::RedundantAnnotationPass),
        Box::new(redundant_if::RedundantIfPass),
        Box::new(self_comparison::SelfComparisonPass),
        Box::new(shadowed_prelude::ShadowedPreludePass),
        Box::new(unused_import::UnusedImportPass),
        Box::new(duplicate_import::DuplicateImportPass),
    ]
}

/// Returns all built-in semantic passes.
#[must_use]
pub fn semantic_passes() -> Vec<Box<dyn SemanticPass + Sync>> {
    vec![
        Box::new(float_equality::FloatEqualityPass),
        Box::new(dimension_literal::DimensionLiteralPass),
    ]
}
