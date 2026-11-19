# Adaptive ODE solvers

An adaptive solver chooses its own time grid. You supply a rate law, an initial state, the requested time span, and an error scale. The solver can try a shorter step when its error estimate is too large. In this chapter you will compare two integrations against a known solution and check what the increased accuracy costs.

## Turn the model into a rate

Use the relaxation model from [Numerical models that evolve in time](08-time-evolution.md). A position starts at 5 m and tends toward 1 m with a time constant of 2 s. Its rate is the negative displacement from equilibrium divided by the time constant. The callback therefore takes a time and a length, and must return a velocity. It accepts the time even though this particular law has no explicit time dependence.

The exact position at elapsed time `t` is `1 m + 4 m * exp(-t / (2 s))`. The exponent is dimensionless. Keep this formula separate from the callback so you can use it as an independent check at the endpoint and between accepted steps.

## Integrate twice and inspect the result

The example uses `AdaptiveRk4` and `OdePhysicalOpts`. The absolute tolerance `atol` is a length, like the state, and `rtol` is a dimensionless relative tolerance. Setting `rtol` to zero isolates the effect of absolute tolerance. Both integrations request the same initial step of 2 s and permit steps up to that size. The lower step bound and work limits keep a failed solve from trying indefinitely.

```aevia,run
open Units.SI
import Ode { AdaptiveRk4, OdePhysicalOpts, ode_solve }

fn rate(_t: Float[Time], position: Float[Length]): Float[Length/Time] =
    -(position - 1.0 m) / (2.0 s)

fn reference(t: Float[Time]): Float[Length] =
    1.0 m + 4.0 m * exp(-t / (2.0 s))

fn options(tolerance: Float[Length]): OdePhysicalOpts<Float[Length]> = {
    rtol = 0.0, atol = tolerance, method = AdaptiveRk4,
    steps = None, events = None,
    h0 = 2.0 s, h_min = 0.000001 s, h_max = 2.0 s,
    max_steps = 1000, max_evaluations = 10000,
}

fn main(): IO Unit = do
    let finish = 4.0 s
    let probe = 3.25 s
    let loose = ode_solve(rate, 5.0 m, (0.0 s, finish), options(0.001 m), [])
    let tight = ode_solve(rate, 5.0 m, (0.0 s, finish), options(0.00000001 m), [])
    let loose_error = abs(loose.at(finish) - reference(finish))
    let tight_error = abs(tight.at(finish) - reference(finish))
    let dense_error = abs(tight.at(probe) - reference(probe))
    let last_time = tight.t[length(tight.t) - 1]
    println({ reference = reference(finish), endpoint = tight.at(finish), time = last_time })
    println({ loose_error = loose_error, tight_error = tight_error, dense_error = dense_error })
    println({ method = tight.method, loose_steps = length(loose.t) - 1, tight_steps = length(tight.t) - 1 })
    println({ loose_nfev = loose.nfev, tight_nfev = tight.nfev, rejected = tight.rejected })
    println(last_time == finish && tight_error < loose_error && tight_error < 0.000001 m && dense_error < 0.000001 m)
```

The reference endpoint is about **1.54134113 m**. In a verified run, the loose endpoint error was about **6.8 × 10⁻⁵ m** and the tight error was about **1.1 × 10⁻⁹ m**. The tight solution reached **4 s**, and its error at **3.25 s** was below **1 µm**. The last line should print `true`. The check deliberately uses error bounds, so it can tolerate small differences in floating-point calculations.

The record's `t` and `y` fields contain the accepted times and their states, including the initial condition. Subtract one from the length of `t` to count accepted integration steps. The `at` function evaluates the numerical trajectory within the solved span, including at times between stored steps. The comparison at 3.25 s checks interpolation alongside the check of the final state.

## Read the work counters

Here the tighter run accepted 114 steps versus 8 for the looser one. Its `nfev` count, the number of times the solver called the rate function, rose from 71 to 827. The tight run rejected two trial steps. A rejection means the solver tested a step and discarded it before advancing the accepted trajectory. Step and evaluation counts can change with implementation details, so use them as diagnostics.

The requested tolerance controls a local error estimate. Establish accuracy for your quantity of interest by comparing against a reference, a conservation law, or another run with stricter settings. Smaller tolerances can increase work or exhaust the step or evaluation limits. If that happens, check the rate, units and initial condition first. A model with very different time scales may need a stiff solver instead of a sequence of smaller explicit steps. See the available methods and options in [Ode](/stdlib/Ode?lang=en).

## Exercises

1. Add a third run with `atol = 0.00001 m`. Record its endpoint error and `nfev`. Compare all three runs by how much each additional evaluation improves the result.
2. Let `options` take a step cap and use it for both `h0` and `h_max`. Run the loose case with a cap of `0.25 s` and compare its endpoint error and work count with the original loose case.
3. Stop when the position falls through 2 m. Supply an event whose function is `(position - 2.0 m) / (1.0 m)`, with `direction = -1` and `terminal = true`. Compare the reported event time with `2 s * ln(4)`, about **2.773 s**. Check the last accepted time against the event and keep interpolation queries inside the shorter span. The repository's [adaptive geometric ODE example](https://github.com/Apich-Organization/aevia/blob/v0.1/examples/ode_adaptive_geometric.aev) shows an event and a state with independently dimensioned fields.
