# Dimensions and units

A dimension is a type-level product of base quantities. Length divided by time is `Length/Time`. A unit gives a scale and a display convention for a quantity of that dimension. Scientific code often receives one display unit and reports another while the model's signature stays the same.

## Write a dimensioned signature

Use the distance and elapsed time to compute a speed. The annotation also makes a future edit that swaps the two arguments visible to the checker.

```aevia,run
fn pace(distance: Float[Length], elapsed: Float[Time]): Float[Length/Time] =
    distance / elapsed

fn main(): IO Unit = do
    let distance = 2.4 km
    let elapsed = 720.0 s
    let speed = pace(distance, elapsed)
    println(speed)
    println(value_in(m, distance))
```

`value_in(m, distance)` provides the numeric magnitude in metres. Use it at a boundary where another tool expects a plain number. Keep the original quantity for calculations. Use `to(m, distance)` when you want a quantity displayed in metres.

## Distinguish points and differences

An affine temperature such as `25.0 °C` is a point. The difference between two readings is a temperature interval. A formula that adds two points has no physical interpretation in this model.

```aevia,run
fn main(): IO Unit = do
    let earlier = 18.0 °C
    let later = 25.0 °C
    let change = later - earlier
    println(value_in(K, later))
    println(change)
```

Expect approximately 298.15 for the absolute temperature in kelvin and a difference of seven kelvin. The affine offset applies to the point conversion. It cancels when subtracting readings.

## Choose exact and approximate data

Use exact expressions for known rational factors and unit identities. Give numeric solver interfaces floating-point values with the correct dimension. For functions such as `exp`, the argument must be dimensionless. In a decay model, write `exp(-t / lifetime)` with `t` and `lifetime` both in time units.

Try to call `pace` with pressure and time. Keep the diagnostic as a reminder of the dimensional contract. Then modify the temperature example to report the difference between 15 °C and 20 °C. Consult [Tensors and linear algebra](06-tensors.md) for arrays of dimensioned readings.

## Check your understanding

Compare `100 cm` with `1 m`, and write down what should agree before evaluating both. Repeat with the physical expression `2.0 m / 4.0 s` and a deliberately mismatched input. The first pair checks conversion, while the second checks compatibility. Keep dimensions on the public function, and choose the display unit where you report the result.
