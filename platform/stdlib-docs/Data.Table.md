# `Data.Table`

Source: `stdlib/Data/Table.aev`

The `Data/Table` module of §8.20.

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
## `type Table`

```aevia
type Table<a>
```
