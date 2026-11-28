# Symbolic calculation

Aevia can retain an exact expression and ask RSSN to transform it. That is useful when you need a formula before substituting a particular input. Keep the function you want to transform pure and write its dimensioned parameters explicitly where they clarify the operation.

## Differentiate a position model

For a constant speed (v) and acceleration (a), position is (v t + a t^2 / 2). The derivative is a speed. Evaluate it at two times to see how the speed changes.

```aevia,run
fn position(t: Real[Time]): Real[Length] =
    (2 m / 1 s) * t + (3 m / 1 s^2) * t^2 / 2

fn main(): IO Unit = do
    let speed = diff(position)
    println(position(4 s))
    println(speed(0 s))
    println(speed(4 s))
```

The exact position at four seconds is 32 m. The derivative runs from 2 m/s at zero to 14 m/s at four seconds. Test the result at zero to catch a missing coefficient and at a nonzero input to catch a missing power.

## Select exact or numerical operations

`diff(f)` returns a callable derivative. A definite integral uses a function and bounds. A symbolic integral can retain an unresolved term, and exact roots may have a symbolic representation. Use `approx` to evaluate an exact expression numerically and handle its result. For root finding, distinguish the analytic solution set from a numerical solve that starts near one location. Use the equation residual and consider other starting values when roots may be multiple.

When differentiating with units, the result has the output dimension divided by the input dimension. Integration multiplies the dimensions. Track dimensions at the boundary of each transformation so that a change to the model stays visible.

## Find an RSSN operator and call it with a checked function

Run `aevia rssn --describe num_native_par_map_slice` to see the linked signature,
dimension rule and available bindings. `Rssn.call_ho` receives ordinary values and
checked callbacks in separate lists. Keep each list in its order within the
signature. This example supplies the measured vector and the two integer
controls as ordinary values. The callback transforms a distance into a time.

```aevia,run
fn round_trip_time(distance: Float[Length]): Float[Time] =
    (distance / 1.0 m) * 2.0 s

fn round_trip_times(): Result<Tensor<1, Float[Time]>, OpError> =
    let ordinary = [to_value([1.0 m, 2.0 m, 3.0 m]), to_value(1), to_value(3)]
    match Rssn.call_ho("num_native_par_map_slice", ordinary, [to_callback(round_trip_time)]) with
    | Ok(value) -> from_value<Tensor<1, Float[Time]>>(value)
    | Err(error) -> Err(error)

test "the returned values retain order and time units" =
    round_trip_times() == Ok([2.0 s, 4.0 s, 6.0 s])

fn main(): IO Unit = do
    println(round_trip_times())
```

`from_value` checks the returned tensor's rank, element type and dimension.
The callback runs synchronously and its handle belongs to this call. Use
`aevia rssn --audit` to explore the coverage of the linked build, or emit its
modules with `aevia rssn --emit --out generated --namespaces`.

## Change the experiment

Replace the constant acceleration term with a cubic term representing a constant jerk. Choose its units to make position a length again. Predict the derivative's dimensions and its value at 0 s before running the modified example.

## Check your understanding

Choose a point away from singularities and evaluate the symbolic derivative there. Approximate the same derivative with a centered difference using the original function. Halve the step and compare again. Agreement checks your model in that neighborhood. Domain assumptions and a successful evaluation at one point have distinct roles in the symbolic calculation.
