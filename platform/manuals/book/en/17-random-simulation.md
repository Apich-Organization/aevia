# Seeded stochastic simulation

A stochastic model produces a distribution of trajectories. Replaying one trajectory checks whether the inputs identify the same calculation. Simulating an ensemble lets you check quantities such as a mean or a variance. This chapter uses both checks on a process whose endpoint distribution you can derive before running the program.

## Predict the endpoint distribution

Start a particle at 0 m. It has a constant drift of 0.5 m/s and Brownian noise with amplitude 0.4 m/√s. In the equation `dX = μ dt + σ dW`, a Brownian increment over a time interval has mean zero and variance equal to that interval. The increment has units of square-root time. Multiplying by the diffusion coefficient gives a change in position, in metres. The drift contributes a change in metres as well.

After 1 s, the expected position is **0.5 m**, and the standard deviation is **0.4 m**. Individual endpoints can differ considerably from 0.5 m. The expected standard error of the mean across 64 independent paths is `0.4 m / sqrt(64)`, or **0.05 m**.

## Replay one path and check many

The fixed-step Euler–Maruyama method adds the deterministic drift and a Brownian increment at each step. This example has constant coefficients, so the method reproduces the endpoint distribution on the chosen grid apart from floating-point rounding. We use a one-element length vector and `Diagonal` noise, which assigns one independent noise channel to each state component. The eight steps of 0.125 s reach the requested endpoint of 1 s.

```aevia,run
open Units.SI
open Sde

fn drift(_t: Float[Time], _y: Tensor<[1], Float[Length]>): Tensor<[1], Float[Length/Time]> =
    [0.5 m / (1.0 s)]

fn diffusion(_t: Float[Time], _y: Tensor<[1], Float[Length]>): Tensor<[1], Float[Length/Time^(1/2)]> =
    [0.4 m / sqrt(1.0 s)]

fn options(seed: Int) = Fixed({
    structure = Diagonal, method = EulerMaruyama, seed = seed,
    t0 = 0.0 s, dt = 0.125 s, steps = 8, terms = 0,
})

fn main(): IO Unit =
    match sdeint_diag(drift, diffusion, [0.0 m], options(2718)) with
    | Err(problem) -> println(problem)
    | Ok(first) ->
        match sdeint_diag(drift, diffusion, [0.0 m], options(2718)) with
        | Err(problem) -> println(problem)
        | Ok(repeat) -> do
            let members = sde_ensemble_diag(drift, diffusion, [0.0 m], options(2718), 64)
            match find(is_err, members) with
            | Some(problem) -> println(problem)
            | None -> do
                let endpoints = map(fn member => match member with
                    | Ok(path) -> path.y[length(path.y) - 1][0]
                    | Err(_) -> nan * (1.0 m), members)
                let center = mean(endpoints)
                let spread = std(endpoints)
                let time = first.t[length(first.t) - 1]
                let replay = first.t == repeat.t && first.y == repeat.y
                println({ endpoint = first.y[length(first.y) - 1][0], time = time, replay = replay })
                println({ paths = length(members), mean = center, std = spread })
                println(time == 1.0 s && replay && abs(center - 0.5 m) < 0.15 m && abs(spread - 0.4 m) < 0.15 m)
```

The first output should show `time = 1.0 s` and `replay = true`. With the supplied seed, the example produced a single endpoint of about **1.177 m**. That endpoint is one draw. The run also produced an ensemble mean of about **0.492 m** and a sample standard deviation of about **0.380 m**. The final line should be `true`. The example uses a 0.15 m allowance for each ensemble statistic, which is three expected standard errors for the mean.

Each ensemble member is a `Result`. The program checks for a failed member before computing summary statistics. The matching branch in the mapping function is exhaustive, and its `NaN` sentinel is unreachable after that check. Treat a failed member as a diagnostic. Removing failed paths can change which parts of the distribution you observe.

## Choose the right reproducibility check

A seed and the same numerical settings make replay useful for debugging a specific path. The ensemble routine derives a different seed for each member. If one member behaves oddly, use `sde_ensemble_seed` to recover that member's seed and run it alone with the same settings. Repeating the base seed in a loop would replay a single path and give a misleading apparent sample size.

The ensemble mean and standard deviation can move when the seed changes. Compare them to theoretical values with an allowance for sampling variation. Increasing the number of independent paths reduces the standard error of the mean in proportion to the inverse square root of the count. Changing the time grid can change the realization drawn from a given seed. A pathwise time-step study requires Brownian increments that represent the same underlying noise on both grids. With nonconstant coefficients, check time discretization separately from ensemble size. The [Sde reference](/stdlib/Sde?lang=en) describes additional schemes and noise structures.

## Exercises

1. Set the diffusion coefficient to zero. Check that the endpoint equals 0.5 m up to rounding and that the ensemble spread is zero.
2. Increase the coefficient to `0.8 m / sqrt(1.0 s)`. The expected mean stays at 0.5 m while the expected spread doubles. Update the validation thresholds to reflect the new spread and sampling error before running.
3. Change the seed and compare the first path and the ensemble summary. Restore the original seed and the same grid to recover the original result. For an extension with rejected trial steps, see the repository's [adaptive implicit SDE example](https://github.com/Apich-Organization/aevia/blob/v0.1/examples/sde_adaptive_implicit.aev), where the solver reuses one Brownian realization during retries.
