# `Data.Csv`

Source: `stdlib/Data/Csv.aev`

The `Data/Csv` module of §8.20.

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
## `let write_csv`

```aevia
let write_csv = write_csv
```

Serialises a table as CSV, with the unit back in the header.

**Signature:** `write_csv : (String, Table<ρ>) -> IO (Result<(), DataError>)`

*Source: Rust documentation of the `aevia-std` function.*
## `type Table`

```aevia
type Table<a>
```
