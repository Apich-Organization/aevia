# Tensors and linear algebra

A tensor's rank counts its axes. Its shape gives the length of every axis. `Tensor<2, Float>` states a rank, while `Tensor<[2,3], Float>` also states a shape. Put fixed shapes on function boundaries when their axes have mathematical meaning.

## Apply a linear model

Each row of a matrix below assigns weights to two independent readings. Restating the operation as a matrix-vector product lets the checker relate the common dimensions and axes.

```aevia,run
fn predict(weights: Tensor<[2,2], Float>, inputs: Tensor<[2], Float[Length]>): Tensor<[2], Float[Length]> =
    weights @ inputs

fn main(): IO Unit = do
    let weights = [[0.75, 0.25], [0.4, 0.6]]
    let values = predict(weights, [2.0 m, 6.0 m])
    println(shape(values))
    println(values)
    println(mean(values))
```

The predictions are 3 m and 4.4 m. Change the vector length and check the file. The common axis is part of the operation's contract. `*` represents element-wise multiplication for tensor operands. Use `@` for a linear algebra product.

## Choose a solver by the structure

For a dense linear system, use `linsolve` with the coefficient matrix and the right-hand side. Inspect the returned result and validate with the residual `A @ x - b`. Exact equality is useful for exact arithmetic. With floating-point inputs, compare a norm against a tolerance with matching units. A small residual alone does not imply a well-conditioned system.

Use sparse representations for matrices with few nonzero entries, such as graph incidence matrices or discretized differential operators. The [sparse module](/stdlib/LinearAlgebra.Sparse?lang=en) distinguishes layouts and exposes explicit conversions. Use a dense reference calculation on a small instance before increasing the grid size.

## Practice with an altered model

Add a third sensor and write down the new shape before changing the literal. Then test whether one row sums to one, the invariant that makes its prediction a weighted average. Look up axis-specific reductions before applying a global reduction to a multidimensional dataset.
