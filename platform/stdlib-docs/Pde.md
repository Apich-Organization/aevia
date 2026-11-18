# `Pde`

Source: `stdlib/Pde.aev`

The `Pde` module of §8.20.

## `let solve_advection`

```aevia
let solve_advection = solve_advection
```

**Signature:** `solve_advection : AdvectionSpec -> Result<Tensor<1, Field>, PdeError>`
## `let solve_heat`

```aevia
let solve_heat = solve_heat
```

**Signature:** `solve_heat : HeatSpec -> Result<Tensor<1, Field>, PdeError>`
## `let solve_poisson`

```aevia
let solve_poisson = solve_poisson
```

**Signature:** `solve_poisson : PoissonSpec -> Result<Field, PdeError>`
## `let solve_wave`

```aevia
let solve_wave = solve_wave
```

**Signature:** `solve_wave : WaveSpec -> Result<Tensor<1, Field>, PdeError>`
