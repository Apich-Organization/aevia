# `Device`

Source: `stdlib/Device.aev`

The `Device` module of §8.20.

## `let device_of`

```aevia
let device_of = device_of
```

The device a matrix currently lives on.

**Signature:** `device_of : Tensor<n,τ> -> Device`

**Examples:**
```text
device_of([[1.0, 2.0], [3.0, 4.0]])
```

*Source: Aevia standard library guide.*
## `let on`

```aevia
let on = on
```

Runs the zero-argument function thunk on the requested CPU device.

**Signature:** `on : (Device, () -> a) -> IO a`

This IO action currently supports Cpu. Requests for Gpu or Distributed produce E1001.

**Examples:**
```text
fn run_here(m: Tensor<2, Float>): IO Int = on(device_of(m), fn () => 1 + 1)
```

*Source: Aevia standard library guide.*
## `let to_device`

```aevia
let to_device = to_device
```

Places a tensor on the requested CPU device and returns it.

**Signature:** `to_device : (Tensor<n,τ>, Device) -> IO (Tensor<n,τ>)`

This IO action currently supports Cpu. Use device_of to query the tensor location. Gpu and Distributed placement produce E1001 even if the RSSN host can discover a GPU.

**Examples:**
```text
fn stay(m: Tensor<2, Float>): IO Tensor<2, Float> = to_device(m, device_of(m))
```

*Source: Aevia standard library guide.*
