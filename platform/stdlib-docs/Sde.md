# `Sde`

Source: `stdlib/Sde.aev`

The `Sde` module of §8.20.

## `type SdeMethod`

```aevia
type SdeMethod
```
## `type SdeStructure`

```aevia
type SdeStructure
```
## `type SdeFixedOpts`

```aevia
type SdeFixedOpts<T>
```
## `type SdeAdaptiveOpts`

```aevia
type SdeAdaptiveOpts<T>
```
## `type SdeOpts`

```aevia
type SdeOpts<T>
```
## `type SdeEvent`

```aevia
type SdeEvent<s>
```
## `type SdeHit`

```aevia
type SdeHit<s>
```
## `let sde_at`

```aevia
let sde_at = sde_at
```

**Signature:** `sde_at : (SdePath<s>, Float[T]) -> s`
## `let sde_ensemble_diag`

```aevia
let sde_ensemble_diag = sde_ensemble_diag
```

**Signature:** `sde_ensemble_diag : (((Float[T], s) -> RateOf<s>), ((Float[T], s) -> DiffusionOf<s>), s, SdeOpts<T>, Int) -> Tensor<1, Result<SdePath<s>, SdeError>>`
## `let sde_ensemble_general`

```aevia
let sde_ensemble_general = sde_ensemble_general
```

**Signature:** `sde_ensemble_general : (((Float[T], s) -> RateOf<s>), ((Float[T], s) -> GeneralDiffusionOf<s>), s, SdeOpts<T>, Int) -> Tensor<1, Result<SdePath<s>, SdeError>>`
## `let sde_ensemble_seed`

```aevia
let sde_ensemble_seed = sde_ensemble_seed
```

**Signature:** `sde_ensemble_seed : (Int, Int) -> Int`
## `let sde_events`

```aevia
let sde_events = sde_events
```

**Signature:** `sde_events : (SdePath<s>, Tensor<1, SdeEvent<s>>) -> { path: SdePath<s>, hits: Tensor<1, SdeHit<s>> }`
## `let sde_interp`

```aevia
let sde_interp = sde_interp
```

**Signature:** `sde_interp : SdePath<s> -> ((Float[T]) -> s)`
## `let sde_order`

```aevia
let sde_order = sde_order
```

**Signature:** `sde_order : SdeMethod -> { strong: Float[1], weak: Float[1] }`
## `let sde_order_with`

```aevia
let sde_order_with = sde_order_with
```

**Signature:** `sde_order_with : (SdeMethod, SdeStructure) -> { strong: Float[1], weak: Float[1] }`
## `let sde_resample`

```aevia
let sde_resample = sde_resample
```

**Signature:** `sde_resample : (SdePath<s>, Tensor<1, Float[T]>) -> Tensor<1, s>`
## `let sdeint_diag`

```aevia
let sdeint_diag = sdeint_diag
```

**Signature:** `sdeint_diag : (((Float[T], s) -> RateOf<s>), ((Float[T], s) -> DiffusionOf<s>), s, SdeOpts<T>) -> Result<SdePath<s>, SdeError>`
## `let sdeint_general`

```aevia
let sdeint_general = sdeint_general
```

**Signature:** `sdeint_general : (((Float[T], s) -> RateOf<s>), ((Float[T], s) -> GeneralDiffusionOf<s>), s, SdeOpts<T>) -> Result<SdePath<s>, SdeError>`
## `let wiener`

```aevia
let wiener = wiener
```

**Signature:** `wiener : (Int, Int, Int, Float[T]) -> Tensor<2, Float[T^(1/2)]>`
