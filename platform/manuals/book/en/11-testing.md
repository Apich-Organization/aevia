# Checking scientific results

Dimension checks catch incompatible equations. A test checks the meaning of an equation, its numerical stability or the behavior at a boundary. Build tests from results you can derive independently.

## Test a model

This function returns circular area. The first test uses zero, which should be exact. The second checks the scaling relationship for a circle with twice the radius. The main program is for exploration, while `aevia test` evaluates the test declarations.

```aevia,run
fn circle(radius: Float[Length]): Float[Length^2] =
    3.141592653589793 * radius^2

test "zero radius has zero area" = circle(0.0 m) == 0.0 m^2

test "double radius gives four times the area" =
    abs(circle(2.0 m) - 4 * circle(1.0 m)) < 0.000001 m^2

fn main(): IO Unit =
    println(circle(3.0 m))
```

Save this as `circle.aev` and run `aevia test circle.aev`. Then replace the exponent with a dimensionally valid but scientifically wrong formula, such as `radius * 1.0 m`. Run it again and check which assertion catches the mistake.

## Add examples that compile

A `##` documentation comment supports fenced Aevia examples. `aevia test --doc` checks their code. Each example is a separate synthetic module, so declare the required bindings or add imports. A leading `compile_fail E0401` is appropriate when the example is intended to demonstrate a particular dimensional rejection. Use executable test declarations for assertions about numerical results.

## Use a consistent error scale

Choose a tolerance from the problem and report it in the same dimension as the residual. A relative test works away from zero. Near zero, choose a meaningful absolute scale. For algorithms with branches, include a representative case for each branch and a value on either side of its boundary. When work is parallel, test the output with one worker and several workers.

## Check your understanding

For a numerical regression, keep an expected result obtained independently, such as a hand derivation or an exact solution. Exercise at least one boundary and one relation that should survive changes to representation. A dimension-specific tolerance states how much error is acceptable and guards against comparing an error to an unrelated unit.
