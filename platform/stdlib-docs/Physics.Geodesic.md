# `Physics.Geodesic`

Source: `stdlib/Physics/Geodesic.aev`

The `Physics/Geodesic` module of §8.20.

## `type Params`

```aevia
type Params
```

The parameters of the `geodesic` simulation, one field per setting.
## `type GeodesicResult`

```aevia
type GeodesicResult
```

What the `geodesic` simulation returns.
## `fn run`

```aevia
fn run(p: Params): Result<GeodesicResult, SimError>
```

Runs the `geodesic` simulation with the given parameters and returns its result, or `Err(SimError)` if the parameters are rejected.
