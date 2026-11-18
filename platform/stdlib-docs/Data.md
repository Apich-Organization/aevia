# `Data`

Source: `stdlib/Data.aev`

The `Data` module of §8.20.

## `let column`

```aevia
let column = column
```

**Signature:** `column : (Table<ρ>, String) -> Result<Tensor<1, ρ[name]>, DataError>`
## `let column_dyn`

```aevia
let column_dyn = column_dyn
```

**Signature:** `column_dyn : (Table<ρ>, String) -> Result<Tensor<1, Value>, DataError>`
## `let filter`

```aevia
let filter = filter
```

**Signature:** `filter : ((a -> Bool), Tensor<1, a>) -> Tensor<1, a>`

**Signature:** `filter : ((Tensor<n-1, a> -> Bool), Tensor<n, a>) -> Tensor<n, a>`

**Signature:** `filter : ((ρ -> Bool), Table<ρ>) -> Table<ρ>`
## `let join`

```aevia
let join = join
```

**Signature:** `join : (String, Tensor<1, String>) -> String`

**Signature:** `join : (Table<ρ>, Table<σ>, String) -> Table<ρ + σ>`
## `let read_csv`

```aevia
let read_csv = read_csv
```

**Signature:** `read_csv : String -> IO (Result<Table<ρ>, DataError>)`
## `let read_csv_dyn`

```aevia
let read_csv_dyn = read_csv_dyn
```

**Signature:** `read_csv_dyn : String -> IO (Result<Table<Dynamic>, DataError>)`
## `let read_json`

```aevia
let read_json = read_json
```

**Signature:** `read_json : String -> IO (Result<Json, DataError>)`
## `let read_npy`

```aevia
let read_npy = read_npy
```

Read a NumPy Float array in C or Fortran order, preserving its rank.

**Signature:** `read_npy : String -> IO (Result<Tensor<n, Float>, DataError>)`

**Errors**
Returns a domain error for unsupported or malformed data.

*Source: Rust documentation of the `aevia-std` function.*
## `let row`

```aevia
let row = row
```

**Signature:** `row : (Table<ρ>, Int) -> Result<ρ, DataError>`
## `let schema`

```aevia
let schema = schema
```

**Signature:** `schema : Table<ρ> -> Tensor<1, ColumnInfo>`
## `let select`

```aevia
let select = select
```

**Signature:** `select : (Table<ρ>, Tensor<1, String>) -> Table<σ>`
## `let to_dynamic`

```aevia
let to_dynamic = to_dynamic
```

**Signature:** `to_dynamic : Table<ρ> -> Table<Dynamic>`
## `let to_table`

```aevia
let to_table = to_table
```

**Signature:** `to_table : Tensor<1, ρ> -> Result<Table<ρ>, DataError>`
## `let write_csv`

```aevia
let write_csv = write_csv
```

Serialises a table as CSV, with the unit back in the header.

**Signature:** `write_csv : (String, Table<ρ>) -> IO (Result<(), DataError>)`

*Source: Rust documentation of the `aevia-std` function.*
## `let write_json`

```aevia
let write_json = write_json
```

Serialises JSON, compactly.

**Signature:** `write_json : (String, Json) -> IO (Result<(), DataError>)`

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
## `type Table`

```aevia
type Table<a>
```
