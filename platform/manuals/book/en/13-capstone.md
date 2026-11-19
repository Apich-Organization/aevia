# Capstone — estimate acceleration from position readings

Build a small analysis from a physical model, a validation choice and two independent tests. A cart starts from rest with constant acceleration. Its measured position includes an unknown offset. Given three samples with equal spacing (h), the second difference (x_2 - 2x_1 + x_0) equals (a h^2). The initial position and speed cancel.

## Implement the measurement model

Save this program as `cart.aev`. The variant lets the caller distinguish a computed result from a rejected sample series. The three readings are required by the tensor shape. The spacing remains an explicit input so the same analysis can be used for a different sampling frequency.

```aevia,run
type Estimate = | Acceleration(Float[Length/Time^2]) | InvalidSpacing

fn estimate(samples: Tensor<[3], Float[Length]>, spacing: Float[Time]): Estimate =
    if spacing <= 0.0 s then InvalidSpacing
    else Acceleration((samples[2] - 2 * samples[1] + samples[0]) / spacing^2)

fn describe(result: Estimate): String =
    match result with
    | Acceleration(value) -> "acceleration = {value}"
    | InvalidSpacing -> "sample spacing must be positive"

fn near_expected(samples: Tensor<[3], Float[Length]>, spacing: Float[Time], expected: Float[Length/Time^2]): Bool =
    match estimate(samples, spacing) with
    | Acceleration(value) -> abs(value - expected) < 0.000001 m/s^2
    | InvalidSpacing -> false

test "second difference removes position offset" =
    near_expected([3.0 m, 4.0 m, 7.0 m], 1.0 s, 2.0 m/s^2)

test "uniform motion has zero acceleration" =
    near_expected([10.0 m, 9.0 m, 8.0 m], 0.5 s, 0.0 m/s^2)

test "coincident samples are rejected" =
    match estimate([0.0 m, 1.0 m, 2.0 m], 0.0 s) with
    | InvalidSpacing -> true
    | Acceleration(_) -> false

fn main(): IO Unit = do
    let readings = [1.1 m, 1.6 m, 3.1 m]
    println(describe(estimate(readings, 0.5 s)))
    println(describe(estimate(readings, 0.0 s)))
```

The first report is 4 m/s². The second explains the invalid input. Run `aevia test cart.aev` to validate the properties of the estimate. The checker guarantees that the model combines sampled lengths and times in the intended dimensional relationship. The tests check coefficients and the sample policy.

## Change the sampling assumptions

Move the three measurement times into a second tensor and check their differences. Choose a tolerance for equal spacing based on the timing instrument. If spacing can differ, derive a formula for nonuniform times and add a test with deliberately unequal intervals. Give the new model a separate name until both formulas agree on the uniform case.

## Build the analysis into a package

Move the estimator and its test cases into `src/Cart.aev` in a package created with `aevia init`. Keep the report and file input in the entry module. Add a small input fixture with a documented expected result. A larger dataset should include sample identifiers and enough metadata to reconstruct the spacing. Add a documentation example and run `aevia test --doc src` before sharing the package.

These steps give you a pattern for a larger project: express physical constraints at the boundary, keep the core pure, verify known cases, and report errors where the caller can act on them.

## Check your understanding

For uniform spacing, translate all three positions by the same distance and predict the result. Repeat after adding a constant velocity term to each sampled position. The second difference cancels both. Next add a known perturbation to the middle sample and calculate its effect on the estimate before running the model. These checks expose both the invariant and the sensitivity of the estimator.
