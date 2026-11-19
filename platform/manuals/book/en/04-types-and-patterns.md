# Types, variants and patterns

A record groups fields that exist together. A variant describes alternatives, each with its own payload. Use a variant when a reading can legitimately be missing or rejected. A `match` handles each case where the decision is made.

## Represent the decision in the result

This program filters a reading using the physical range of a sensor. An accepted reading carries a length. A rejection carries a reason suitable for a report.

```aevia,run
type Reading = | Accepted(Float[Length]) | Rejected(String)

fn calibrate(raw: Float[Length]): Reading =
    if raw < 0.0 m then Rejected("below the sensor range")
    else Accepted(raw + 0.02 m)

fn describe(reading: Reading): String =
    match reading with
    | Accepted(value) -> "calibrated: {value}"
    | Rejected(reason) -> "invalid: {reason}"

fn main(): IO Unit = do
    println(describe(calibrate(0.8 m)))
    println(describe(calibrate(-0.1 m)))
```

Remove one branch from `describe` and check the file. The type checker reports the missing case. Use a wildcard when all other cases really share a response. Cases with distinct scientific meaning should have separate handling.

## Handle `Option` and `Result`

`Some(value)` and `None` describe presence. `Ok(value)` and `Err(reason)` describe success or failure visible to the caller. Inspect the signature before combining library calls, especially when numerical functions return a diagnostic or a structured convergence record.

A useful initial policy is to surface a failure in a `match`. Choosing a replacement requires a scientific reason. For example, a missing humidity value should not be counted as zero humidity in an average.

## Add a regression case

Add an `OutOfRange` variant for measurements over 2 m, then update `describe` and add a `test` that exercises the boundary. Check a value just below and just above the limit. Continue with [IO, data and errors](05-io-and-data.md) for returning and reporting failures during file processing.
