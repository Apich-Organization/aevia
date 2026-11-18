# `Device`

Source: `stdlib/Device.aev`

The `Device` module of §8.20.

## `let device_of`

```aevia
let device_of = device_of
```

**Signature:** `device_of : Tensor<n,τ> -> Device`
## `let on`

```aevia
let on = on
```

**Signature:** `on : (Device, () -> a) -> IO a`
## `let to_device`

```aevia
let to_device = to_device
```

**Signature:** `to_device : (Tensor<n,τ>, Device) -> IO (Tensor<n,τ>)`
