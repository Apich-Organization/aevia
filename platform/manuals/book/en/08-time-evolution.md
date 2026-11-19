# Numerical models that evolve in time

An evolution equation turns a law into a trajectory. Its initial condition, output times, and units are parts of the model. Study a model with a closed-form solution before using a general solver.

## Derive a reference value

A first-order relaxation satisfies (dx/dt = -(x - x_mathrm{eq}) / 	au). The elapsed time and the time constant must have the same dimension. The exact mathematical solution uses an exponential of their ratio.

```aevia,run
fn relax(initial: Float[Length], equilibrium: Float[Length], age: Float[Time], tau: Float[Time]): Float[Length] =
    equilibrium + (initial - equilibrium) * exp(-age / tau)

fn main(): IO Unit = do
    let ages = [0.0 s, 1.0 s, 2.0 s, 4.0 s]
    let trajectory = map(fn t => relax(5.0 m, 1.0 m, t, 2.0 s), ages)
    println(trajectory)
    println(trajectory[2])
```

At zero the position is 5 m. After one time constant it is (1 + 4/e) m, approximately 2.4715 m. Verify both facts before changing the model. Add a test with the initial condition equal to the equilibrium. In that case the trajectory is constant for every sample.

## Prepare a solver input

A time integrator needs a rate function. Give the state its physical dimension and the returned rate the state dimension divided by time. For coupled states with different dimensions, use a solver or callback representation that preserves each field's contract. Choose absolute tolerances in the units of the state and relative tolerances as ratios.

Browse [Ode](/stdlib/Ode?lang=en) for ordinary differential equations. The repository's [numerical examples](https://github.com/Apich-Organization/aevia/tree/v0.1/examples) include adaptive DAE and delay models, event detection, boundary conditions, stochastic noise and PDEs. Start with a small time horizon and finite, valid controls before extending a case.

## Decide whether a result is adequate

Compare the end time and endpoint to expectations. Repeat with tighter tolerances and observe how the answer and the accepted step count change. Check constraints at several points where the model predicts a conservation law. A smaller requested tolerance does not correct a mistaken equation or a wrong initial condition.

## Check your understanding

Use the reference solution to track one final-time error. Hold the time interval fixed while making the solver tolerance stricter or reducing the step, depending on the method. Compare the final-state error and work separately. A method may terminate early at an event or return a convergence diagnostic. Check the result before taking the last time sample.
