# `Ode`

Source: `stdlib/Ode.aev`

The `Ode` module of §8.20.

## `type OdeMethod`

```aevia
type OdeMethod
```
## `type OdeOpts`

```aevia
type OdeOpts<s>
```
## `type OdeAdaptiveOpts`

```aevia
type OdeAdaptiveOpts<s>
```
## `type OdePhysicalOpts`

```aevia
type OdePhysicalOpts<s>
```
## `type OdeEvent`

```aevia
type OdeEvent<s>
```
## `type OdeHit`

```aevia
type OdeHit<s>
```
## `type OdeSolution`

```aevia
type OdeSolution<s>
```
## `let ode_bvp`

```aevia
let ode_bvp = ode_bvp
```

**Signature:** `ode_bvp : (((Float[T], s) -> RateOf<s>), s, (Float[T], Float[T]), ((s, s) -> Tensor<1, Float[d]>), OdeOpts<s>) -> OdeSolution<s>`
## `let ode_solve`

```aevia
let ode_solve = ode_solve
```

`y(t1)` for `y' = rhs(t, y)` with `y(t0) = y0` using `dopri5`, `rosenbrock23`, `radau5`, `bdf`, `sdirk3`, or `exprb32`.

**Signature:** `ode_solve : (((Float[T], s) -> RateOf<s>), s, (Float[T], Float[T]), OdeOpts<s>, Tensor<1, OdeEvent<s>>) -> OdeSolution<s>`

**Kernel signature** (derived):
- `ode_solve : (Dyn<term>, Dyn<term>, Dyn<term>, Dyn<term>, Dyn<term>, Dyn<term>, Dyn<term>, Dyn<term>) -> Dyn<term>`
- `ode_solve : (Dyn<term>, Dyn<term>, Dyn<term>, Int, Int, Int, Float, Dyn<term>) -> Tensor<1, Float>`
- `ode_solve : (Tensor<1, Dyn<term>>, Tensor<1, Symbol>, Dyn<term>, Tensor<1, Int>, Int, Float, Float, Dyn<term>) -> Tensor<1, Float>`

**Dimensions:** the result has the dimension of argument 4

**Examples:**
- `ode_solve(t, y, t, 0, 0, 2, 0.000000001, dopri5)` returns a `[Float]`
- `ode_solve(-2*y, y, t, 1, 0, 1, 0.000000001, dopri5)` returns a `[Float]`
- `ode_solve(list(y2, -y1), list(y1, y2), t, list(1, 0), 0, 1.5, 0.0000000001, dopri5)` returns a `[Float]`
(recorded kernel calls; the result is shown as its sort)

*Source: rssn operator `numeric.ode_solve`.*
## `let odeint`

```aevia
let odeint = odeint
```

Numerically integrates `dy/dx = f(x, y)` from `(x0, y0)` to `x1`.

**Signature:** `odeint : (((Float[T], s) -> RateOf<s>), s, Tensor<1, Float[T]>) -> Tensor<1, s>`

**Kernel signature** (derived):
- `odeint : (Dyn<term>, Dyn<term>, Dyn<term>, Dyn<term>, Dyn<term>, Dyn<term>) -> Dyn<term>`

**Dimensions:** the result has the dimension of argument 5

*Source: rssn operator `ode.odeint`.*
## `let odeint_order`

```aevia
let odeint_order = odeint_order
```

**Signature:** `odeint_order : (((Float[T], y0, y1, ..., yk) -> RateOf^(k+1)<y0>), (y0, y1, ..., yk), Tensor<1, Float[T]>) -> { t: Tensor<1, Float[T]>, y: Tensor<1, y0>, d: Tensor<1, (y0, y1, ..., yk)> }`
## `let odeint_order_with`

```aevia
let odeint_order_with = odeint_order_with
```

**Signature:** `odeint_order_with : (rhs, (y0, y1, ..., yk), Tensor<1, Float[T]>, OdeOpts<y0>) -> { t: Tensor<1, Float[T]>, y: Tensor<1, y0>, d: Tensor<1, (y0, y1, ..., yk)> }`
## `let odeint_with`

```aevia
let odeint_with = odeint_with
```

**Signature:** `odeint_with : (((Float[T], s) -> RateOf<s>), s, Tensor<1, Float[T]>, OdeOpts<s>) -> { t: Tensor<1, Float[T]>, y: Tensor<1, s> }`
## `let solve_ivp`

```aevia
let solve_ivp = solve_ivp
```

**Signature:** `solve_ivp : (((Float[T], s) -> RateOf<s>), s, (Float[T], Float[T]), OdeOpts<s>) -> { t: Tensor<1, Float[T]>, y: Tensor<1, s> }`
