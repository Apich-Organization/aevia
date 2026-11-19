# `Test`

Source: `stdlib/Test.aev`

The `Test` module of §8.20.

## `let approx_eq`

```aevia
let approx_eq = approx_eq
```

approx_eq(a, b, tol) is true when a and b differ by at most the absolute tolerance tol.

**Signature:** `approx_eq : (Scalar<ρ,d>, Scalar<ρ,d>, Scalar<ρ,d>) -> Bool`

All three must have the same dimension, so approx_eq(1.0 m, 1.001 m, 0.01 m) is the right way to compare lengths.

**Examples:**
```text
approx_eq(0.1 + 0.2, 0.3, 0.000001)
```

*Source: Aevia standard library guide.*
## `let assert`

```aevia
let assert = assert
```

Fails the program with a diagnostic when the Bool is false, and does nothing when it is true.

**Signature:** `assert : Bool -> IO Unit`

It is an IO action, so call it inside a do block or at the top level of a test.

**Examples:**
```text
assert(1 + 1 == 2)
```

*Source: Aevia standard library guide.*
## `let assert_eq`

```aevia
let assert_eq = assert_eq
```

Fails with both values shown when the two arguments are not equal.

**Signature:** `assert_eq : (Eq a, Show a) => (a, a) -> IO Unit`

It is an IO action. For Floats compare with approx_eq instead, since rounding errors make == unreliable.

**Examples:**
```text
assert_eq(2 + 2, 4)
```

*Source: Aevia standard library guide.*
## `let compile_fail`

```aevia
let compile_fail = compile_fail
```

Takes Aevia source as a string and returns true if it does NOT compile.

**Signature:** `compile_fail : String -> Bool`

Use it in tests to check that the type checker rejects a unit mismatch or other mistake; the source is compiled on its own, with only the Prelude in scope.

**Examples:**
```text
compile_fail("let x = 1 m + 1 s")
```

*Source: Aevia standard library guide.*
## `let forall`

```aevia
let forall = forall
```

Checks a proposition over fixed numeric samples.

**Signature:** `forall : (a -> Prop) -> Bool`

Use a unary predicate taking a dimensionless Real. The samples are 0, 1, -1, 2, -3, and 1/2. A false verdict returns false. If no verdict is false but a sample is undecidable, the call raises E0804.

**Examples:**
```text
forall(fn (x: Real) => x + 0 == x)
```

*Source: Aevia standard library guide.*
