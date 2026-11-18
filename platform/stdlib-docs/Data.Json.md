# `Data.Json`

Source: `stdlib/Data/Json.aev`

The `Data/Json` module of §8.20.

## `let from_json`

```aevia
let from_json = from_json
```

**Signature:** `from_json : Json -> Result<a, SimError>`
## `let read_json`

```aevia
let read_json = read_json
```

**Signature:** `read_json : String -> IO (Result<Json, DataError>)`
## `let to_json`

```aevia
let to_json = to_json
```

**Signature:** `to_json : a -> Json`
## `let write_json`

```aevia
let write_json = write_json
```

Serialises JSON, compactly.

**Signature:** `write_json : (String, Json) -> IO (Result<(), DataError>)`

*Source: Rust documentation of the `aevia-std` function.*
