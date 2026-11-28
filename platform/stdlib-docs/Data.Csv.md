# `Data.Csv`

Source: `stdlib/Data/Csv.aev`

The `Data/Csv` module of §8.20.

## `let column`

```aevia
let column = column
```

Looks up a column by name at run time and returns its cells in a rank-one tensor.

**Signature:** `column : (Table<ρ>, String) -> Result<Tensor<1, a>, DataError>`

The surrounding expression supplies the expected element type independently of the table’s row type. Numeric cells are dimensionless Float values and other cells are String values. A missing column returns Err(DataError). Use column_dyn when the cells’ types are determined at run time.

**Examples:**
```text
fn readings(t: Table<{ time: Float }>, name: String): Result<Tensor<1, Float>, DataError> = column(t, name)
```

*Source: Aevia standard library guide.*
## `let column_dyn`

```aevia
let column_dyn = column_dyn
```

Looks up a column by name at run time and returns a rank-one tensor of dynamic Value cells.

**Signature:** `column_dyn : (Table<ρ>, String) -> Result<Tensor<1, Value>, DataError>`

Numeric cells are dimensionless Float values and other cells are String values. This works with any table row type. A missing column returns Err(DataError).

**Examples:**
```text
fn pick(t: Table<Dynamic>, name: String): Result<Tensor<1, Value>, DataError> = column_dyn(t, name)
```

*Source: Aevia standard library guide.*
## `let read_csv`

```aevia
let read_csv = read_csv
```

Reads comma-separated .csv or tab-separated .tsv files.

**Signature:** `read_csv : String -> IO (Result<Table<ρ>, DataError>)`

An explicit type argument or the surrounding expression supplies the row type, which is used only for static checking. The reader checks quoting and row widths using the file’s header. Header units such as s in time [s] are column metadata reported by schema, and numeric cells are dimensionless Float values. The IO action returns Ok(table) or Err(DataError).

**Examples:**
```text
read_csv<{ time: Float, speed: Float }>("run.csv")
```

*Source: Aevia standard library guide.*
## `let read_csv_dyn`

```aevia
let read_csv_dyn = read_csv_dyn
```

Reads comma-separated .csv or tab-separated .tsv files as Table<Dynamic>.

**Signature:** `read_csv_dyn : String -> IO (Result<Table<Dynamic>, DataError>)`

The reader checks quoting and row widths. Header units such as s in time [s] are column metadata reported by schema, and numeric cells are dimensionless Float values. The IO action returns Ok(table) or Err(DataError). Use column_dyn to get values by column name.

**Examples:**
```text
read_csv_dyn("run.tsv")
```

*Source: Aevia standard library guide.*
## `let write_csv`

```aevia
let write_csv = write_csv
```

Writes a table as comma-separated CSV and includes any column units in the header.

**Signature:** `write_csv : (String, Table<ρ>) -> IO (Result<(), DataError>)`

The IO action returns Ok(()) or Err(DataError).

**Examples:**
```text
fn dump(t: Table<{ x: Float }>) = write_csv("out.csv", t)
```

*Source: Aevia standard library guide.*
## `type Table`

```aevia
type Table<a>
```

A table with named columns and a row type parameter. Table<{ time: Float }> provides a static row type for operations such as row. For a table read from a file, the header supplies the actual column names and optional unit metadata. Use schema to inspect that metadata and column_dyn to read cells whose types are determined at run time.
