# `Math.Geometry`

Source: `stdlib/Math/Geometry.aev`

The `Math/Geometry` module of §8.20.

## `let distance`

```aevia
let distance = distance
```

Euclidean distance between two points given as vectors of equal length, in the coordinates' dimension.

**Signature:** `distance : (Tensor<1, Real[L]>, Tensor<1, Real[L]>) -> Real[L]`

**Kernel signature** (derived):
- `distance : (Real, Real) -> Real`
- `distance : (Dyn<term>, Dyn<term>) -> Dyn<term>`

**Dimensions:** the result has the dimension of argument 1

**Examples:**
```text
distance([0.0 m, 0.0 m], [3.0 m, 4.0 m])
```

*Source: Aevia standard library guide.*
## `let polygon_area`

```aevia
let polygon_area = polygon_area
```

Area of a simple polygon from a matrix of (x, y) vertices in order, via the shoelace formula; the result has the square of the coordinates' dimension.

**Signature:** `polygon_area : Tensor<2, Real[L]> -> Real[L^2]`

**Examples:**
```text
polygon_area([[0.0 m, 0.0 m], [2.0 m, 0.0 m], [2.0 m, 3.0 m]])
```

*Source: Aevia standard library guide.*
