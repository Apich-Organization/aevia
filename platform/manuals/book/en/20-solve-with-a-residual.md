# Solve a system and check the residual

Two instruments mix two underlying displacements. The calibration model uses a dimensionless matrix and measured lengths. Recover the displacements by solving the matrix equation, then verify the answer by substituting it into the original model.

## Try a case you can solve on paper

~~~aevia,run
import LinearAlgebra as LA

fn residual_ok(): Bool =
    let a = [[2.0, 1.0], [1.0, 3.0]]
    let b = [5.0 m, 10.0 m]
    match LA.linsolve(a, b) with
    | Ok(x) -> LA.norm(a @ x - b) < 1e-9 m
    | Err(_) -> false

test "substituting the solution recovers the observations" = residual_ok()

fn main(): IO Unit = do
    let a = [[2.0, 1.0], [1.0, 3.0]]
    let b = [5.0 m, 10.0 m]
    match LA.linsolve(a, b) with
    | Ok(x) -> do
        println(x)
        println(LA.norm(a @ x - b))
    | Err(error) -> println(error)
~~~

The first row checks `2x₀ + x₁ = 5 m` and the second checks `x₀ + 3x₁ = 10 m`. The expected solution is `[1 m, 3 m]`. Both the unknown vector and the residual carry lengths. The tolerance does too. Run `aevia test` on this file before changing the observations.

## Diagnose the solve

If the solver returns an error, inspect the input dimensions and the matrix rank before changing tolerances. If the solve succeeds with a large residual, check the orientation of the matrix, the scaling of rows and the accuracy of the measurements. If the residual is small, still check conditioning when the rows are nearly dependent. A small perturbation of the observations can substantially change the inferred displacement.

## Compare two systems

Replace the second row with `[4.0, 2.0]`. Predict what information the second equation adds before running the solver. Then change the row to `[4.0, 2.000001]` and compare the solutions when the second observation changes by one micrometre. Explain which number in the output measures fit and which measures sensitivity. [Tensor operations](06-tensors.md)
