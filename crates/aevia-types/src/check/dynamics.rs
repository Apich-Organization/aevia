//! Type checking for ODE and SDE solver entry points.

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

use super::Checker;
use super::builtin_type;
use crate::adt;
use crate::ty::Shape;
use crate::ty::Type;
use aevia_dim::Dim;
use aevia_syntax::Span;
use aevia_syntax::ast::Expr;

enum OdeOutput<'a> {
    Events(&'a Expr, &'a Expr),
    Boundary(&'a Expr, &'a Expr),
    Higher(Option<&'a Expr>),
}

impl Checker {
    pub(super) fn ode_call(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Span,
    ) -> Type {
        let (rhs, initial, input, options) = match (name, args) {
            | ("odeint", [rhs, initial, input]) => (rhs, initial, input, None),
            | ("odeint_with" | "solve_ivp", [rhs, initial, input, options]) => {
                (rhs, initial, input, Some(options))
            },
            | _ => {
                let expected = if name == "odeint" { 3 } else { 4 };
                return self.invalid_solver_arity(name, expected, args, span);
            },
        };
        let time = Type::float(Dim::base("Time"));
        let Some(state) = self.first_order_rate(name, rhs, &time, false, span) else {
            return self.synth_solver_tail(args.iter().skip(1));
        };
        self.check(initial, &state);
        let grid = series(time.clone());
        if name == "solve_ivp" {
            self.check(input, &time_interval(&time));
        } else {
            self.check(input, &grid);
        }
        let trajectory = series(state);
        if let Some(options) = options {
            self.check_ode_options(options);
            Type::record([("t".into(), grid), ("y".into(), trajectory)])
        } else {
            trajectory
        }
    }

    pub(super) fn ode_ext_call(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Span,
    ) -> Type {
        let (rhs, initial, input, output) = match (name, args) {
            | ("ode_solve", [rhs, initial, input, options, events]) => {
                (rhs, initial, input, OdeOutput::Events(options, events))
            },
            | ("ode_bvp", [rhs, initial, input, boundary, options]) => {
                (rhs, initial, input, OdeOutput::Boundary(options, boundary))
            },
            | ("odeint_order", [rhs, initial, input]) => {
                (rhs, initial, input, OdeOutput::Higher(None))
            },
            | ("odeint_order_with", [rhs, initial, input, options]) => {
                (rhs, initial, input, OdeOutput::Higher(Some(options)))
            },
            | _ => {
                let expected = match name {
                    | "ode_solve" | "ode_bvp" => 5,
                    | "odeint_order" => 3,
                    | _ => 4,
                };
                return self.invalid_solver_arity(name, expected, args, span);
            },
        };
        let time = Type::float(Dim::base("Time"));
        match output {
            | OdeOutput::Higher(options) => {
                self.higher_ode_call(name, rhs, initial, input, options, span)
            },
            | OdeOutput::Events(options, constraint) | OdeOutput::Boundary(options, constraint) => {
                let Some(state) = self.first_order_rate(name, rhs, &time, false, span) else {
                    return self.synth_solver_tail(args.iter().skip(1));
                };
                self.check(initial, &state);
                self.check(input, &time_interval(&time));
                if matches!(output, OdeOutput::Boundary(..)) {
                    let residual_dim = self.infer.fresh_dim();
                    let boundary = Type::func(
                        vec![state.clone(), state.clone()],
                        series(Type::float(residual_dim)),
                    );
                    self.check(constraint, &boundary);
                    self.check_ode_options(options);
                } else {
                    self.check_ode_options(options);
                    self.check(constraint, &series(event_record_type(&state, &time)));
                }
                ode_solution_type(&state, &time)
            },
        }
    }

    #[allow(
        clippy::single_call_fn,
        reason = "pairs every initial derivative with the corresponding callback parameter"
    )]
    fn higher_ode_call(
        &mut self,
        name: &str,
        rhs: &Expr,
        initial: &Expr,
        times: &Expr,
        options: Option<&Expr>,
        span: Span,
    ) -> Type {
        let time = Type::float(Dim::base("Time"));
        let maybe_rhs = self.synth(rhs);
        let resolved = self.infer.resolve(&maybe_rhs);
        let Type::Fn(params, result) = resolved else {
            return self.invalid_higher_rhs(name, rhs, initial, times, options, &resolved);
        };
        let [argument_time, state, derivatives @ ..] = params.as_slice() else {
            return self.invalid_higher_rhs(
                name,
                rhs,
                initial,
                times,
                options,
                &Type::Fn(params, result),
            );
        };
        self.unify_solver_type(argument_time, &time, rhs.span);
        let state = self.infer.resolve(state);
        let mut fields = vec![("0".to_string(), state.clone())];
        let mut derivative = self.rate_of(&state, span);
        for parameter in derivatives {
            if !derivative.is_error() {
                self.unify_solver_type(parameter, &derivative, rhs.span);
            }
            fields.push((fields.len().to_string(), derivative.clone()));
            if !derivative.is_error() {
                derivative = self.rate_of(&derivative, span);
            }
        }
        if !derivative.is_error() {
            self.unify_solver_type(&result, &derivative, rhs.span);
        }
        let initial_type = Type::record(fields);
        let grid = series(time);
        self.check(initial, &initial_type);
        self.check(times, &grid);
        if let Some(options) = options {
            self.check_ode_options(options);
        }
        Type::record([
            ("t".into(), grid),
            ("y".into(), series(state)),
            ("d".into(), series(initial_type)),
        ])
    }

    fn invalid_higher_rhs(
        &mut self,
        name: &str,
        rhs: &Expr,
        initial: &Expr,
        times: &Expr,
        options: Option<&Expr>,
        actual: &Type,
    ) -> Type {
        self.error_help(
            "E0302",
            format!(
                "the right-hand side of `{name}` must be a function `(Float[T], y, y', …, y⁽ⁿ⁻¹⁾) -> y⁽ⁿ⁾`, found {actual}"
            ),
            rhs.span,
            "write `fn rhs(_t: Float[T], s: State): Rate = …`",
        );
        self.synth(initial);
        self.synth(times);
        if let Some(options) = options {
            self.synth(options);
        }
        Type::Error
    }

    pub(super) fn sde_call(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Span,
    ) -> Type {
        let (entry, noise, initial, policy, count) = match args {
            | [entry, noise, initial, policy] if !name.starts_with("sde_ensemble_") => {
                (entry, noise, initial, policy, None)
            },
            | [entry, noise, initial, policy, count] if name.starts_with("sde_ensemble_") => {
                (entry, noise, initial, policy, Some(count))
            },
            | _ => {
                let expected = if name.starts_with("sde_ensemble_") {
                    5
                } else {
                    4
                };
                return self.invalid_solver_arity(name, expected, args, span);
            },
        };
        let name = if count.is_some() {
            if name.ends_with("general") {
                "sdeint_general"
            } else {
                "sdeint_diag"
            }
        } else {
            name
        };
        if let Some(count) = count {
            self.check(count, &Type::int());
        }
        let time = Type::float(Dim::base("Time"));
        let Some(state) = self.first_order_rate(name, entry, &time, true, span) else {
            return self.synth_solver_tail([noise, initial, policy]);
        };
        let diffusion = if name == "sdeint_diag" {
            self.diffusion_of(&state, span)
        } else {
            let Some(expected) = self.general_sde_noise(&state, span) else {
                return self.synth_solver_tail([noise, initial, policy]);
            };
            expected
        };
        self.check_noise(name, noise, &time, &state, &diffusion);
        self.check(initial, &state);
        let policy = self.synth(policy);
        let policy = self.infer.resolve(&policy);
        // Recover the nominal method from the selected policy instead of
        // assuming how its defining module was imported.
        let method = sde_policy_method_type(&policy)
            .unwrap_or_else(|| builtin_type("SdeMethod").unwrap_or(Type::Error));
        let path = Type::record([
            ("t".into(), series(time)),
            ("y".into(), series(state)),
            ("steps".into(), Type::int()),
            ("rejected".into(), Type::int()),
            ("nfev".into(), Type::int()),
            ("seed".into(), Type::int()),
            ("noise".into(), Type::int()),
            ("method".into(), method),
        ]);
        let result = Type::result(path, builtin_type("SdeError").unwrap_or(Type::Error));
        if count.is_some() {
            series(result)
        } else {
            result
        }
    }

    fn first_order_rate(
        &mut self,
        name: &str,
        expr: &Expr,
        time: &Type,
        sde: bool,
        span: Span,
    ) -> Option<Type> {
        let rhs = self.synth(expr);
        let resolved = self.infer.resolve(&rhs);
        if let Type::Fn(params, ret) = &resolved
            && let [parameter_time, parameter_state] = params.as_slice()
        {
            self.unify_solver_type(parameter_time, time, expr.span);
            let state = self.infer.resolve(parameter_state);
            let rate = self.rate_of(&state, span);
            if !rate.is_error() {
                self.unify_solver_type(ret, &rate, expr.span);
            }
            return Some(state);
        }
        let (role, hint) = if sde {
            (
                "drift",
                "write `fn drift(_t: Float[T], s: State): RateOf<State> = …`",
            )
        } else {
            (
                "right-hand side",
                "write `fn rhs(_t: Float[T], s: State): Rate = …`",
            )
        };
        self.error_help(
            "E0302",
            format!("the {role} of `{name}` must be a function `(Float[T], s) -> RateOf<s>`, found {resolved}"),
            expr.span,
            hint,
        );
        None
    }

    #[allow(
        clippy::single_call_fn,
        reason = "types the general-noise layout independently from callback validation"
    )]
    fn general_sde_noise(
        &mut self,
        state: &Type,
        span: Span,
    ) -> Option<Type> {
        match self.infer.resolve(state) {
            | scalar @ Type::Scalar(..) => {
                Some(Type::tensor(
                    Shape::Rank(2),
                    self.diffusion_of(&scalar, span),
                ))
            },
            | Type::Tensor { shape, elem } if shape.rank() == 1 => {
                Some(Type::tensor(Shape::Rank(2), self.diffusion_of(&elem, span)))
            },
            | record @ Type::Record(_) => Some(self.general_diffusion_of(&record, span)),
            | other => {
                self.error_help(
                    "E0302",
                    format!(
                        "`sdeint_general` needs a scalar, rank-1 or record state, found {other}"
                    ),
                    span,
                    "pass a `Float`, a `Tensor<1, Float>` or a closed record of them as the state",
                );
                None
            },
        }
    }

    #[allow(
        clippy::single_call_fn,
        reason = "diffusion has different units and may have extra axes relative to drift"
    )]
    fn check_noise(
        &mut self,
        name: &str,
        expr: &Expr,
        time: &Type,
        state: &Type,
        diffusion: &Type,
    ) {
        let ty = self.synth(expr);
        let resolved = self.infer.resolve(&ty);
        if let Type::Fn(params, result) = &resolved
            && let [parameter_time, parameter_state] = params.as_slice()
        {
            self.unify_solver_type(parameter_time, time, expr.span);
            self.unify_solver_type(parameter_state, state, expr.span);
            if !diffusion.is_error() {
                self.unify_solver_type(result, diffusion, expr.span);
            }
        } else {
            self.error_help(
                "E0302",
                format!("the diffusion of `{name}` must be a function `(Float[T], s) -> DiffusionOf<s>`, found {resolved}"),
                expr.span,
                "write `fn noise(_t: Float[T], s: State): DiffusionOf<State> = …`",
            );
        }
    }

    fn check_ode_options(
        &mut self,
        options: &Expr,
    ) {
        let ty = self.synth(options);
        let Type::Record(row) = self.infer.resolve(&ty) else {
            return;
        };
        for (name, expected) in [
            ("rtol", Dim::one()),
            ("h0", Dim::base("Time")),
            ("h_min", Dim::base("Time")),
            ("h_max", Dim::base("Time")),
        ] {
            let Some(field) = row.field(name) else {
                continue;
            };
            let field = self.force_scalar(field);
            if let Type::Scalar(_, dimension) = field {
                if let Err(error) = self.infer.unify_dim(&dimension, &expected) {
                    self.report_unify(error, options.span);
                }
            } else if !field.is_error() {
                self.error(
                    "E0302",
                    format!("ODE option `{name}` must be a numeric scalar, found {field}"),
                    options.span,
                );
            }
        }
    }

    fn unify_solver_type(
        &mut self,
        actual: &Type,
        expected: &Type,
        span: Span,
    ) {
        if let Err(error) = self.infer.unify(actual, expected) {
            self.report_unify(error, span);
        }
    }

    fn invalid_solver_arity(
        &mut self,
        name: &str,
        expected: usize,
        args: &[Expr],
        span: Span,
    ) -> Type {
        self.error(
            "E0315",
            format!("`{name}` takes {expected} arguments, found {}", args.len()),
            span,
        );
        for arg in args {
            self.synth(arg);
        }
        Type::Error
    }

    fn synth_solver_tail<'a>(
        &mut self,
        args: impl IntoIterator<Item = &'a Expr>,
    ) -> Type {
        for arg in args {
            self.synth(arg);
        }
        Type::Error
    }
}

fn series(element: Type) -> Type {
    Type::tensor(Shape::Rank(1), element)
}

fn time_interval(time: &Type) -> Type {
    Type::record([("0".into(), time.clone()), ("1".into(), time.clone())])
}

#[allow(
    clippy::single_call_fn,
    reason = "assembles the event callback schema for event-based integration"
)]
fn event_record_type(
    state: &Type,
    time: &Type,
) -> Type {
    Type::record([
        (
            "g".into(),
            Type::func(vec![time.clone(), state.clone()], Type::float(Dim::one())),
        ),
        ("terminal".into(), Type::bool()),
        ("direction".into(), Type::int()),
    ])
}

#[allow(
    clippy::single_call_fn,
    reason = "keeps the public ODE solution fields in one definition"
)]
fn ode_solution_type(
    state: &Type,
    time: &Type,
) -> Type {
    let hit = Type::record([
        ("index".into(), Type::int()),
        ("t".into(), time.clone()),
        ("y".into(), state.clone()),
    ]);
    Type::record([
        ("t".into(), series(time.clone())),
        ("y".into(), series(state.clone())),
        ("at".into(), Type::func(vec![time.clone()], state.clone())),
        ("events".into(), series(hit)),
        ("nfev".into(), Type::int()),
        ("rejected".into(), Type::int()),
        ("method".into(), Type::string()),
    ])
}

/// Finds the nominal method type shared by every concrete SDE policy branch.
#[allow(
    clippy::single_call_fn,
    reason = "derives the result constructor from the selected policy's declarations"
)]
fn sde_policy_method_type(policy: &Type) -> Option<Type> {
    let Type::Adt(id, params) = policy else {
        return None;
    };
    let definition = adt::get(*id)?;
    let mut selected = None;
    for field in definition.ctors.iter().flat_map(|ctor| &ctor.fields) {
        let Type::Record(row) = field.substitute_params(params) else {
            continue;
        };
        let Some(method @ Type::Adt(_, _)) = row.field("method") else {
            continue;
        };
        if selected.as_ref().is_some_and(|previous| previous != method) {
            return None;
        }
        selected = Some(method.clone());
    }
    selected
}

#[cfg(test)]
mod tests {
    use crate::check::check_module;
    use aevia_syntax::Severity;
    use aevia_syntax::lex;
    use aevia_syntax::parse_module;

    fn errors(source: &str) -> Vec<String> {
        let tokens = lex(0, source).expect("source lexes");
        let (module, parsed) = parse_module(0, source, &tokens);
        assert!(!parsed.has_errors(), "{:?}", parsed.items);
        check_module(0, &module)
            .diagnostics
            .items
            .into_iter()
            .filter(|diagnostic| diagnostic.severity == Severity::Error)
            .map(|diagnostic| diagnostic.code)
            .collect()
    }

    #[test]
    fn known_ode_option_fields_check_their_dimensions() {
        let prelude = "fn rhs(_t: Float[T], _y: Float[L]): Float[L/T] = 1.0 m/s\n";
        assert!(errors(&format!(
            "{prelude}let p = odeint_with(rhs, 0.0 m, [0.0 s, 1.0 s], {{ rtol = 1e-5, h0 = 0.1 s }})\n"
        ))
        .is_empty());
        for options in [
            "rtol = 1.0 m",
            "h0 = 1.0 m",
            "h_min = 1.0 m",
            "h_max = 1.0 m",
        ] {
            let codes = errors(&format!(
                "{prelude}let p = odeint_with(rhs, 0.0 m, [0.0 s, 1.0 s], {{ {options} }})\n"
            ));
            assert_eq!(codes, ["E0401"], "{options}");
        }
    }

    #[test]
    fn higher_order_ode_initial_values_use_the_callbacks_derivative_dimensions() {
        let prelude =
            "fn rhs(_t: Float[T], _y: Float[L], _v: Float[L/T]): Float[L/T^2] = 1.0 m/s^2\n";
        assert!(
            errors(&format!(
                "{prelude}let p = odeint_order(rhs, (1.0 m, 1.0 m/s), [0.0 s, 1.0 s])\n"
            ))
            .is_empty()
        );
        assert!(
            errors(&format!(
                "{prelude}let p = odeint_order(rhs, (1.0 m, 1.0 m), [0.0 s, 1.0 s])\n"
            ))
            .contains(&"E0401".to_string())
        );
    }
}
