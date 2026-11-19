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

on(device, thunk) runs the computation thunk() on a device and returns its result.

**Signature:** `on : (Device, () -> a) -> IO a`

It is an IO action.

**Examples:**
```text
fn run_here(m: Tensor<2, Float>): IO Int = on(device_of(m), fn () => 1 + 1)
```

*Source: Aevia standard library guide.*
## `let to_device`

```aevia
let to_device = to_device
```

Moves a matrix to a compute device and returns it.

**Signature:** `to_device : (Tensor<n,τ>, Device) -> IO (Tensor<n,τ>)`

A Device is Cpu, Gpu(index) or Distributed, and you can obtain the one a matrix already lives on with device_of. It is an IO action. Without GPU support in the build, asking for a Gpu device is a runtime error.

**Examples:**
```text
fn stay(m: Tensor<2, Float>): IO Tensor<2, Float> = to_device(m, device_of(m))
```

*Source: Aevia standard library guide.*
