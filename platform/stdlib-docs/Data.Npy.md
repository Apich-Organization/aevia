# `Data.Npy`

Source: `stdlib/Data/Npy.aev`

The `Data/Npy` module of §8.20.

## `let read_npy`

```aevia
let read_npy = read_npy
```

Reads a NumPy .npy file of floats as a tensor.

**Signature:** `read_npy : String -> IO (Result<Tensor<n, Float>, DataError>)`

You must state the rank as a type argument, read_npy<2>(path), so the result type is known; a file of a different rank gives Err(DataError). It is an IO action.

**Examples:**
```text
read_npy<2>("matrix.npy")
```

*Source: Aevia standard library guide.*
## `let write_npy`

```aevia
let write_npy = write_npy
```

Writes a dimensionless Float tensor of any rank to a NumPy .npy file (little-endian float64).

**Signature:** `write_npy : (String, Tensor<n, Float>) -> IO (Result<(), DataError>)`

It is an IO action returning Ok(()) or Err(DataError).

**Examples:**
```text
write_npy("matrix.npy", [[1.0, 2.0], [3.0, 4.0]])
```

*Source: Aevia standard library guide.*
