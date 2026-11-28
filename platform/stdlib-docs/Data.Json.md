# `Data.Json`

Source: `stdlib/Data/Json.aev`

The `Data/Json` module of §8.20.

## `let from_json`

```aevia
let from_json = from_json
```

Decodes a Json value into the type you ask for, returning Err(SimError) when the JSON does not have that shape.

**Signature:** `from_json : Json -> Result<a, SimError>`

State the target type with an annotation.

**Examples:**
```text
let back: Result<Int, SimError> = from_json(to_json(3))
```

*Source: Aevia standard library guide.*
## `let read_json`

```aevia
let read_json = read_json
```

Reads a JSON file as a dynamic Json value, returning Ok(json) or Err(DataError).

**Signature:** `read_json : String -> IO (Result<Json, DataError>)`

Convert parts of it to typed values with from_json. It is an IO action.

**Examples:**
```text
read_json("config.json")
```

*Source: Aevia standard library guide.*
## `let to_json`

```aevia
let to_json = to_json
```

Encodes a value as a dynamic Json value, ready for write_json or simulate.

**Signature:** `to_json : a -> Json`

It does not check that the value can be represented; a value that cannot be encoded fails when it is used.

**Examples:**
```text
to_json({ name = "run1", steps = 100 })
```

*Source: Aevia standard library guide.*
## `let write_json`

```aevia
let write_json = write_json
```

Writes a Json value to a file.

**Signature:** `write_json : (String, Json) -> IO (Result<(), DataError>)`

Build the value with to_json. It is an IO action returning Ok(()) or Err(DataError).

**Examples:**
```text
write_json("out.json", to_json([1, 2, 3]))
```

*Source: Aevia standard library guide.*
