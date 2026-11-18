# `Test`

Source: `stdlib/Test.aev`

The `Test` module of §8.20.

## `let approx_eq`

```aevia
let approx_eq = approx_eq
```

**Signature:** `approx_eq : (Scalar<ρ,d>, Scalar<ρ,d>, Scalar<ρ,d>) -> Bool`
## `let assert`

```aevia
let assert = assert
```

**Signature:** `assert : Bool -> IO Unit`
## `let assert_eq`

```aevia
let assert_eq = assert_eq
```

**Signature:** `assert_eq : (Eq a, Show a) => (a, a) -> IO Unit`
## `let compile_fail`

```aevia
let compile_fail = compile_fail
```

**Signature:** `compile_fail : String -> Bool`
## `let forall`

```aevia
let forall = forall
```

**Signature:** `forall : Arbitrary a => (a -> Prop) -> Bool`
