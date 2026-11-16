//! Lint level resolution: defaults → `aevia.toml [lints]` → CLI flags → `@allow/@warn/@deny/@forbid`.

use std::collections::BTreeMap;

use aevia_core::manifest::LintSetting;
use aevia_syntax::Diagnostic;
use aevia_syntax::Diagnostics;
use aevia_syntax::FileId;
use aevia_syntax::Severity;
use aevia_syntax::Span;
use aevia_syntax::ast::Attribute;
use aevia_syntax::ast::BinOp;
use aevia_syntax::ast::Decl;
use aevia_syntax::ast::Expr;
use aevia_syntax::ast::ExprKind;
use aevia_syntax::ast::Module;
use aevia_syntax::ast::StrPiece;
use aevia_syntax::visit;

use crate::lint::Level;
use crate::lint::LintId;
use crate::lint::LintStore;
use crate::pass::LintDiagnostic;

/// A single `-A/-W/-D/-F <target>` CLI flag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CliOverride {
    /// Target level.
    pub level: Level,
    /// Lint name, code, or group (`warnings`, `style`, `correctness`, `pedantic`, `all`).
    pub target: String,
}

/// Configuration inputs to the lint driver.
#[derive(Clone, Debug, Default)]
pub struct LintConfig {
    /// `[lints]` table from `aevia.toml`.
    pub manifest_lints: BTreeMap<String, LintSetting>,
    /// Ordered CLI `-A/-W/-D/-F` overrides.
    pub cli_overrides: Vec<CliOverride>,
    /// Whether this run executes extra `aevia lint` passes (`true`) or compiler-only (`false`).
    pub run_extra_lints: bool,
}

#[derive(Clone, Debug)]
struct ScopeOverride {
    span: Span,
    lint: LintId,
    level: Level,
    attr_span: Span,
}

/// Resolved per-module lint levels, including `@allow/@warn/@deny/@forbid` scope overrides.
#[derive(Clone, Debug)]
pub struct LevelMap {
    base: Vec<Level>,
    scoped: Vec<ScopeOverride>,
    /// Configuration/attribute errors (`E0211` forbid override, `E0212` unknown lint name).
    pub errors: Vec<Diagnostic>,
}

impl LevelMap {
    /// Effective level of `lint` at `span` (walks enclosing attribute scopes innermost-first).
    /// An enclosing `Forbid` cannot be lowered.
    #[must_use]
    pub fn level_at(
        &self,
        lint: LintId,
        span: Span,
    ) -> Level {
        let base = self.base.get(lint.0).copied().unwrap_or(Level::Allow);
        if base == Level::Forbid {
            return Level::Forbid;
        }
        let mut enclosing: Vec<&ScopeOverride> = self
            .scoped
            .iter()
            .filter(|o| {
                o.lint == lint
                    && o.span.file == span.file
                    && o.span.start <= span.start
                    && span.end <= o.span.end
            })
            .collect();
        // Outermost to innermost (larger span first, smaller span last).
        enclosing.sort_by_key(|o| std::cmp::Reverse(o.span.end.saturating_sub(o.span.start)));

        let mut current = base;
        for entry in enclosing {
            if current == Level::Forbid {
                return Level::Forbid;
            }
            current = entry.level;
        }
        current
    }

    /// True if `lint` can fire anywhere in this module (base != Allow or any inner `@warn/@deny/@forbid`).
    #[must_use]
    pub fn is_enabled_anywhere(
        &self,
        lint: LintId,
    ) -> bool {
        if self.base.get(lint.0).copied().unwrap_or(Level::Allow) != Level::Allow {
            return true;
        }
        self.scoped
            .iter()
            .any(|o| o.lint == lint && o.level != Level::Allow)
    }

    /// Deterministic fingerprint of this module's resolved levels for disk caching.
    #[must_use]
    pub fn fingerprint(&self) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for lvl in &self.base {
            h = (h ^ (*lvl as u64)).wrapping_mul(0x0000_0100_0000_01b3);
        }
        for s in &self.scoped {
            h = (h ^ u64::from(s.span.start)).wrapping_mul(0x0000_0100_0000_01b3);
            h = (h ^ u64::from(s.span.end)).wrapping_mul(0x0000_0100_0000_01b3);
            h = (h ^ (s.lint.0 as u64)).wrapping_mul(0x0000_0100_0000_01b3);
            h = (h ^ (s.level as u64)).wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }
}

/// Resolves lint levels from defaults, `aevia.toml`, CLI flags, and AST attributes.
pub struct LevelResolver;

impl LevelResolver {
    /// Builds a [`LevelMap`] for a single module.
    #[must_use]
    pub fn resolve(
        store: &LintStore,
        config: &LintConfig,
        module: Option<&Module>,
        is_root_package: bool,
    ) -> LevelMap {
        let count = store.all().len();
        let mut base: Vec<Level> = if is_root_package {
            store.all().iter().map(|l| l.default_level).collect()
        } else {
            // `--cap-lints allow` for dependencies.
            vec![Level::Allow; count]
        };
        let mut errors = Vec::new();

        if is_root_package {
            for (target, setting) in &config.manifest_lints {
                let Some(ids) = store.expand_target(target) else {
                    errors.push(Diagnostic::error(
                        "E0212",
                        format!("unknown lint or group `{target}` in `[lints]`"),
                        Span::default(),
                    ));
                    continue;
                };
                let new_level = Level::from_manifest(setting.level);
                for id in ids {
                    if base[id.0] == Level::Forbid && new_level != Level::Forbid {
                        errors.push(Diagnostic::error(
                            "E0211",
                            format!(
                                "cannot lower forbidden lint `{}`",
                                store.get(id).map_or("", |l| l.name)
                            ),
                            Span::default(),
                        ));
                    } else {
                        base[id.0] = new_level;
                    }
                }
            }

            for ov in &config.cli_overrides {
                let Some(ids) = store.expand_target(&ov.target) else {
                    errors.push(Diagnostic::error(
                        "E0212",
                        format!("unknown lint or group `{}`", ov.target),
                        Span::default(),
                    ));
                    continue;
                };
                for id in ids {
                    if base[id.0] == Level::Forbid && ov.level != Level::Forbid {
                        errors.push(Diagnostic::error(
                            "E0211",
                            format!(
                                "cannot lower forbidden lint `{}` with CLI flag",
                                store.get(id).map_or("", |l| l.name)
                            ),
                            Span::default(),
                        ));
                    } else {
                        base[id.0] = ov.level;
                    }
                }
            }
        }

        let mut scoped = Vec::new();
        if let Some(module) = module
            && is_root_package
        {
            let mut collector = AttrCollector {
                store,
                scoped: &mut scoped,
                errors: &mut errors,
            };
            visit::walk_module(&mut collector, module);

            // Check that no inner attribute lowers an outer `Forbid`.
            for entry in &scoped {
                if entry.level == Level::Forbid {
                    continue;
                }
                let base_forbid = base[entry.lint.0] == Level::Forbid;
                let outer_forbid = scoped.iter().any(|outer| {
                    outer.lint == entry.lint
                        && outer.level == Level::Forbid
                        && outer.span.file == entry.span.file
                        && outer.span.start <= entry.span.start
                        && entry.span.end <= outer.span.end
                        && outer.attr_span != entry.attr_span
                });
                if base_forbid || outer_forbid {
                    errors.push(Diagnostic::error(
                        "E0211",
                        format!(
                            "cannot override forbidden lint `{}` with `@{}`",
                            store.get(entry.lint).map_or("", |l| l.name),
                            entry.level.as_str()
                        ),
                        entry.attr_span,
                    ));
                }
            }
        }

        LevelMap { base, scoped, errors }
    }

    /// Filters and rewrites compiler diagnostics (from `aevia-types`) according to `levels`.
    #[must_use]
    pub fn apply_to_compiler_diagnostics(
        store: &LintStore,
        levels: &LevelMap,
        diagnostics: &Diagnostics,
        file: FileId,
    ) -> (Vec<LintDiagnostic>, Diagnostics) {
        let mut lint_diags = Vec::new();
        let mut out = Diagnostics::new();

        for err in &levels.errors {
            if err.span.file == file || err.span == Span::default() {
                out.push(err.clone());
            }
        }

        for diag in &diagnostics.items {
            if diag.span.file != file {
                continue;
            }
            if diag.severity != Severity::Warning {
                out.push(diag.clone());
                continue;
            }
            let Some((id, lint)) = store.find_by_code(&diag.code) else {
                out.push(diag.clone());
                continue;
            };
            let level = levels.level_at(id, diag.span);
            if level == Level::Allow {
                continue;
            }
            let mut rewritten = diag.clone();
            if matches!(level, Level::Deny | Level::Forbid) {
                rewritten.severity = Severity::Error;
            }
            out.push(rewritten.clone());
            lint_diags.push(LintDiagnostic {
                diagnostic: rewritten,
                lint,
                level,
                fixes: Vec::new(),
            });
        }
        (lint_diags, out)
    }
}

struct AttrCollector<'a> {
    store: &'a LintStore,
    scoped: &'a mut Vec<ScopeOverride>,
    errors: &'a mut Vec<Diagnostic>,
}

impl<'ast> visit::Visitor<'ast> for AttrCollector<'_> {
    fn visit_decl(
        &mut self,
        decl: &'ast Decl,
    ) {
        for attr in &decl.attrs {
            let Some(level) = Level::parse(&attr.name) else {
                continue;
            };
            self.collect_attr(attr, level, decl.span);
        }
        visit::walk_decl(self, decl);
    }
}

impl AttrCollector<'_> {
    fn collect_attr(
        &mut self,
        attr: &Attribute,
        level: Level,
        scope_span: Span,
    ) {
        if attr.args.is_empty() {
            self.errors.push(Diagnostic::error(
                "E0210",
                format!("`@{}` requires at least one lint name", attr.name),
                attr.span,
            ));
            return;
        }
        let mut targets = Vec::new();
        for arg in &attr.args {
            if let Some(name) = &arg.name {
                if name == "reason" {
                    if string_literal(&arg.value).is_none() {
                        self.errors.push(Diagnostic::error(
                            "E0210",
                            "`reason` in lint attribute must be a string literal",
                            arg.value.span,
                        ));
                    }
                    continue;
                }
                self.errors.push(Diagnostic::error(
                    "E0210",
                    format!("unknown named argument `{name}` in `@{}`", attr.name),
                    arg.value.span,
                ));
                continue;
            }
            let Some(target) = extract_lint_name(&arg.value) else {
                self.errors.push(Diagnostic::error(
                    "E0210",
                    format!("expected a lint or group name in `@{}`", attr.name),
                    arg.value.span,
                ));
                continue;
            };
            let Some(ids) = self.store.expand_target(&target) else {
                self.errors.push(Diagnostic::error(
                    "E0212",
                    format!("unknown lint or group `{target}`"),
                    arg.value.span,
                ));
                continue;
            };
            targets.extend(ids);
        }
        for lint in targets {
            self.scoped.push(ScopeOverride {
                span: scope_span,
                lint,
                level,
                attr_span: attr.span,
            });
        }
    }
}

fn string_literal(expr: &Expr) -> Option<String> {
    let ExprKind::Str(pieces) = &expr.kind else {
        return None;
    };
    let mut out = String::new();
    for piece in pieces {
        let StrPiece::Text(text) = piece else {
            return None;
        };
        out.push_str(text);
    }
    Some(out)
}

/// Reconstructs `unused-variable`, `unused_variable`, `W0101`, or `"unused-variable"` from an attribute arg.
fn extract_lint_name(expr: &Expr) -> Option<String> {
    if let Some(text) = string_literal(expr) {
        return Some(text);
    }
    match &expr.kind {
        | ExprKind::Var(name) => Some(name.clone()),
        | ExprKind::QName(segs) if segs.len() == 1 => Some(segs[0].clone()),
        | ExprKind::Binary {
            op: BinOp::Sub,
            left,
            right,
        } => {
            Some(format!(
                "{}-{}",
                extract_lint_name(left)?,
                extract_lint_name(right)?
            ))
        },
        | _ => None,
    }
}
