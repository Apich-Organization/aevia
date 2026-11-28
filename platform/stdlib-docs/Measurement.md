# `Measurement`

Source: `stdlib/Measurement.aev`

The `Measurement` module of §8.20.

## `let measure`

```aevia
let measure = measure
```

Construct a measurement record containing a value and an uncertainty with the same physical dimension.

**Signature:** `measure : (Real[d], Real[d]) -> Measurement<d>`

**Examples:**
```text
measure(3 m, 0.1 m)
```

*Source: Aevia standard library guide.*
## `let uncertainty`

```aevia
let uncertainty = uncertainty
```

The uncertainty field of a measurement: its one-sigma error, in the measurement's dimension.

**Signature:** `uncertainty : Measurement<d> -> Real[d]`

**Examples:**
```text
uncertainty(measure(1.5 m, 0.1 m))
```

*Source: Aevia standard library guide.*
## `let value`

```aevia
let value = value
```

The value field of a measurement, the number without its uncertainty.

**Signature:** `value : Measurement<d> -> Real[d]`

**Examples:**
```text
value(measure(1.5 m, 0.1 m))
```

*Source: Aevia standard library guide.*
