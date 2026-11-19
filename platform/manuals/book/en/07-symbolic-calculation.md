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

## Change the experiment

Replace the constant acceleration term with a cubic term representing a constant jerk. Choose its units to make position a length again. Predict the derivative's dimensions and its value at 0 s before running the modified example.
