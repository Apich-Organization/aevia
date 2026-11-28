# Make a parameter sweep reproducible

A measured length is systematically low. Test several dimensionless gains against a reference. The objective takes one gain and returns a length error. Each trial is independent, so the same objective can run in parallel without changing the candidate list.

## Separate the objective from the schedule

~~~aevia,run
fn prediction(gain: Float, reading: Float[Length]): Float[Length] =
    gain * reading

fn error(gain: Float): Float[Length] =
    abs(prediction(gain, 2.0 m) - 3.0 m)

test "the calibrated gain reproduces the reference" = error(1.5) == 0.0 m
test "a unit gain preserves the uncorrected error" = error(1.0) == 1.0 m

fn main(): IO Unit = do
    let candidates = [0.5, 1.0, 1.5, 2.0]
    let errors = par_map(error, candidates)
    println(candidates)
    println(errors)
    println(sum(errors))
~~~

The errors correspond to the candidate gains in order. A gain of `1.5` gives zero error on this calibration point. The other errors are `2 m`, `1 m` and `1 m`, so the sum is `4 m`. Write down the expected sequence before running the model.

## Check equivalence before comparing speed

Save the file as `sweep.aev`. Run it with `aevia --jobs 1 run sweep.aev` and again with `--jobs 4`. Replace `par_map` with `map` and compare the candidate order and error tensor. With a tiny workload the scheduling overhead can dominate. For a timing comparison, use a larger fixed grid or a more expensive pure function, and keep the workload, build and worker count visible alongside the result. [Parallel execution and profiling](12-parallel-and-profiling.md)

## Extend the study

Add two reference lengths. Use the same gain for all three pairs and choose whether to minimize total absolute error or squared error. Compute the error unit before changing the program. Use a fourth observation to see how the chosen gain performs off the calibration data. Keep the policy, candidate grid, toolchain version and worker count with any saved results.
