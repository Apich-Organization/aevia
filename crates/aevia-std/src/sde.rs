//! Stochastic differential equations backed by the RSSN simulation kernel.
//!
//! All integration, jump sampling, and multilevel estimation live in RSSN.

use crate::NumericError;
#[cfg(test)]
use rssn::kernels::sde as kernel;
pub use rssn::sim::stochastic::*;

impl From<NumericError> for SdeError {
    fn from(error: NumericError) -> Self {
        match error {
            | NumericError::NotConverged => {
                Self::StepLimit("the iteration did not converge".into())
            },
            | NumericError::Domain(message) => Self::Invalid(message),
            | other => Self::Invalid(other.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default seed of `Interp`'s language RNG.
    const INTERP_SEED: u64 = 0x2545_f491_4f6c_dd1d;

    /// A boxed drift or diffusion callback, so a test can name the pair.
    type Coeff = Box<dyn FnMut(f64, &[f64]) -> Result<Vec<f64>, SdeError>>;

    /// Geometric Brownian motion: `dX = mu X dt + sigma X dW` (scalar).
    fn gbm(
        mu: f64,
        sigma: f64,
    ) -> (Coeff, Coeff) {
        (
            Box::new(move |_t: f64, y: &[f64]| Ok(vec![mu * y[0]])),
            Box::new(move |_t: f64, y: &[f64]| Ok(vec![sigma * y[0]])),
        )
    }

    /// Ornstein–Uhlenbeck: `dX = theta (mu - X) dt + sigma dW`.
    fn ou(
        theta: f64,
        mu: f64,
        sigma: f64,
    ) -> (Coeff, Coeff) {
        (
            Box::new(move |_t: f64, y: &[f64]| Ok(vec![theta * (mu - y[0])])),
            Box::new(move |_t: f64, _y: &[f64]| Ok(vec![sigma])),
        )
    }

    #[test]
    fn the_stream_is_the_interpreters_rng() {
        // The interpreter's default seed is not public; construct a fresh
        // interpreter and compare its first outputs with the documented mirror.
        let compilation = aevia_core::compile("seed.aev", "let x = 1\n");
        let mut interp = aevia_core::Interp::new(&compilation.checked);
        let mut stream = Stream::new(INTERP_SEED);
        for _ in 0..8 {
            assert_eq!(
                interp.next_random().to_bits(),
                stream.next_unit().to_bits(),
                "the language stream must be the interpreter's LCG"
            );
        }
    }

    #[test]
    fn wiener_is_reproducible_and_has_the_documented_shape() {
        let first = wiener(7, 3, 5, 0.01);
        let second = wiener(7, 3, 5, 0.01);
        assert_eq!(first, second);
        assert_eq!(first.len(), 5);
        assert!(first.iter().all(|increment| increment.len() == 3));
        // A different seed gives a different path.
        assert_ne!(first, wiener(8, 3, 5, 0.01));
        // The scale is sqrt(dt): the mean square of a scalar increment is dt.
        let increments = wiener(9, 1, 10_000, 0.01);
        let mean_square: f64 = increments
            .iter()
            .map(|increment| increment[0] * increment[0])
            .sum::<f64>()
            / 10_000.0;
        assert!(
            (mean_square - 0.01).abs() < 0.004,
            "mean square {mean_square} should be close to dt = 0.01"
        );
    }

    /// A fixed-step path consumes exactly the increments `wiener` publishes: the
    /// EM path equals a hand-driven kernel EM run on those increments.
    #[test]
    fn a_fixed_path_consumes_the_published_increments() {
        let (drift, diffusion) = gbm(0.1, 0.4);
        let options = Options::fixed(
            Method::EulerMaruyama,
            Structure::Diagonal,
            11,
            0.0,
            0.01,
            20,
        );
        let path = integrate(drift, diffusion, &[1.0], &options).expect("a path");
        let increments = wiener(11, 1, 20, 0.01);
        let expected = kernel::euler_maruyama(
            |_t: f64, y: &[f64], out: &mut [f64]| out[0] = 0.1 * y[0],
            |_t: f64, y: &[f64], out: &mut [f64]| out[0] = 0.4 * y[0],
            1,
            0.0,
            0.01,
            &[1.0],
            &increments,
        )
        .expect("the hand-driven reference");
        assert_eq!(path.y, expected, "the path is not the published wiener");
        assert_eq!(path.steps, 20);
        assert_eq!(path.t.len(), 21);
        assert_eq!(path.rejected, 0);
        assert_eq!(path.seed, 11);
        assert_eq!(path.noise, 1);
        assert!(path.nfev > 0);
    }

    #[test]
    fn a_fixed_path_is_bit_reproducible() {
        let run = || {
            let (drift, diffusion) = gbm(0.05, 0.2);
            let options =
                Options::fixed(Method::Milstein, Structure::Diagonal, 424, 0.0, 0.005, 40);
            integrate(drift, diffusion, &[2.0], &options).expect("a path")
        };
        let first = run();
        let second = run();
        assert_eq!(first, second);
    }

    #[test]
    fn every_fixed_method_runs_and_orders_are_reported() {
        for method in [
            Method::EulerMaruyama,
            Method::Milstein,
            Method::MilsteinStratonovich,
            Method::HeunStratonovich,
            Method::SrkPlaten,
            Method::MilsteinNoncommutative,
            Method::MilsteinNoncommutativeStratonovich,
        ] {
            let (drift, diffusion) = gbm(0.02, 0.3);
            let options =
                Options::fixed(method, Structure::Diagonal, 3, 0.0, 0.01, 10).with_terms(4);
            let path = integrate(drift, diffusion, &[1.0], &options)
                .unwrap_or_else(|error| panic!("{}: {error}", method.name()));
            assert_eq!(path.method, method);
            assert!(path.y.iter().flatten().all(|value| value.is_finite()));
            let order = sde_order(method);
            assert!(order.strong > 0.0 && order.strong <= 1.0);
            assert_eq!(order.weak, 1.0);
        }
        assert_eq!(sde_order(Method::EulerMaruyama).strong, 0.5);
        assert_eq!(sde_order(Method::Milstein).strong, 1.0);
        assert_eq!(Method::Milstein.calculus(), Calculus::Ito);
        assert_eq!(Method::HeunStratonovich.calculus(), Calculus::Stratonovich);
        assert_eq!(Method::from_name("srk_platen"), Some(Method::SrkPlaten));
        assert_eq!(Method::from_name("nope"), None);
    }

    /// For additive noise the Milstein/Heun corrections vanish, so the schemes
    /// coincide on the same Brownian path (they see the same increments).
    #[test]
    fn additive_noise_schemes_agree() {
        let (drift, diffusion) = ou(2.0, 0.0, 0.5);
        let em = integrate(
            drift,
            diffusion,
            &[1.0],
            &Options::fixed(Method::EulerMaruyama, Structure::Diagonal, 5, 0.0, 0.01, 50),
        )
        .expect("EM");
        let (drift, diffusion) = ou(2.0, 0.0, 0.5);
        let milstein = integrate(
            drift,
            diffusion,
            &[1.0],
            &Options::fixed(Method::Milstein, Structure::Diagonal, 5, 0.0, 0.01, 50),
        )
        .expect("Milstein");
        let em_end = em.last().expect("end")[0];
        let milstein_end = milstein.last().expect("end")[0];
        assert!(
            (em_end - milstein_end).abs() < 1e-12,
            "additive noise: EM {em_end} vs Milstein {milstein_end}"
        );
    }

    /// Diagonal and general noise with the same diagonal matrix are the same
    /// process and must produce the same path.
    #[test]
    fn diagonal_and_general_agree_on_a_diagonal_matrix() {
        let sigma = [[0.3, 0.0], [0.0, 0.2]];
        let (drift, diffusion) = (
            |_t: f64, y: &[f64]| Ok(vec![-0.1 * y[0], -0.2 * y[1]]),
            |_t: f64, _y: &[f64]| Ok(vec![sigma[0][0], sigma[1][1]]),
        );
        let diagonal = integrate(
            drift,
            diffusion,
            &[1.0, 2.0],
            &Options::fixed(
                Method::EulerMaruyama,
                Structure::Diagonal,
                21,
                0.0,
                0.01,
                20,
            ),
        )
        .expect("diagonal");
        let (drift, diffusion) = (
            |_t: f64, y: &[f64]| Ok(vec![-0.1 * y[0], -0.2 * y[1]]),
            move |_t: f64, _y: &[f64]| Ok(vec![sigma[0][0], sigma[0][1], sigma[1][0], sigma[1][1]]),
        );
        let general = integrate(
            drift,
            diffusion,
            &[1.0, 2.0],
            &Options::fixed(
                Method::EulerMaruyama,
                Structure::General {
                    noise: 2,
                    commutative: true,
                },
                21,
                0.0,
                0.01,
                20,
            ),
        )
        .expect("general");
        assert_eq!(diagonal.y, general.y);
    }

    #[test]
    fn the_noncommutative_method_runs_and_needs_terms() {
        // Two correlated, non-commutative noise channels: G depends on y.
        let drift = |_t: f64, y: &[f64]| Ok(vec![-y[0], -y[1]]);
        let diffusion =
            |_t: f64, y: &[f64]| Ok(vec![0.3 * y[0], 0.1 * y[1], 0.1 * y[0], 0.3 * y[1]]);
        let options = Options::fixed(
            Method::MilsteinNoncommutative,
            Structure::General {
                noise: 2,
                commutative: false,
            },
            7,
            0.0,
            0.01,
            10,
        )
        .with_terms(4);
        let first = integrate(drift, diffusion, &[1.0, 1.0], &options).expect("nc path");
        let second = integrate(drift, diffusion, &[1.0, 1.0], &options).expect("nc path");
        assert_eq!(first.y, second.y, "same seed, same path");
        assert!(first.y.iter().flatten().all(|value| value.is_finite()));

        // `terms == 0` selects the documented default rule, not an error.
        let missing = Options::fixed(
            Method::MilsteinNoncommutative,
            Structure::General {
                noise: 2,
                commutative: false,
            },
            7,
            0.0,
            0.01,
            10,
        );
        let defaulted = integrate(drift, diffusion, &[1.0, 1.0], &missing).expect("default terms");
        let explicit = integrate(
            drift,
            diffusion,
            &[1.0, 1.0],
            &missing.clone().with_terms(default_terms(0.01)),
        )
        .expect("explicit terms");
        assert_eq!(defaulted.y, explicit.y, "terms = 0 means default_terms(dt)");
        assert_eq!(default_terms(0.01), 16);
        assert_eq!(default_terms(1e-4), 100);
        assert_eq!(default_terms(1e-12), 4096);
        assert_eq!(default_terms(0.0), 16);
    }

    #[test]
    fn adaptive_paths_are_reproducible_and_report_their_work() {
        let run = || {
            let (drift, diffusion) = ou(1.5, 0.0, 0.4);
            let options = Options::adaptive(
                Method::Milstein,
                Structure::Diagonal,
                99,
                0.0,
                1.0,
                1e-3,
                1e-6,
                100_000,
                0,
            );
            integrate(drift, diffusion, &[2.0], &options).expect("adaptive path")
        };
        let first = run();
        let second = run();
        assert_eq!(first, second);
        assert!(first.t.windows(2).all(|window| window[1] > window[0]));
        assert!((first.t.first().copied().unwrap() - 0.0).abs() < 1e-12);
        assert!((first.t.last().copied().unwrap() - 1.0).abs() < 1e-12);
        assert!(first.steps > 0);
        assert!(first.nfev >= first.steps);
    }

    #[test]
    fn the_noncommutative_adaptive_method_runs() {
        let drift = |_t: f64, y: &[f64]| Ok(vec![-y[0], -2.0 * y[1]]);
        let diffusion =
            |_t: f64, y: &[f64]| Ok(vec![0.2 * y[0], 0.05 * y[1], 0.05 * y[0], 0.2 * y[1]]);
        let options = Options::adaptive(
            Method::MilsteinNoncommutative,
            Structure::General {
                noise: 2,
                commutative: false,
            },
            13,
            0.0,
            0.2,
            1e-2,
            1e-5,
            10_000,
            64,
        )
        .with_terms(3);
        let path = integrate(drift, diffusion, &[1.0, 1.0], &options).expect("adaptive nc");
        assert!(path.steps > 0);
        assert!(path.t.windows(2).all(|window| window[1] > window[0]));
    }

    #[test]
    fn ensemble_members_equal_the_individual_runs() {
        let mut options = Options::fixed(Method::Milstein, Structure::Diagonal, 31, 0.0, 0.01, 25);
        let (drift, diffusion) = gbm(0.03, 0.25);
        let members = ensemble(drift, diffusion, &[1.0], &options, 3).expect("ensemble");
        assert_eq!(members.len(), 3);
        for (index, member) in members.iter().enumerate() {
            options.seed = ensemble_seed(31, index);
            let (drift, diffusion) = gbm(0.03, 0.25);
            let individual =
                integrate(drift, diffusion, &[1.0], &options).expect("the individual run");
            assert_eq!(
                member.y, individual.y,
                "ensemble member {index} is not the individual run"
            );
        }
        // Order independence: re-running the ensemble gives the same members.
        let (drift, diffusion) = gbm(0.03, 0.25);
        let again = ensemble(
            drift,
            diffusion,
            &[1.0],
            &Options::fixed(Method::Milstein, Structure::Diagonal, 31, 0.0, 0.01, 25),
            3,
        )
        .expect("ensemble");
        assert_eq!(members, again);
    }

    #[test]
    fn errors_are_typed() {
        // A drift of the wrong length is E0802.
        let options = Options::fixed(Method::EulerMaruyama, Structure::Diagonal, 1, 0.0, 0.01, 2);
        let error = integrate(
            |_t: f64, _y: &[f64]| Ok(vec![0.0, 0.0, 0.0]),
            |_t: f64, _y: &[f64]| Ok(vec![0.0, 0.0]),
            &[1.0, 1.0],
            &options,
        )
        .expect_err("wrong drift length");
        assert_eq!(error.code(), "E0802");

        // A diffusion of the wrong length is E0802.
        let error = integrate(
            |_t: f64, _y: &[f64]| Ok(vec![0.0]),
            |_t: f64, _y: &[f64]| Ok(vec![0.0, 0.0]),
            &[1.0],
            &options,
        )
        .expect_err("wrong diffusion length");
        assert_eq!(error.code(), "E0802");

        // A zero step is a parameter error.
        let error = integrate(
            |_t: f64, _y: &[f64]| Ok(vec![0.0]),
            |_t: f64, _y: &[f64]| Ok(vec![0.0]),
            &[1.0],
            &Options::fixed(Method::EulerMaruyama, Structure::Diagonal, 1, 0.0, 0.0, 2),
        )
        .expect_err("zero step");
        assert_eq!(error.code(), "E0802");

        // An identical diffusion that becomes non-finite: E0806.
        let error = integrate(
            |_t: f64, _y: &[f64]| Ok(vec![f64::INFINITY]),
            |_t: f64, _y: &[f64]| Ok(vec![0.0]),
            &[1.0],
            &options,
        )
        .expect_err("non-finite");
        assert_eq!(error.code(), "E0806");

        // An adaptive step budget of one step cannot reach t1: E0807.
        let error = integrate(
            |_t: f64, y: &[f64]| Ok(vec![-100.0 * y[0]]),
            |_t: f64, _y: &[f64]| Ok(vec![0.1]),
            &[1.0],
            &Options::adaptive(
                Method::Milstein,
                Structure::Diagonal,
                2,
                0.0,
                1.0,
                1e-12,
                0.0,
                1,
                0,
            ),
        )
        .expect_err("step limit");
        assert_eq!(error.code(), "E0807");

        // A Stratonovich method under an adaptive controller now runs (Ito conversion).
        let path = integrate(
            |_t: f64, _y: &[f64]| Ok(vec![0.0]),
            |_t: f64, _y: &[f64]| Ok(vec![0.1]),
            &[1.0],
            &Options::adaptive(
                Method::HeunStratonovich,
                Structure::Diagonal,
                2,
                0.0,
                1.0,
                1e-3,
                1e-6,
                10_000,
                0,
            ),
        )
        .expect("adaptive Stratonovich");
        assert!(path.steps > 0);

        // A commutative-only method on declared non-commutative noise dispatches
        // to the Levy-area kernel instead of failing.
        let path = integrate(
            |_t: f64, _y: &[f64]| Ok(vec![0.0, 0.0]),
            |_t: f64, y: &[f64]| Ok(vec![0.1 * y[1], 0.1, 0.2, 0.1 * y[0]]),
            &[1.0, 1.0],
            &Options::fixed(
                Method::Milstein,
                Structure::General {
                    noise: 2,
                    commutative: false,
                },
                3,
                0.0,
                0.01,
                2,
            ),
        )
        .expect("non-commutative Milstein");
        assert_eq!(path.steps, 2);

        // A non-uniform explicit grid is rejected: the kernel's fixed-step
        // solvers assume one step size.
        let error = integrate(
            |_t: f64, _y: &[f64]| Ok(vec![0.0]),
            |_t: f64, _y: &[f64]| Ok(vec![0.1]),
            &[1.0],
            &Options::grid(
                Method::EulerMaruyama,
                Structure::Diagonal,
                1,
                vec![0.0, 0.01, 0.03],
            ),
        )
        .expect_err("non-uniform grid");
        assert_eq!(error.code(), "E0802");
    }

    #[test]
    fn a_uniform_explicit_grid_reaches_the_same_end_as_fixed_steps() {
        let times = vec![0.0, 0.01, 0.02, 0.03];
        let (drift, diffusion) = gbm(0.1, 0.2);
        let grid = integrate(
            drift,
            diffusion,
            &[1.0],
            &Options::grid(Method::EulerMaruyama, Structure::Diagonal, 17, times),
        )
        .expect("grid path");
        let (drift, diffusion) = gbm(0.1, 0.2);
        let fixed = integrate(
            drift,
            diffusion,
            &[1.0],
            &Options::fixed(Method::EulerMaruyama, Structure::Diagonal, 17, 0.0, 0.01, 3),
        )
        .expect("fixed path");
        assert_eq!(grid.y, fixed.y);
    }

    #[test]
    fn fixed_span_reaches_the_end_exactly() {
        let span =
            Options::fixed_span(Method::EulerMaruyama, Structure::Diagonal, 5, 0.0, 1.0, 0.3);
        let (drift, diffusion) = gbm(0.0, 0.1);
        let path = integrate(drift, diffusion, &[1.0], &span).expect("a span path");
        assert_eq!(path.steps, 3, "the span is divided into whole steps");
        assert!((path.t.last().copied().unwrap() - 1.0).abs() < 1e-12);
    }

    /// The calculus is part of the scheme name: on the same Brownian path the
    /// Itô and Stratonovich Milstein solutions differ by the Itô correction,
    /// which for `dX = sigma X dW` makes the Stratonovich path larger by
    /// `exp(sigma^2 T / 2)` in the limit (here with the O(h) discretisation
    /// error much smaller than the tolerance).
    #[test]
    fn ito_and_stratonovich_differ_by_the_expected_correction() {
        let sigma = 0.2;
        let dt = 0.001;
        let steps = 100;
        let horizon = dt * steps as f64;
        let (drift, diffusion) = gbm(0.0, sigma);
        let ito = integrate(
            drift,
            diffusion,
            &[1.0],
            &Options::fixed(Method::Milstein, Structure::Diagonal, 77, 0.0, dt, steps),
        )
        .expect("Ito path");
        let (drift, diffusion) = gbm(0.0, sigma);
        let stratonovich = integrate(
            drift,
            diffusion,
            &[1.0],
            &Options::fixed(
                Method::MilsteinStratonovich,
                Structure::Diagonal,
                77,
                0.0,
                dt,
                steps,
            ),
        )
        .expect("Stratonovich path");
        let ratio = stratonovich.last().unwrap()[0] / ito.last().unwrap()[0];
        let expected = (0.5 * sigma * sigma * horizon).exp();
        assert!(
            (ratio - expected).abs() < 1e-3,
            "Ito vs Stratonovich ratio {ratio} should be exp(sigma^2 T/2) = {expected}"
        );
    }

    /// The kernel's adaptive controller integrates the Milstein solution for
    /// both Itô requests; the code keeps the requested name but the path is the
    /// same. This is the documented contract, asserted so it cannot drift into a
    /// silent scheme substitution either way.
    #[test]
    fn adaptive_em_and_milstein_are_the_same_controlled_path() {
        let options = |method| {
            Options::adaptive(
                method,
                Structure::Diagonal,
                123,
                0.0,
                0.5,
                1e-3,
                1e-6,
                100_000,
                0,
            )
        };
        let (drift, diffusion) = ou(2.0, 0.0, 0.3);
        let em = integrate(drift, diffusion, &[1.0], &options(Method::EulerMaruyama))
            .expect("adaptive EM");
        let (drift, diffusion) = ou(2.0, 0.0, 0.3);
        let milstein = integrate(drift, diffusion, &[1.0], &options(Method::Milstein))
            .expect("adaptive Milstein");
        assert_eq!(em.y, milstein.y);
    }

    #[test]
    fn the_noncommutative_methods_also_run_on_commutative_noise() {
        // The Levy-area methods are valid for any structure (the areas just
        // drop out of the commutative correction).
        for method in [
            Method::MilsteinNoncommutative,
            Method::MilsteinNoncommutativeStratonovich,
        ] {
            let path = integrate(
                |_t: f64, _y: &[f64]| Ok(vec![0.0]),
                |_t: f64, y: &[f64]| Ok(vec![0.1 * y[0]]),
                &[1.0],
                &Options::fixed(method, Structure::Diagonal, 1, 0.0, 0.01, 4).with_terms(4),
            )
            .expect("Levy-area method on diagonal noise");
            assert_eq!(path.steps, 4);
            assert_eq!(path.method, method);
        }
    }

    const NC: Structure = Structure::General {
        noise: 2,
        commutative: false,
    };

    /// Two non-commuting noise fields on a 2-state system.
    fn nc_system() -> (Coeff, Coeff) {
        (
            Box::new(|_t: f64, y: &[f64]| Ok(vec![-0.5 * y[0], -0.3 * y[1]])),
            Box::new(|_t: f64, y: &[f64]| Ok(vec![0.3 * y[1], 0.1, 0.2, 0.3 * y[0]])),
        )
    }

    /// The method name selects the family, the structure the kernel: `Milstein` /
    /// `MilsteinStratonovich` on non-commutative noise are the explicit
    /// non-commutative methods, bit for bit.
    #[test]
    fn method_names_dispatch_on_the_structure() {
        let run = |method| {
            let (drift, diffusion) = nc_system();
            let options = Options::fixed(method, NC, 17, 0.0, 0.01, 12).with_terms(5);
            integrate(drift, diffusion, &[1.0, 0.5], &options).expect("path")
        };
        assert_eq!(
            run(Method::Milstein).y,
            run(Method::MilsteinNoncommutative).y
        );
        assert_eq!(
            run(Method::MilsteinStratonovich).y,
            run(Method::MilsteinNoncommutativeStratonovich).y
        );
        assert_ne!(
            run(Method::Milstein).y,
            run(Method::MilsteinStratonovich).y,
            "the calculus is part of the method"
        );
        for method in [
            Method::SrkPlaten,
            Method::HeunStratonovich,
            Method::EulerMaruyama,
        ] {
            let path = run(method);
            assert!(path.y.iter().flatten().all(|value| value.is_finite()));
            assert_eq!(path.method, method);
        }
        // SRK on non-commutative noise is not the commutative SRK.
        assert_ne!(run(Method::SrkPlaten).y, run(Method::EulerMaruyama).y);
    }

    #[test]
    fn orders_depend_on_the_structure() {
        assert_eq!(sde_order_for(Method::HeunStratonovich, NC).strong, 0.5);
        assert_eq!(
            sde_order_for(Method::HeunStratonovich, Structure::Diagonal).strong,
            1.0
        );
        assert_eq!(sde_order_for(Method::Milstein, NC).strong, 1.0);
        assert_eq!(sde_order_for(Method::SrkPlaten, NC).strong, 1.0);
        assert_eq!(sde_order_for(Method::EulerMaruyama, NC).strong, 0.5);
        assert_eq!(
            sde_order(Method::MilsteinNoncommutativeStratonovich).strong,
            1.0
        );
        assert_eq!(
            Method::from_name("milstein_noncommutative_stratonovich"),
            Some(Method::MilsteinNoncommutativeStratonovich)
        );
        assert_eq!(
            Method::MilsteinNoncommutativeStratonovich.calculus(),
            Calculus::Stratonovich
        );
    }

    /// Adaptive Stratonovich on `dX = sigma X o dW`: the mean over paths is
    /// `exp(sigma^2 T / 2)` (the Ito mean would be 1), and adaptive `SrkPlaten`
    /// runs the Ito controller.
    #[test]
    fn adaptive_stratonovich_has_the_stratonovich_mean() {
        let sigma = 0.5;
        let paths = 300_u64;
        let (mut sum, mut square) = (0.0, 0.0);
        for seed in 0..paths {
            let (drift, diffusion) = gbm(0.0, sigma);
            let options = Options::adaptive(
                Method::MilsteinStratonovich,
                Structure::Diagonal,
                1000 + seed,
                0.0,
                1.0,
                1e-3,
                1e-4,
                100_000,
                0,
            );
            let path = integrate(drift, diffusion, &[1.0], &options).expect("adaptive Strat");
            let x = path.last().expect("end")[0];
            sum += x;
            square += x * x;
        }
        let n = paths as f64;
        let mean = sum / n;
        let se = ((square / n - mean * mean) / n).sqrt();
        let strat = (0.5 * sigma * sigma).exp();
        assert!(
            (mean - strat).abs() < 4.5 * se,
            "mean {mean} vs {strat} (se {se})"
        );

        let (drift, diffusion) = gbm(0.0, sigma);
        let srk = integrate(
            drift,
            diffusion,
            &[1.0],
            &Options::adaptive(
                Method::SrkPlaten,
                Structure::Diagonal,
                5,
                0.0,
                1.0,
                1e-3,
                1e-4,
                100_000,
                0,
            ),
        )
        .expect("adaptive SRK");
        assert!(srk.steps > 0);
    }

    #[test]
    fn adaptive_noncommutative_runs_for_every_method_with_default_cells() {
        for method in [
            Method::EulerMaruyama,
            Method::Milstein,
            Method::MilsteinStratonovich,
            Method::HeunStratonovich,
            Method::SrkPlaten,
            Method::MilsteinNoncommutativeStratonovich,
        ] {
            let (drift, diffusion) = nc_system();
            let options = Options::adaptive(method, NC, 21, 0.0, 0.2, 1e-2, 1e-5, 100_000, 0);
            let path = integrate(drift, diffusion, &[1.0, 0.5], &options)
                .unwrap_or_else(|error| panic!("{}: {error}", method.name()));
            assert!(path.steps > 0, "{}", method.name());
            assert!((path.t.last().copied().unwrap() - 0.2).abs() < 1e-12);
        }
    }

    // ----- continuous output and events on paths --------------------------

    fn decay_path() -> Path {
        // dX = -X dt with no noise: the Euler path is y_k = (1 - dt)^k.
        let (drift, diffusion) = ou(1.0, 0.0, 0.0);
        let options = Options::fixed(
            Method::EulerMaruyama,
            Structure::Diagonal,
            1,
            0.0,
            0.01,
            200,
        );
        integrate(drift, diffusion, &[1.0], &options).unwrap()
    }

    #[test]
    fn a_path_interpolates_linearly_between_nodes_and_resamples() {
        let path = decay_path();
        for (t, y) in path.t.iter().zip(&path.y) {
            assert_eq!(path.eval(*t).as_ref(), Some(y), "nodes are exact");
        }
        let mid = path.eval(0.005).unwrap()[0];
        assert!((mid - 0.5 * (path.y[0][0] + path.y[1][0])).abs() < 1e-15);
        let last = *path.t.last().unwrap();
        assert!(path.eval(-1e-9).is_none() && path.eval(last + 1e-9).is_none());
        let saved = path.resample(&[0.0, 0.5, last]).unwrap();
        assert_eq!(saved.len(), 3);
        // Chord error against the Euler solution is O(dt^2).
        assert!((saved[1][0] - 0.99f64.powf(50.0)).abs() < 1e-4);
        assert!(path.resample(&[last + 1.0]).is_err());
    }

    #[test]
    fn path_events_locate_crossings_with_direction_and_terminal() {
        let path = decay_path();
        let mut g = |_i: usize, _t: f64, y: &[f64]| y[0] - 0.5;
        let crossing = 0.01 * (0.5f64.ln() / 0.99f64.ln());
        let falling = EventSpec {
            terminal: false,
            direction: -1,
        };
        let (whole, hits) = events(&path, &[falling], &mut g);
        assert_eq!(hits.len(), 1);
        assert!(
            (hits[0].t - crossing).abs() < 1e-4,
            "{} vs {crossing}",
            hits[0].t
        );
        assert!((hits[0].y[0] - 0.5).abs() < 1e-9);
        assert_eq!(whole, path, "no terminal event leaves the path alone");
        // The wrong direction sees nothing.
        let rising = EventSpec {
            terminal: false,
            direction: 1,
        };
        assert!(events(&path, &[rising], &mut g).1.is_empty());
        // A terminal event cuts the path and the hit is its last node.
        let stop = EventSpec {
            terminal: true,
            direction: 0,
        };
        let (cut, hits) = events(&path, &[stop], &mut g);
        assert_eq!(hits.len(), 1);
        assert_eq!(*cut.t.last().unwrap(), hits[0].t);
        assert!((cut.y.last().unwrap()[0] - 0.5).abs() < 1e-9);
        assert!(cut.t.len() < path.t.len());
        assert_eq!(cut.steps, cut.t.len() - 1);
        // Two events: a non-terminal at 0.5 and a terminal at 0.25. Both fire,
        // in time order, and the path stops at the second.
        let mut two = |i: usize, _t: f64, y: &[f64]| y[0] - if i == 0 { 0.5 } else { 0.25 };
        let (cut, hits) = events(
            &path,
            &[
                falling,
                EventSpec {
                    terminal: true,
                    direction: -1,
                },
            ],
            &mut two,
        );
        assert_eq!(
            hits.iter().map(|hit| hit.index).collect::<Vec<_>>(),
            vec![0, 1]
        );
        assert!(hits[0].t < hits[1].t);
        assert!((cut.t.last().unwrap() - 4.0f64.ln()).abs() < 0.02);
    }

    #[test]
    fn a_noisy_path_event_is_reproducible_for_a_seed() {
        let run = |seed: u64| {
            let (drift, diffusion) = gbm(0.0, 0.5);
            let options = Options::fixed(
                Method::Milstein,
                Structure::Diagonal,
                seed,
                0.0,
                0.001,
                2000,
            );
            let path = integrate(drift, diffusion, &[1.0], &options).unwrap();
            let mut g = |_i: usize, _t: f64, y: &[f64]| y[0] - 1.2;
            events(
                &path,
                &[EventSpec {
                    terminal: true,
                    direction: 1,
                }],
                &mut g,
            )
        };
        let (a, hits_a) = run(5);
        let (b, hits_b) = run(5);
        assert_eq!(a, b);
        assert_eq!(hits_a, hits_b);
        assert_ne!(run(6).0, a);
    }

    #[test]
    fn drift_implicit_schemes_remain_stable_on_stiff_sdes_where_explicit_explodes() {
        // Stiff Ornstein-Uhlenbeck: dX = -lambda X dt + sigma dW with lambda = 500, dt = 0.02.
        // Note lambda * dt = 10 > 2, so explicit Euler-Maruyama amplifies by (1 - 10)^k = (-9)^k
        // and blows up immediately, while DriftImplicitEuler, ThetaMaruyama,
        // DriftImplicitMilstein, and SplitStepBackwardEuler remain bounded and stable!
        let lambda = 500.0;
        let sigma = 0.5;
        let dt = 0.02;
        let steps = 50;

        for scheme in [
            DriftImplicitScheme::BackwardEuler,
            DriftImplicitScheme::Trapezoidal,
            DriftImplicitScheme::Theta(0.75),
            DriftImplicitScheme::DriftImplicitMilstein,
            DriftImplicitScheme::SplitStepBackwardEuler,
        ] {
            let opts = Options::fixed(
                Method::DriftImplicitEuler,
                Structure::Diagonal,
                19,
                0.0,
                dt,
                steps,
            );
            let path = integrate_drift_implicit(
                |_t, y| Ok(vec![-lambda * y[0]]),
                |_t, _y| Ok(vec![sigma]),
                &[10.0],
                scheme,
                &opts,
            )
            .unwrap();
            let end_val = path.last().unwrap()[0];
            assert!(
                end_val.abs() < 1.0,
                "scheme {scheme:?} failed to damp stiff mode: {end_val}"
            );
        }

        // Also test Method::DriftImplicitEuler, Method::ThetaMaruyama, and
        // Method::DriftImplicitMilstein directly via `integrate`:
        for method in [
            Method::DriftImplicitEuler,
            Method::ThetaMaruyama,
            Method::DriftImplicitMilstein,
        ] {
            let opts = Options::fixed(method, Structure::Diagonal, 23, 0.0, dt, steps);
            let path = integrate(
                |_t, y| Ok(vec![-lambda * y[0]]),
                |_t, y| Ok(vec![0.1 * y[0]]),
                &[5.0],
                &opts,
            )
            .unwrap();
            let end_val = path.last().unwrap()[0];
            assert!(end_val.is_finite() && end_val.abs() < 1.0);
        }
    }

    #[test]
    fn jump_diffusion_and_levy_sdes_simulate_merton_kou_and_alpha_stable_paths() {
        // 1. Merton jump-diffusion model: dS = mu S dt + sigma S dW + S (exp(Z) - 1) dN
        let merton_spec = JumpSpec {
            intensity: 5.0,
            distribution: JumpDistribution::Normal {
                mean: vec![-0.05],
                std: vec![0.15],
            },
            compensated: true,
        };
        let opts = Options::fixed(Method::Milstein, Structure::Diagonal, 101, 0.0, 0.01, 200);
        let jpath = integrate_jump_diffusion_compensated(
            |_t, y| Ok(vec![0.05 * y[0]]),
            |_t, y| Ok(vec![0.2 * y[0]]),
            |_t, y, z| Ok(vec![y[0] * (z[0].exp() - 1.0)]),
            |_t, y| {
                Ok(vec![
                    y[0] * ((-0.05_f64 + 0.5 * 0.15_f64.powi(2)).exp() - 1.0),
                ])
            },
            &[100.0],
            &merton_spec,
            &opts,
        )
        .unwrap();
        assert_eq!(jpath.path.steps, 200);
        assert!(jpath.total_jumps > 0, "expected Poisson jumps to occur");
        assert!(jpath.path.y.iter().all(|s| s[0] > 0.0 && s[0].is_finite()));

        // 2. Kou double-exponential jump-diffusion model
        let kou_spec = JumpSpec {
            intensity: 4.0,
            distribution: JumpDistribution::DoubleExponential {
                p_up: 0.4,
                eta_up: 10.0,
                eta_down: 5.0,
                dim: 1,
            },
            compensated: true,
        };
        let kou_path = integrate_jump_diffusion_compensated(
            |_t, y| Ok(vec![0.03 * y[0]]),
            |_t, y| Ok(vec![0.15 * y[0]]),
            |_t, y, z| Ok(vec![y[0] * z[0]]),
            |_t, y| Ok(vec![y[0] * (0.4 / 10.0 - 0.6 / 5.0)]),
            &[100.0],
            &kou_spec,
            &opts,
        )
        .unwrap();
        assert!(kou_path.total_jumps > 0);
        assert!(kou_path.path.last().unwrap()[0].is_finite());

        // 3. Alpha-stable Lévy-driven Ornstein-Uhlenbeck process (alpha = 1.5, beta = 0.0)
        assert!(
            integrate_levy(
                |_t, y| Ok(vec![-2.0 * y[0]]),
                |_t, _y| Ok(vec![0.3]),
                &[1.0],
                1.5,
                0.0,
                &opts,
            )
            .is_err()
        );
        let levy_options = Options::fixed(
            Method::EulerMaruyama,
            Structure::Diagonal,
            101,
            0.0,
            0.01,
            200,
        );
        let levy_path = integrate_levy(
            |_t, y| Ok(vec![-2.0 * y[0]]),
            |_t, _y| Ok(vec![0.3]),
            &[1.0],
            1.5,
            0.0,
            &levy_options,
        )
        .unwrap();
        assert_eq!(levy_path.steps, 200);
        assert!(levy_path.y.iter().flatten().all(|v| v.is_finite()));
    }

    #[test]
    fn mlmc_achieves_variance_reduction_and_accurate_expectation() {
        // Geometric Brownian Motion: dX = r X dt + sigma X dW, X(0) = 1.0
        // Analytical E[X(1)] = exp(r * 1) = exp(0.05) = 1.0512710963760241.
        let r = 0.05;
        let sigma = 0.2;
        let mlmc_opts = MlmcOptions {
            method: Method::Milstein,
            structure: Structure::Diagonal,
            seed: 777,
            t0: 0.0,
            t1: 1.0,
            base_steps: 4,
            min_level: 3,
            max_level: 5,
            warmup_samples: 60,
            max_samples_per_level: 4000,
            target_rmse: 5e-3,
        };
        let res = mlmc(
            |_t, y| Ok(vec![r * y[0]]),
            |_t, y| Ok(vec![sigma * y[0]]),
            |y_end| Ok(y_end[0]),
            &[1.0],
            &mlmc_opts,
        )
        .unwrap();

        let exact = r.exp();
        assert!(
            (res.estimate - exact).abs() < 2.5e-2,
            "MLMC estimate {} vs exact {}",
            res.estimate,
            exact
        );
        // Verify telescopic variance decay: coupled level variances V_l for l >= 1
        // must be orders of magnitude smaller than the base level variance V_0!
        let v0 = res.levels[0].variance;
        let v_last = res.levels.last().unwrap().variance;
        assert!(
            v_last < 0.01 * v0,
            "expected strong coupled variance reduction: V_0 = {v0}, V_last = {v_last}"
        );
        // Sample allocation N_l should decrease with level l
        assert!(res.levels[0].samples >= res.levels.last().unwrap().samples);
    }
    #[test]
    fn jump_diffusion_respects_implicit_scheme_and_rejects_invalid_compensator() {
        let options = Options::fixed(
            Method::DriftImplicitEuler,
            Structure::Diagonal,
            7,
            0.0,
            0.1,
            4,
        );
        let spec = JumpSpec {
            intensity: 0.0,
            distribution: JumpDistribution::Constant(vec![0.0]),
            compensated: false,
        };
        let result = integrate_jump_diffusion(
            |_, y| Ok(vec![-1000.0 * y[0]]),
            |_, _| Ok(vec![0.0]),
            |_, _, z| Ok(z.to_vec()),
            &[1.0],
            &spec,
            &options,
        )
        .unwrap();
        assert!(result.path.y.last().unwrap()[0].abs() < 1e-7);
        let invalid = JumpSpec {
            intensity: 1.0,
            distribution: JumpDistribution::Normal {
                mean: vec![0.0],
                std: vec![1.0],
            },
            compensated: true,
        };
        assert!(
            integrate_jump_diffusion(
                |_, _| Ok(vec![0.0]),
                |_, _| Ok(vec![0.0]),
                |_, _, z| Ok(vec![z[0].exp() - 1.0]),
                &[1.0],
                &invalid,
                &options
            )
            .is_err()
        );
    }

    #[test]
    fn poisson_large_mean_uses_exact_uniform_rejection() {
        let value = rssn::kernels::sde_ext::sample_poisson_with(
            &mut || 0.5,
            &mut || panic!("normal approximation must not be used"),
            100.0,
        );
        assert_eq!(value, 100);
    }

    #[test]
    fn extended_sde_solvers_do_not_silently_replace_adaptive_schedules() {
        let options = Options::adaptive(
            Method::DriftImplicitEuler,
            Structure::Diagonal,
            7,
            0.0,
            1.0,
            1e-5,
            1e-8,
            1000,
            0,
        );
        assert!(
            integrate(
                |_, y| Ok(y.to_vec()),
                |_, _| Ok(vec![0.0]),
                &[1.0],
                &options
            )
            .is_err()
        );
        assert!(
            integrate_drift_implicit(
                |_, y| Ok(y.to_vec()),
                |_, _| Ok(vec![0.0]),
                &[1.0],
                DriftImplicitScheme::BackwardEuler,
                &options
            )
            .is_err()
        );
    }
}
