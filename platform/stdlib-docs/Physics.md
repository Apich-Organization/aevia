# `Physics`

Source: `stdlib/Physics.aev`

The `Physics` module of §8.20.

## `let simulate`

```aevia
let simulate = simulate
```

Runs a scenario with JSON parameters and returns its JSON result.

**Signature:** `simulate : (String, Json) -> Result<Json, SimError>`

**Errors**

The backend's message: an unknown scenario, parameters that do not parse, or
a failed simulation.

*Source: Rust documentation of the `aevia-std` function.*
