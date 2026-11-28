# `Physics.Ising`

Source: `stdlib/Physics/Ising.aev`

The `Physics/Ising` module of §8.20.

## `type Params`

```aevia
type Params
```

The parameters of the `ising` simulation, one field per setting.
## `type IsingResult`

```aevia
type IsingResult
```

What the `ising` simulation returns.
## `fn run`

```aevia
fn run(p: Params): Result<IsingResult, SimError>
```

Runs the `ising` simulation with the given parameters and returns its result, or `Err(SimError)` if the parameters are rejected.
