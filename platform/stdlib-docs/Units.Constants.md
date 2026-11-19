# `Units.Constants`

Source: `stdlib/Units/Constants.aev`

Physical constants in SI units. Each entry names its reference and
says whether the value is exact or measured.

Use `import Units.Constants as C` to refer to the speed of light as
`C.c` and the reduced Planck constant as `C.hbar`.

## `let f`

```aevia
let f: Real[Current*Time/Amount]
```

Faraday constant (SI 2019 definition, exact)
## `let g`

```aevia
let g: Real[Length^3/(Mass*Time^2)]
```

Newtonian constant of gravitation (CODATA 2022, measured)
## `let g_0`

```aevia
let g_0: Real[Current^2*Time^3/(Length^2*Mass)]
```

conductance quantum (SI 2019 definition, exact)
## `let k_J`

```aevia
let k_J: Real[Current*Time^2/(Length^2*Mass)]
```

Josephson constant (SI 2019 definition, exact)
## `let k_cd`

```aevia
let k_cd: Real[Luminosity*Time^3/(Length^2*Mass)]
```

luminous efficacy (SI 2019 definition, exact)
## `let m_u`

```aevia
let m_u: Real[Mass/Amount]
```

molar mass constant (SI 2019 definition, exact)
## `let n_A`

```aevia
let n_A: Real[1/Amount]
```

Avogadro constant (SI 2019 definition, exact)
## `let phi_0`

```aevia
let phi_0: Real[Length^2*Mass/(Current*Time^2)]
```

magnetic flux quantum (SI 2019 definition, exact)
## `let r`

```aevia
let r: Real[Length^2*Mass/(Amount*Temperature*Time^2)]
```

molar gas constant (SI 2019 definition, exact)
## `let r_K`

```aevia
let r_K: Real[Length^2*Mass/(Current^2*Time^3)]
```

von Klitzing constant (SI 2019 definition, exact)
## `let r_inf`

```aevia
let r_inf: Real[1/Length]
```

Rydberg constant (CODATA 2022, measured)
## `let t_tp`

```aevia
let t_tp: Real[Temperature]
```

triple point of water (CGPM 1954, exact)
## `let a_0`

```aevia
let a_0: Real[Length]
```

Bohr radius (CODATA 2022, measured)
## `let alpha`

```aevia
let alpha: Real
```

fine-structure constant (CODATA 2022, measured)
## `let atm`

```aevia
let atm: Real[Mass/(Length*Time^2)]
```

standard atmosphere (CGPM 1954, exact)
## `let c`

```aevia
let c: Real[Length/Time]
```

speed of light in vacuum (SI 2019 definition, exact)
## `let dnu_Cs`

```aevia
let dnu_Cs: Real[1/Time]
```

caesium hyperfine frequency (SI 2019 definition, exact)
## `let e`

```aevia
let e: Real[Current*Time]
```

elementary charge (SI 2019 definition, exact)
## `let epsilon_0`

```aevia
let epsilon_0: Real[Current^2*Time^4/(Length^3*Mass)]
```

vacuum electric permittivity (CODATA 2022, measured)
## `let g_n`

```aevia
let g_n: Real[Length/Time^2]
```

standard acceleration of gravity (CGPM 1901, exact)
## `let h`

```aevia
let h: Real[Length^2*Mass/Time]
```

Planck constant (SI 2019 definition, exact)
## `let hbar`

```aevia
let hbar: Real[Length^2*Mass/Time]
```

reduced Planck constant (SI 2019 definition, exact)
## `let k_B`

```aevia
let k_B: Real[Length^2*Mass/(Temperature*Time^2)]
```

Boltzmann constant (SI 2019 definition, exact)
## `let m_e`

```aevia
let m_e: Real[Mass]
```

electron mass (CODATA 2022, measured)
## `let m_n`

```aevia
let m_n: Real[Mass]
```

neutron mass (CODATA 2022, measured)
## `let m_p`

```aevia
let m_p: Real[Mass]
```

proton mass (CODATA 2022, measured)
## `let m_u_2`

```aevia
let m_u_2: Real[Mass]
```

atomic mass constant (CODATA 2022, measured)
## `let mu_0`

```aevia
let mu_0: Real[Length*Mass/(Current^2*Time^2)]
```

vacuum magnetic permeability (CODATA 2022, measured)
## `let mu_B`

```aevia
let mu_B: Real[Current*Length^2]
```

Bohr magneton (CODATA 2022, measured)
## `let sigma_SB`

```aevia
let sigma_SB: Real[Mass/(Temperature^4*Time^3)]
```

Stefan-Boltzmann constant (CODATA 2022, measured)
