# `Physics`

Source: `stdlib/Physics.aev`

The `Physics` module of §8.20.

## `let simulate`

```aevia
let simulate = simulate
```

Runs a named simulation scenario of the scientific kernel (for example "ising") from JSON parameters and returns its JSON result, or Err(SimError) for an unknown scenario or bad parameters.

**Signature:** `simulate : (String, Json) -> Result<Json, SimError>`

Most scenarios also have a typed wrapper module such as Physics.Ising, which is easier to use.

**Examples:**
```text
simulate("ising", to_json({ width = 8, height = 8, temperature = 2.0, mc_steps = 100, seed = Some(1) }))
```

*Source: Aevia standard library guide.*
