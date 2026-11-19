# `Ode`

Source: `stdlib/Ode.aev`

The `Ode` module of §8.20.

## `type OdeMethod`

```aevia
type OdeMethod
```

The integration scheme for an ODE solver. Dopri5 is the adaptive default for smooth problems; Rk4 is a fixed-step classical method. Radau, Bdf and Rosenbrock23 are for stiff problems, and AutoSwitch starts with Dopri5 and falls back to Radau. StormerVerlet, Ruth4 and the GaussLegendre and ImplicitMidpoint methods are geometric integrators that preserve the structure of Hamiltonian systems, and the Adaptive* forms vary the step while keeping that method.
## `type OdeOpts`

```aevia
type OdeOpts<s>
```

Options for odeint_with, solve_ivp and ode_solve with a fixed or automatically chosen step: rtol and atol are the relative and absolute error tolerances, method is an OdeMethod, steps is the number of sub-steps for fixed-step methods (or None), and events is an optional function whose sign change stops the integration.
## `type OdeAdaptiveOpts`

```aevia
type OdeAdaptiveOpts<s>
```

Options for adaptive stepping: the same fields as OdeOpts plus the starting step h0 (zero lets the solver choose), the smallest and largest steps h_min and h_max, and limits on the number of steps (max_steps, counting rejected attempts) and right-hand-side evaluations (max_evaluations). Running out of either limit is an error.
## `type OdePhysicalOpts`

```aevia
type OdePhysicalOpts<s>
```

Like OdeAdaptiveOpts, but atol has the same structure and units as the state, so each component gets a tolerance in its own unit instead of a single dimensionless number.
## `type OdeEvent`

```aevia
type OdeEvent<s>
```

An event for ode_solve: g(t, y) is a dimensionless function whose sign change marks the event, terminal says whether the integration stops there, and direction is -1 to react only to falling crossings, 1 for rising crossings and 0 for both.
## `type OdeHit`

```aevia
type OdeHit<s>
```

One event that fired during ode_solve: the index of the event in the list you supplied, the time t it happened and the state y at that time.
## `type OdeSolution`

```aevia
type OdeSolution<s>
```

The result of ode_solve. It holds the accepted times t and states y, a function at(t) that gives the state at any time in the span, the events that fired (events), the number of right-hand-side evaluations (nfev), the number of rejected steps (rejected) and the name of the method that finished the run.
## `let ode_bvp`

```aevia
let ode_bvp = ode_bvp
```

Solves a boundary-value problem by single shooting: ode_bvp(f, y0_guess, (t0, t1), residual, options).

**Signature:** `ode_bvp : (((Float[T], s) -> RateOf<s>), s, (Float[T], Float[T]), ((s, s) -> Tensor<1, Float[d]>), OdeOpts<s>) -> OdeSolution<s>`

It adjusts the initial state until residual(start, end) is zero, and returns the solution. It does not converge for strongly unstable problems.

**Examples:**
```text
let solver = ode_bvp
```

*Source: Aevia standard library guide.*
## `let ode_solve`

```aevia
let ode_solve = ode_solve
```

The full-featured ODE solver: ode_solve(f, y0, (t0, t1), options, events).

**Signature:** `ode_solve : (((Float[T], s) -> RateOf<s>), s, (Float[T], Float[T]), OdeOpts<s>, Tensor<1, OdeEvent<s>>) -> OdeSolution<s>`

**Kernel signature** (derived):
- `ode_solve : (Dyn<term>, Dyn<term>, Dyn<term>, Dyn<term>, Dyn<term>, Dyn<term>, Dyn<term>, Dyn<term>) -> Dyn<term>`
- `ode_solve : (Dyn<term>, Dyn<term>, Dyn<term>, Int, Int, Int, Float, Dyn<term>) -> Tensor<1, Float>`
- `ode_solve : (Tensor<1, Dyn<term>>, Tensor<1, Symbol>, Dyn<term>, Tensor<1, Int>, Int, Float, Float, Dyn<term>) -> Tensor<1, Float>`

**Dimensions:** the result has the dimension of argument 4

It returns a solution record with the accepted times and states, a function at(t) that gives the state at any time in the span, the events that fired, and counts of work done. Each event is a record { g, terminal, direction } where g changes sign when the event happens; a terminal event stops the integration. Use odeint when you only need values at chosen times.

**Examples:**
```text
let solver = ode_solve
```

*Source: Aevia standard library guide.*
## `let odeint`

```aevia
let odeint = odeint
```

Integrates dy/dt = f(t, y) and returns the state at each requested time: odeint(f, y0, times).

**Signature:** `odeint : (((Float[T], s) -> RateOf<s>), s, Tensor<1, Float[T]>) -> Tensor<1, s>`

**Kernel signature** (derived):
- `odeint : (Dyn<term>, Dyn<term>, Dyn<term>, Dyn<term>, Dyn<term>, Dyn<term>) -> Dyn<term>`

**Dimensions:** the result has the dimension of argument 5

The state can be a number, a vector or a record; the function must return a matching record of rates, whose dimension is the state's per unit time. It uses an adaptive Dormand-Prince method with tight tolerances. Use odeint_with to choose the method and tolerances.

**Examples:**
```text
fn decay(t: Float[T], y: Float[1]): Float[1/T] = -y / 1.0 s
odeint(decay, 1.0, linspace(0.0 s, 2.0 s, 5))
```

*Source: Aevia standard library guide.*
## `let odeint_order`

```aevia
let odeint_order = odeint_order
```

Integrates a higher-order equation such as y'' = f(t, y, y').

**Signature:** `odeint_order : (((Float[T], y0, y1, ..., yk) -> RateOf^(k+1)<y0>), (y0, y1, ..., yk), Tensor<1, Float[T]>) -> { t: Tensor<1, Float[T]>, y: Tensor<1, y0>, d: Tensor<1, (y0, y1, ..., yk)> }`

Give the right-hand side and the initial values of y and its derivatives as a tuple; the result holds the times, y at each time, and all derivatives. Each derivative's dimension is that of y per time to the power of its order.

**Examples:**
```text
let solver = odeint_order
```

*Source: Aevia standard library guide.*
## `let odeint_order_with`

```aevia
let odeint_order_with = odeint_order_with
```

Like odeint_order, with an options record choosing the method and tolerances.

**Signature:** `odeint_order_with : (rhs, (y0, y1, ..., yk), Tensor<1, Float[T]>, OdeOpts<y0>) -> { t: Tensor<1, Float[T]>, y: Tensor<1, y0>, d: Tensor<1, (y0, y1, ..., yk)> }`

**Examples:**
```text
let solver = odeint_order_with
```

*Source: Aevia standard library guide.*
## `let odeint_with`

```aevia
let odeint_with = odeint_with
```

Like odeint, with options (method, tolerances, fixed step count, events) in a record.

**Signature:** `odeint_with : (((Float[T], s) -> RateOf<s>), s, Tensor<1, Float[T]>, OdeOpts<s>) -> { t: Tensor<1, Float[T]>, y: Tensor<1, s> }`

It returns the times and states actually reached as { t, y }.

**Examples:**
```text
import Ode { Dopri5 }
fn decay(t: Float[T], y: Float[1]): Float[1/T] = -y / 1.0 s
odeint_with(decay, 1.0, linspace(0.0 s, 2.0 s, 5), { rtol = 0.000001, atol = 0.000000001, method = Dopri5, steps = None, events = None })
```

*Source: Aevia standard library guide.*
## `let solve_ivp`

```aevia
let solve_ivp = solve_ivp
```

Solves an initial value problem over a time span: solve_ivp(f, y0, (t0, t1), options), returning { t, y } at the steps taken.

**Signature:** `solve_ivp : (((Float[T], s) -> RateOf<s>), s, (Float[T], Float[T]), OdeOpts<s>) -> { t: Tensor<1, Float[T]>, y: Tensor<1, s> }`

Options are as for odeint_with and choose the integration method.

**Examples:**
```text
import Ode { Dopri5 }
fn decay(t: Float[T], y: Float[1]): Float[1/T] = -y / 1.0 s
solve_ivp(decay, 1.0, (0.0 s, 2.0 s), { rtol = 0.000001, atol = 0.000000001, method = Dopri5, steps = None, events = None })
```

*Source: Aevia standard library guide.*
