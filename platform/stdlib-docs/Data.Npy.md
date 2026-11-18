# `Data.Npy`

Source: `stdlib/Data/Npy.aev`

The `Data/Npy` module of §8.20.

## `let read_npy`

```aevia
let read_npy = read_npy
```

Read a NumPy Float array in C or Fortran order, preserving its rank.

**Signature:** `read_npy : String -> IO (Result<Tensor<n, Float>, DataError>)`

**Errors**
Returns a domain error for unsupported or malformed data.

*Source: Rust documentation of the `aevia-std` function.*
## `let write_npy`

```aevia
let write_npy = write_npy
```

Encode a valid NumPy Float array.

**Signature:** `write_npy : (String, Tensor<n, Float>) -> IO (Result<(), DataError>)`

**Panics**
Panics if the shape and data do not agree; use [`try_write_npy`] for
untrusted input and a recoverable error.

*Source: Rust documentation of the `aevia-std` function.*
