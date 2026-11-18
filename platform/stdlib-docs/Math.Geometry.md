# `Math.Geometry`

Source: `stdlib/Math/Geometry.aev`

The `Math/Geometry` module of §8.20.

## `let distance`

```aevia
let distance = distance
```

Euclidean distance `abs(a - b)` between complex points `a` and `b`.

**Signature:** `distance : (Tensor<1, Real[L]>, Tensor<1, Real[L]>) -> Real[L]`

**Kernel signature** (derived):
- `distance : (Real, Real) -> Real`
- `distance : (Dyn<term>, Dyn<term>) -> Dyn<term>`

**Dimensions:** the result has the dimension of argument 1

*Source: rssn operator `complex.distance`.*
## `let polygon_area`

```aevia
let polygon_area = polygon_area
```

The non-negative area of a flat `[x0, y0, x1, y1, ...]` polygon.

**Signature:** `polygon_area : Tensor<2, Real[L]> -> Real[L^2]`

*Source: Rust documentation of the `aevia-std` function.*
