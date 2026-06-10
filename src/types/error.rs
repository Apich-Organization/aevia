//! Type error definitions with source span support for miette diagnostics.

use miette::{Diagnostic, SourceSpan};
use std::fmt;

/// A dimensional type error produced by the checker.
#[derive(Debug, Clone)]
pub struct TypeError {
    /// Human-readable description of the error.
    pub message: String,
    /// Where in the source the error occurred.
    pub span: SourceSpan,
    /// Optional secondary label (e.g. the conflicting type's span).
    pub secondary: Option<(SourceSpan, String)>,
    /// The error code tag.
    pub code: TypeErrorCode,
}

/// Structured error codes for dimensional type errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeErrorCode {
    /// Tried to add/subtract values with incompatible dimensions.
    DimensionMismatch,
    /// A declared dimension annotation doesn't match the inferred type.
    AnnotationConflict,
    /// Could not resolve a named base unit or type alias.
    UnknownUnit,
    /// The return expression dimension doesn't match the declared return type.
    ReturnTypeMismatch,
    /// A function argument has the wrong dimension.
    ArgumentDimension,
    /// Used a dimensionless value where a physical dimension was expected.
    UnexpectedDimensionless,
    /// Tensor shape mismatch (e.g. adding tensors of different shapes).
    ShapeMismatch,
}

impl fmt::Display for TypeErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::DimensionMismatch       => "E[dim::mismatch]",
            Self::AnnotationConflict      => "E[dim::annotation]",
            Self::UnknownUnit             => "E[dim::unknown_unit]",
            Self::ReturnTypeMismatch      => "E[dim::return_type]",
            Self::ArgumentDimension       => "E[dim::argument]",
            Self::UnexpectedDimensionless => "E[dim::dimensionless]",
            Self::ShapeMismatch           => "E[dim::shape_mismatch]",
        };
        write!(f, "{s}")
    }
}

impl fmt::Display for TypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.code, self.message)
    }
}

impl std::error::Error for TypeError {}

impl Diagnostic for TypeError {
    fn code<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        Some(Box::new(self.code))
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = miette::LabeledSpan> + '_>> {
        let mut labels = vec![miette::LabeledSpan::new(
            Some(self.message.clone()),
            self.span.offset(),
            self.span.len(),
        )];
        if let Some((sec_span, sec_msg)) = &self.secondary {
            labels.push(miette::LabeledSpan::new(
                Some(sec_msg.clone()),
                sec_span.offset(),
                sec_span.len(),
            ));
        }
        Some(Box::new(labels.into_iter()))
    }
}

impl TypeError {
    /// Construct a basic type error with a span.
    pub fn new(code: TypeErrorCode, message: impl Into<String>, span: SourceSpan) -> Self {
        Self {
            message: message.into(),
            span,
            secondary: None,
            code,
        }
    }

    /// Attach a secondary labeled span (e.g. the conflicting definition).
    #[must_use]
    pub fn with_secondary(mut self, span: SourceSpan, label: impl Into<String>) -> Self {
        self.secondary = Some((span, label.into()));
        self
    }

    // ── Convenience constructors ───────────────────────────────────────────

    pub fn dimension_mismatch(
        span: SourceSpan,
        got: &str,
        expected: &str,
    ) -> Self {
        Self::new(
            TypeErrorCode::DimensionMismatch,
            format!("dimension mismatch: expected `{expected}`, found `{got}`"),
            span,
        )
    }

    pub fn annotation_conflict(
        span: SourceSpan,
        declared: &str,
        inferred: &str,
    ) -> Self {
        Self::new(
            TypeErrorCode::AnnotationConflict,
            format!("declared type `{declared}` conflicts with inferred `{inferred}`"),
            span,
        )
    }

    pub fn unknown_unit(span: SourceSpan, name: &str) -> Self {
        Self::new(
            TypeErrorCode::UnknownUnit,
            format!("unknown unit or type alias `{name}`"),
            span,
        )
    }

    pub fn return_type_mismatch(span: SourceSpan, declared: &str, inferred: &str) -> Self {
        Self::new(
            TypeErrorCode::ReturnTypeMismatch,
            format!("return type mismatch: declared `{declared}`, body evaluates to `{inferred}`"),
            span,
        )
    }

    pub fn shape_mismatch(span: SourceSpan, got: &str, expected: &str) -> Self {
        Self::new(
            TypeErrorCode::ShapeMismatch,
            format!("tensor shape mismatch: expected `{expected}`, found `{got}`"),
            span,
        )
    }
}
