# 05 — Units

Chapter 04 made dimensions types; this chapter makes units **compile-time scales**. A unit is
not part of a type. It supplies:

1. a dimension `d`;
2. a scale `s` (an exact multiple of the coherent SI unit);
3. an affine offset `o : Real[d]` (affine units only).

The unit literal `3.2 km` folds at compile time to `3200.0` (the coherent SI value) with type
`Float[L]`; `km` does not stay in the value. To display a value in a unit, attach a display
unit with `to(km, x)` (§5.4).

## 5.1 Unit declarations

```aevia
dimension Currency                  # a user-defined base dimension
unit USD : Currency = 1             # base unit
unit EUR : Currency = 1.08 USD      # derived unit

unit m : Length = 1 prefixable      # base unit, SI prefixes allowed
unit km : L = 1000 m                # explicit derived unit
unit N : M*L/T^2 = 1 kg*m/s^2
unit degC : Θ = 1 K offset 273.15 K # affine unit
```

### 5.1.1 Syntax

```
dimension_decl ::= [ "private" ] "dimension" UIDENT [ "=" dim ]
unit_decl      ::= [ "private" ] "unit" unit_name ":" dim "=" unit_rhs [ "prefixable" ]
unit_rhs       ::= unit_scale [ unit_expr ] [ "offset" unit_scale unit_expr ]
unit_scale     ::= number [ "/" number ] | "pi" [ "/" number ]
```

- `dimension` and `unit` are **exported** by default; `private` limits them to the module.
  Exported items enter the manifest and the registry, so downstream packages can `import`
  them. `Units.*` is a generated view of `manifest.units`; user declarations need no change
  to the standard library.
- `unit_name` is an identifier or a special name containing `°` (`°C`, `°F`).
- `unit_scale` is a positive rational or a rational multiple of `pi`. Decimal literals are
  read as **exact decimals** (`0.3048` is `381/1250`), not as `Float`, so conversions such as
  `ft → m` are exact. Unit declarations are evaluated at compile time and admit no run-time
  expression; write a decimal literal of sufficient precision for an approximate scale.
- An omitted `unit_expr` (§5.2) declares a base unit: the scale MUST be 1 and the dimension
  MUST be a base dimension.
- `offset` is for affine units only; the offset's dimension MUST equal the unit's (`E0504`).
- `prefixable` generates the SI-prefixed variants (§5.1.4).

### 5.1.2 Base and derived units

- A unit name MUST be unique; a repeated declaration is `E0505`. A dimension may have several
  units of scale 1; they are equivalent.
- Units are visible in declaration order: using a unit before its declaration is `E0501`, so
  definitions cannot form a cycle.
- A derived scale is computed at compile time as an exact rational. If an exponent leaves the
  scale without an exact value (`3 ft^(1/2)`: the scale of `ft` is not a perfect square), the
  error is `E0506`.
- Unit declarations can be imported across modules; a `private unit` is not exported.

### 5.1.3 Unit namespace

Unit names and value names live in **separate namespaces**:

- In the literal `3 m`, `m` resolves in the unit namespace; `let m = 5` does not change
  `3 m`. To multiply by a variable, write `3 * m`.
- In an ordinary expression a unit name is the value `Unit[d]` (globally visible).
- On a clash, the longest match wins: `ms` is milliseconds, not `m*s`; write `m*s` for
  metre-seconds.
- A unit value cannot be accessed by a qualified name (`Units.SI.km` is invalid); units are globally visible from the kernel table, so use the short name.

Unit names and dimension names are also separate namespaces; one spelling may have a meaning
in each, and the parser decides by position:

| Spelling | As a unit | As a dimension |
|---|---|---|
| `L` | litre (alias of `liter`/`litre`, `prefixable`) | display alias of `Length` |
| `T` | tesla | display alias of `Time` |
| `G` | gauss (`1e-4 T`) | not the gravitational constant |
| `h` | hour | not Planck's constant |

Canonical names are always the full names (`Length`/`Mass`/`Time`/…); single letters are for
source and display only.

### 5.1.4 Prefixes

| Symbol | Factor | Symbol | Factor | Symbol | Factor | Symbol | Factor |
|---|---|---|---|---|---|---|---|
| `Q` quetta | 10^30 | `P` peta | 10^15 | `h` hecto | 10^2 | `µ` micro | 10^-6 |
| `R` ronna | 10^27 | `T` tera | 10^12 | `da` deca | 10^1 | `n` nano | 10^-9 |
| `Y` yotta | 10^24 | `G` giga | 10^9 | `d` deci | 10^-1 | `p` pico | 10^-12 |
| `Z` zetta | 10^21 | `M` mega | 10^6 | `c` centi | 10^-2 | `f` femto | 10^-15 |
| `E` exa | 10^18 | `k` kilo | 10^3 | `m` milli | 10^-3 | `a` atto | 10^-18 |
| | | | | | | `z`/`y`/`r`/`q` | 10^-21/-24/-27/-30 |

- A prefix applies only to a `prefixable` unit. `kg` is the SI base unit but prefixes attach
  to `g` (`1/1000 kg`, `prefixable`); an **exact name match wins over prefix decomposition**,
  so `kg` is the base unit, not `k`+`g`.
- Longest match: `dam` is `da`+`m`, not `d`+`am`.
- Prefixes do not stack: `kkm` is not a unit and is reported as `E0501`.
- Binary prefixes `Ki`, `Mi`, `Gi`, `Ti` apply only to information units (`bit`, `B`).

## 5.2 Unit expressions

```
unit_expr   ::= unit_factor { ("*" | "·" | "/") unit_factor }
unit_factor ::= unit_atom [ "^" exponent ]
unit_atom   ::= unit_name | "(" unit_expr ")" | "%"
```

- `*` and `·` are synonyms; `/` is left-associative. An exponent is an integer literal or a
  parenthesised rational: `m^2`, `s^-1`, `m^(1/2)`.
- Numbers (other than exponents) and juxtaposition by whitespace are not allowed inside a unit
  expression.
- An affine unit may appear only alone; in a product or power it is `E0508` (§5.6.4).

### 5.2.1 Unit values

At the value level a unit expression is a `Unit[d]` value:

```aevia
let speed_unit = km / h          # Unit[L/T], scale 1000/3600
let v = 100 * speed_unit         # Real[L/T], value 250/9
```

`100 speed_unit` is not a unit literal (`speed_unit` is not a unit name) and is `E0501`;
write `100 * speed_unit`. `Unit[d]` supports `u * v : Unit[d*e]`, `u / v : Unit[d/e]` and
`u ^ n : Unit[d^n]` (`n` a literal); units can be compared (same dimension), printed and
passed as arguments. A unit is not a number and cannot be added to a `Real`.

### 5.2.2 Evaluating a unit literal

```
value = number * scale(unit_expr) + offset(unit_expr)
type  = Scalar<rep(number), dim(unit_expr)>
```

- A linear unit has `offset = 0`: `3 km` → `3000`, `Real[L]`.
- An affine unit is legal only alone: `25 °C` → `25 + 273.15 = 298.15`, `Point[Θ]` (§5.6).
- An `Int` number gives `Real` (exact) by default and a `Float` number gives `Float`:
  `3 km : Real[L]`, `3.0 km : Float[L]`. The expected type overrides the representation:
  `let x: Real[L] = 3.0 m` is the exact `3`; in `fall_time(20 m)`, `20 m` is instantiated at
  `Float[L]`.
- A scale containing `pi` may give a symbolic result: `30 deg` → `pi/6 : Real[1]`, or
  `0.5235987755982988` when `Float` is expected.

## 5.3 Unit tables

The standard library `Units.SI` provides the following units (excerpt).

### 5.3.1 SI base units

| Unit | Dimension | Note |
|---|---|---|
| `m` | `L` | metre, prefixable |
| `kg` | `M` | kilogram (`g` is prefixable) |
| `s` | `T` | second, prefixable |
| `A` | `I` | ampere, prefixable |
| `K` | `Θ` | kelvin, prefixable |
| `mol` | `Amt` | mole, prefixable |
| `cd` | `Lum` | candela, prefixable |

### 5.3.2 SI derived units

| Unit | Dimension | Definition |
|---|---|---|
| `Hz` | `1/T` | `1/s` |
| `N` | `M*L/T^2` | `kg*m/s^2` |
| `Pa` | `M/(L*T^2)` | `N/m^2` |
| `J` | `M*L^2/T^2` | `N*m` |
| `W` | `M*L^2/T^3` | `J/s` |
| `C` | `I*T` | `A*s` |
| `V` | `M*L^2/(I*T^3)` | `W/A` |
| `F` | `I^2*T^4/(M*L^2)` | `C/V` |
| `Ω` / `ohm` | `M*L^2/(I^2*T^3)` | `V/A` |
| `S` | `I^2*T^3/(M*L^2)` | `1/Ω` |
| `Wb` | `M*L^2/(I*T^2)` | `V*s` |
| `T` | `M/(I*T^2)` | `Wb/m^2` |
| `H` | `M*L^2/(I^2*T^2)` | `Wb/A` |
| `lm` | `Lum` | `cd*sr` |
| `lx` | `Lum/L^2` | `lm/m^2` |
| `Bq` | `1/T` | `1/s` |
| `Gy`, `Sv` | `L^2/T^2` | `J/kg` |
| `kat` | `Amt/T` | `mol/s` |
| `rad`, `sr` | `1` | dimensionless, scale 1 |

### 5.3.3 Common non-SI units

| Unit | Dimension | Definition |
|---|---|---|
| `min` / `h` / `d` / `week` / `yr` | `T` | `60 s` / `3600 s` / `86400 s` / `604800 s` / `31557600 s` |
| `°` / `deg` | `1` | `pi/180` |
| `′` / `arcmin` | `1` | `pi/10800` |
| `″` / `arcsec` | `1` | `pi/648000` |
| `l` / `liter` | `L^3` | `1/1000 m^3` |
| `t` | `M` | `1000 kg` |
| `eV` | `Energy` | `1.602176634e-19 J` |
| `Da` / `u` | `M` | `1.66053906660e-27 kg` |
| `au` | `L` | `149597870700 m` |
| `pc` | `L` | `3.0856775814913673e16 m` |
| `ly` | `L` | `9460730472580800 m` |
| `Å` / `angstrom` | `L` | `1e-10 m` |
| `bar` / `atm` / `mmHg` / `psi` | `Pressure` | `100000 Pa` / `101325 Pa` / `133.322387415 Pa` / `6894.757293168 Pa` |
| `cal` | `Energy` | `4.184 J` |
| `Wh` | `Energy` | `3600 J` (`kWh` is the prefixed form) |
| `hp` | `Power` | `745.6998715822702 W` |
| `kn` / `mph` | `L/T` | `1852/3600 m/s` / `1609.344/3600 m/s` |
| `ft` / `in` / `yd` / `mi` / `nmi` | `L` | `0.3048` / `0.0254` / `0.9144` / `1609.344` / `1852` `m` |
| `acre` | `L^2` | `4046.8564224 m^2` |
| `gal` | `L^3` | `0.003785411784 m^3` |
| `oz` / `lb` | `M` | `0.028349523125 kg` / `0.45359237 kg` |
| `lbf` | `Force` | `4.4482216152605 N` |
| `°C` | `Θ` | affine: `1 K offset 273.15 K` |
| `°F` | `Θ` | affine: `5/9 K offset 255.3722222222222 K` |
| `bit` / `B` / `KiB` | `1` | information: `1` / `8 bit` / `1024 B` |

Information units are dimensionless because many formulas use bits as probability or entropy.
For a separate `Information` base dimension, declare `dimension Information` (the kernel's
`Config` must declare `base_dimension("Information")`).

### 5.3.4 Unit modules

| Module | Contents |
|---|---|
| `Units.SI` | base units, derived units, prefixes, common non-SI units |
| `Units.Imperial` | imperial length, mass, volume, pressure, temperature |
| `Units.Astronomy` | `au`, `pc`, `ly`, `Rsun`, `Msun`, `Mearth`, `jup` (`Mpc` is `pc` with a prefix) |
| `Units.CGS` | `cm`, `g`, `dyn`, `erg`, `Gal`, `P`, `St`, `G`, `Mx`, `Ba`, `Oe` |
| `Units.Constants` | physical constants (§5.8) |
| `Units.Natural`, `Units.Planck`, `Units.Atomic`, `Units.Geometrized` | constant modules under a unit system (§5.11.1) |

Status: all of these modules can be `open`ed. Their contents are generated from the kernel
manifest (`manifest.units`, `manifest.constants`, `manifest.unit_systems`); every `UnitValue`
carries `aliases`, `prefixable`, `category` and `since`. Units are **session-private**: a unit
added with `Session::register_unit` appears only in that session's manifest.

## 5.4 Conversion and display

### 5.4.1 Three operations

```aevia
let d = 3.2 km              # Float[L], value 3200.0
let e = to(m, d)            # value 3200.0, display unit m
let f = value_in(km, d)     # Float[1], value 3.2
let g = base(d)             # value 3200.0, display unit removed
```

- `to(u, x)`: `x : Scalar<ρ,d>` and `u : Unit[d]`; value and type are unchanged, only a
  display unit is attached.
- `value_in(u, x)` is `x / u`, a dimensionless number.
- `base(x)` removes the display unit and returns the coherent SI value.
- A unit whose dimension differs from the value's is `E0509`.
- **An affine unit accepts only `Point[d]`**: `value_in(°C, point(298.15, K))` is `25`. Using
  an affine unit on a temperature difference (`Scalar<ρ,Θ>`) is `E0510`, so
  `value_in(°C, 298.15 K)` is an error. Display a difference with `delta_to(°C, dT)`, which
  uses the scale and ignores the offset.

Status: `delta_to(°C, 10.0 K)` currently prints `-263.15 °C` (the offset is still applied),
which contradicts the semantics above. This is a known defect.

### 5.4.2 Display-unit propagation

At run time each scalar may carry an optional display unit:

- `x + y`, `x - y`: kept if both sides have the same display unit, otherwise dropped to SI.
- `x * y`, `x / y`: if one operand is dimensionless and carries a display unit, the other
  operand's display is kept; otherwise it is dropped.
- `to` sets it explicitly and `base` clears it; function calls drop it by default (shape-
  preserving functions such as `abs` keep it).

A display unit is **metadata**: it takes no part in equality, typing or serialisation.
`3.2 km == 3200 m` is `true`.

Status: the warning `W0501` (display unit dropped) has no emission site yet.

### 5.4.3 Printing

- With a display unit, the number is converted to that unit (`3.2 km`, `25 °C`).
- Without one, a "best unit" is chosen: an SI derived unit (`N`, `J`, `Pa`) if one fits,
  otherwise a product of base units (`kg*m/s^2`). Fractional exponents print correctly
  (`T^(1/2)` → `s^(1/2)`).
- `show` is guaranteed readable; `repr` is guaranteed to parse back to the same value.
- `Float` printing uses the shortest round-trip representation; an exact `Real` prints as a
  fraction or radical.

## 5.5 Angles and dimensionless units

- `rad` is a dimensionless unit of scale 1; `deg`/`°` has scale `pi/180`.
- `sin(30 deg)` and `sin(30)` (30 radians) are both legal, because `sin` only requires a
  dimensionless argument; the type system does not tell radians from plain numbers.
- `atan2(y, x)` returns dimensionless radians; display degrees with `to(deg, ...)`.

## 5.6 Affine units and `Point`

### 5.6.1 Problem and types

Temperature is used in two ways: an **absolute temperature** (a point of an affine space;
`25 °C` is 298.15 K) and a **temperature difference** (a vector that can be added and scaled).
Conflating them makes `20 °C + 30 °C` come out as a wrong 50 °C or 596.3 K.

```
Point[Θ]        absolute temperature (affine point)
Float[Θ]        temperature difference (vector)
```

- Only affine unit literals produce `Point[d]`: `25 °C`, `77 °F`. A linear unit literal
  produces a difference: `25 K` is a difference.
- Write an absolute temperature in kelvin with `point(298.15, K)`.
- `Point` has only the `Float` representation; a symbolic temperature is a `Real[Θ]`
  difference.
- A `Point` can be a tensor element (`Tensor<[nx,ny], Point[Θ]>` is a temperature field);
  element-wise operations follow the point rules. Average a field with `centroid`, not
  `mean` (§13.9.2).

### 5.6.2 Operations

```
p - q        : Float[d]        two points subtract to a difference
p + x, x + p : Point[d]        point plus difference
p - x        : Point[d]        point minus difference
lerp(p,q,t)  : Point[d]        affine combination, t : Float[1]
p + q, k * p, p / k, p / K   : error E0405
```

- `Point` is comparable (`p == q`, `p < q`); `min` and `max` return a `Point`;
  `centroid : Tensor<1,Point[d]> -> Point[d]`.
- In `p + x` and `p - x`, `x` MUST be `Float[Θ]`. An exact `Real[Θ]` variable does not
  qualify (`E0302`; use `to_float`), but a literal `10 K` is instantiated at the expected
  `Float`, so `25.0 °C + 10 K` is legal.
- `to(°C, p)` sets a display unit, `value_in(°C, p)` returns the Celsius number, and `base(p)`
  returns the kelvin value as `Float[Θ]`.

```
point     : (Float[1], Unit[d]) -> Point[d]
to        : (Unit[d], Scalar<ρ,d>) -> Scalar<ρ,d>      | (Unit[d], Point[d]) -> Point[d]
value_in  : (Unit[d], Scalar<ρ,d>) -> Scalar<ρ,1>      # E0510 for an affine unit
value_in  : (Unit[d], Point[d]) -> Float[1]
base      : Scalar<ρ,d> -> Scalar<ρ,d>                 | Point[d] -> Float[d]
delta_to  : (Unit[d], Scalar<ρ,d>) -> Scalar<ρ,d>      # scale only, no offset
unit_pow_dyn : (Unit[d], Int) -> Dyn<Real>             # run-time exponent
lerp      : (Point[d], Point[d], Float[1]) -> Point[d]
```

`point(25.0, °C)` is 298.15 K; the first argument is the numeric coefficient in that unit.

### 5.6.3 Why kelvin is linear

`K` is a linear unit (scale 1, offset 0), so `300 K` is a difference by type: the kelvin
scale serves both absolute temperatures and differences, and the unit alone cannot tell them
apart. Celsius and Fahrenheit are affine scales, so their literals are naturally points.

```aevia
let T1 = 25.0 °C            # Point[Θ]
let T2 = point(300.0, K)    # Point[Θ]
let dT = T2 - T1            # Float[Θ]
let T3 = T1 + 10.0 K        # Point[Θ]
```

### 5.6.4 Affine units in unit expressions

`°C/s` and `°C*m` are meaningless and the parser rejects them (`E0508`). `°C` may appear
only alone as a literal unit, or as an argument of `to`/`value_in`.

## 5.7 Implementation of conversion

- A scale is folded at compile time into an exact rational (or a symbolic value containing
  `pi`); `3 km` directly yields `3000`.
- At run time only the display unit's name and conversion data remain: `to(u, x)` sets the
  display unit and printing uses `x / u.scale`. Affine conversion is
  `value_in(°C, p) = (p - 273.15 K) / 1 K`.
- Unit equality: same dimension and same scale (and, for affine units, same offset). `m` and
  `100 cm` are different unit values of the same dimension; `to` converts between them.

## 5.8 Physical constants

`Units.Constants` is generated from `manifest.constants`; symbols are identical to RSSN's:

| Kind | Constants (SI unit) |
|---|---|
| exact | `c` (`m/s`), `g_n` (`m/s^2`), `h`, `hbar` (`J*s`), `k_B` (`J/K`), `N_A` (`1/mol`), `e` (`C`), `F` (`C/mol`), `R` (`J/(mol*K)`), `M_u` (`kg/mol`), `T_tp` (`K`), `atm` (`Pa`), `G_0` (`S`), `K_J` (`Hz/V`), `R_K` (`ohm`), `Phi_0` (`Wb`), `K_cd` (`lm/W`), `dnu_Cs` (`Hz`) |
| measured | `G` (`m^3/(kg*s^2)`), `alpha` (`1`), `R_inf` (`1/m`), `a_0` (`m`), `epsilon_0` (`F/m`), `mu_0` (`N/A^2`), `mu_B` (`J/T`), `m_e`, `m_p`, `m_n`, `m_u` (`kg`), `sigma_SB` (`W/(m^2*K^4)`) |

- An exact constant is a defining value or an exact derivation of one and has no
  `uncertainty`; a measured constant is a CODATA 2022 value with a standard uncertainty. Both
  are exact rationals in the manifest (`ScaleValue`); `exact` means "defining value", not a
  storage type.
- A constant is not a unit: it has a dimension and takes part in arithmetic, but cannot appear
  in a unit position. The core language carries no uncertainty; use `Measurement` (§8.14) when
  needed.
- `pi`, `e` (Euler's number) and `i` live in `Math`. `Units.Constants.e` is the **elementary
  charge** and shares its name with `Math.e`; qualify it when both are open.

Metadata of each entry:

```
type ConstantInfo = {
    name: String,              # "speed of light in vacuum"
    symbol: String,            # "c"
    value: Scalar<Real, d>,    # coherent SI value
    dimension: Dim,
    unit: String,              # "m/s"
    exact: Bool,
    uncertainty: Option<Scalar<Float, d>>,
    source: String,
}
```

## 5.9 Unit errors

| Code | Situation |
|---|---|
| `E0501` | unknown unit name (including a stacked prefix such as `kkm`, or use before declaration) |
| `E0502` | unit not imported/opened (reserved) |
| `E0503` | unit cannot take a prefix (reserved) |
| `E0504` | affine offset dimension mismatch |
| `E0505` | duplicate unit name |
| `E0506` | scale has no exact value for the exponent |
| `E0507` | prefixed unit conflicts with an explicit unit (reserved) |
| `E0508` | affine unit in a product or power |
| `E0509` | unit dimension of `to`/`value_in` does not match the value |
| `E0510` | affine display of a temperature difference |
| `E0511`/`E0512` | unit-system errors (§5.11) |

Status: `E0502`, `E0503` and `E0507` are reserved and currently unreachable. The unit table is
the whole kernel table plus local declarations and has no module-level unit namespace, so the
states "not imported", "prefix forbidden" and "prefix/explicit conflict" do not exist; the
compiler does not substitute another error for them.

## 5.10 Relation to rssn's unit layer

1. **Single source of truth.** `Units.*` is a generated view of `manifest.units`; user
   `dimension`/`unit` declarations register in the same registry and flow back to the manifest.
   `UnitRegistry` serves only run-time services such as `resolve` and `format`.
2. **The term graph carries no units.** `Real`/`Float` values are coherent SI numbers on the
   host side. Only for rssn typesetting or export is a value wrapped with
   `quantity(v, "<coherent SI unit>")`; it never enters `compute`.
3. **Affine units are resolved in the bridge.** A `Point` becomes a kelvin `Float` before
   reaching rssn; `to(°C, ...)` happens only in the display layer.
4. **Registry round trip.** Only for purely rational exponents is the exact round trip
   `DimTerm → Dimension → DimTerm` via `Dimension::exact_factors` supported; symbolic
   exponents do not round-trip and `Dimension.factors` text MUST NOT be parsed.
   `UnitDef → scale/offset → UnitDef` MUST round-trip exactly.
5. **`DimensionMode::Strict` is for FFI/export only.** The static path's term graph is
   dimensionless and uses `Off`; the dimension of a `Dyn<ρ>` is held by the host as a
   `DimTerm`.

## 5.11 Unit systems

Physicists often set some constants to 1 (`c = 1`, `ħ = 1`). Aevia does this as a
**module-level dimension reduction**: dimensions *written* in the module are simplified by the
unit system, while every value remains an honest SI quantity.

### 5.11.1 Selecting a unit system

The module header selects one by identifier:

```aevia
module Hep with units natural
```

The kernel provides four systems (`manifest.unit_systems`); any other name in the header is
`E0512`, and the diagnostic lists the available names.

| Name | Quantities set to 1 | Generator dimensions left after reduction |
|---|---|---|
| `natural` | `c`, `hbar`, `k_B`, `epsilon_0` (Heaviside–Lorentz) | energy, amount of substance, luminous intensity |
| `planck` | `c`, `hbar`, `G`, `k_B`, `4 pi epsilon_0` | amount of substance, luminous intensity |
| `atomic` | `hbar`, `m_e`, `e`, `4 pi epsilon_0` | temperature, amount of substance, luminous intensity |
| `geometrized` | `c`, `G` | length, current, temperature, amount of substance, luminous intensity |

The constant modules (`Units.Natural` and so on) declare `with units <system>` themselves;
`c`, `hbar` and `k_B` are exactly `1` in `Units.Natural` as a result of the reduction.

### 5.11.2 Reduction

Let the dimensions of the quantities set to 1 be `q₁ … q_k` and the generator dimensions
`g₁ … g_m`; together they form a rational basis of the seven SI base dimensions. Every SI
dimension `d` decomposes uniquely as

```
d = Π qᵢ^λᵢ · Π gⱼ^μⱼ
```

`R(d) = Π gⱼ^μⱼ` is the **representative** of `d`. The **system value** of a quantity `x` of
dimension `d` is `x / Π Qᵢ^λᵢ` (`Qᵢ` the exact SI value), an SI quantity of dimension `R(d)`.
Example: in `natural`, `R(L) = R(T) = Energy^-1`, and the system value of `1 m` is
`1 m / (ħ c) ≈ 5.07e24 J⁻¹`. User base dimensions and dimension variables lie outside the space
spanned by the SI bases and are left unchanged.

In a module with a unit system, **the dimension annotations and unit literals written in that
module** are reduced before checking, so the parameters of `fn f(x: Real[L])` and
`fn f(x: Real[T])` have the same type in `natural`.

### 5.11.3 Values are not reinterpreted

Reduction changes only the dimensions *written*, never a value: every quantity is still an SI
quantity whose type is written with its representative dimension. Values cross the boundary of
a module with a unit system without conversion; **an imported binding keeps the SI type it has
in its home module**.

### 5.11.4 Units without an exact scale

Reducing a unit literal needs an exact scale. If some exponent is not an integer the scale is
irrational; the unit has no exact scale in that system and the error is `E0511` (typically `C`
in `natural`, which needs `(ε₀ħc)^(1/2)`). Write the number in system units, or convert an SI
value with `to_system` (§5.11.5).

### 5.11.5 Explicit conversion

```aevia
to_system("natural", x)           # SI quantity x → system value of dimension R(d)
from_system<L>("natural", y)      # system value y → quantity of SI dimension L
```

- The system name MUST be a string literal, otherwise `E0302`; an unknown name is `E0512`; an
  argument count other than 2 is `E0315`.
- `to_system` requires the argument's dimension to be known at check time, otherwise `E0304`;
  the result has the same representation and dimension `R(d)`.
- `from_system<D>` MUST name the target SI dimension `D` (reduction forgets it: `L` and `T` are
  both `Energy^-1`), otherwise `E0304`; `D` is always read as SI. The argument's dimension MUST
  equal `R(D)`, otherwise it is reported as an ordinary dimension mismatch.
- The result keeps the argument's representation: `Float`/`Complex` multiply by a floating
  factor; exact `Int`/`Real` stay exact rationals when the factor is an exact scale without
  `π`, and otherwise become a symbolic term; they never silently become floats.

### 5.11.6 Display

In a module with a unit system, a quantity with no display unit of its own is shown in the
system's generator dimensions: an inverse energy in `natural` shows as `eV^-1`, not
`s^2/(m^2*kg)`. This selects the display unit only; the value is unchanged. Records, variants
and tuples are displayed field by field.

The next chapter defines evaluation: when a `Real` is an exact number, a symbolic term or a
`Float`, and how tensors and IO run.
