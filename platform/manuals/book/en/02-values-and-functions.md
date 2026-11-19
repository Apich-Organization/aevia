# Values and functions

A `let` binds a name to a value. Define the conversion factors and assumptions of a model once, near the functions that consume them. A function takes argument values and returns a value. Its signature describes the public boundary even if its body can be inferred.

## Predict the order of growth

Start with the first terms of a geometric sequence. The ratio and the index are distinct kinds of input, so the function gives them different types. The record collects related quantities by name.

```aevia,run
fn sample(initial: Float, ratio: Float, step: Int): Float =
    initial * ratio ^ step

fn main(): IO Unit = do
    let forecast = { day_1 = sample(100.0, 1.1, 1), day_7 = sample(100.0, 1.1, 7) }
    println(forecast.day_7)
```

The last value should be close to 194.87. Increase the step while keeping the initial value fixed. Decimal rounding becomes visible if you print enough digits. A scientific result usually needs a tolerance, even when the equation is simple.

## Use pure functions and scoped names

A block commonly has local bindings followed by its final value. A lambda, such as `fn (x, y) => x + y`, carries arguments and can capture an enclosing binding. Give an anonymous operation a name when you want to test or reuse it. Multi-argument functions take a tuple of arguments at the call site. If you want a one-argument function for a pipeline, use an explicit lambda or a call with a placeholder.

Write and run this program. The callback supplied to `map` captures `offset`.

```aevia,run
fn adjusted(values: Tensor<1, Float>, offset: Float): Tensor<1, Float> =
    map(fn x => x + offset, values)

fn main(): IO Unit = do
    let baseline = [3.0, 4.5, 5.5]
    let result = adjusted(baseline, 0.25)
    println(result)
    println(mean(result))
```

## Inspect types before predicting results

Try `:type adjusted` in the REPL after entering the function. Then call it with a scalar. The error reports the argument boundary, rather than attempting to reinterpret the number as a one-element tensor. In a checked file, add narrow signatures at model boundaries and allow the checker to infer intermediate bindings.

For practice, add a `ratio` field to the forecasting record and write a function that produces both the seventh-day prediction and the ratio to the initial value. Use [Dimensions and units](03-dimensions-and-units.md) when the forecast carries a physical unit.

## Check your understanding

Give a pure function a name, bind two inputs, and call it twice. Add a local binding with the same name in an inner scope and predict which value the function observes. Then give the function an explicit parameter for the value instead of capturing it. Use the explicit parameter when callers need to vary it or when the inputs read more clearly at the call site.
