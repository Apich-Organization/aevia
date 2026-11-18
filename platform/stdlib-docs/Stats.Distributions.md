# `Stats.Distributions`

Source: `stdlib/Stats/Distributions.aev`

The `Stats/Distributions` module of §8.20.

## `type Normal`

```aevia
type Normal<d>
```
## `type Uniform`

```aevia
type Uniform<d>
```
## `type Exponential`

```aevia
type Exponential<d>
```
## `type Gamma`

```aevia
type Gamma<d>
```
## `type Beta`

```aevia
type Beta
```
## `type ChiSquared`

```aevia
type ChiSquared
```
## `type StudentT`

```aevia
type StudentT
```
## `type Poisson`

```aevia
type Poisson
```
## `type Binomial`

```aevia
type Binomial
```
## `type LogNormal`

```aevia
type LogNormal
```
## `type Weibull`

```aevia
type Weibull<d>
```
## `type Cauchy`

```aevia
type Cauchy<d>
```
## `type F`

```aevia
type F
```
## `type NegativeBinomial`

```aevia
type NegativeBinomial
```
## `type Geometric`

```aevia
type Geometric
```
## `type Distribution`

```aevia
type Distribution<d>
```
## `let cdf`

```aevia
let cdf = cdf
```

Cumulative distribution function `P(X <= x)` of distribution `d` at point `x`.

**Signature:** `cdf : (Distribution<d>, Real[d]) -> Real[1]`

**Kernel signature** (derived):
- `cdf : (Dyn<term>, Dyn<term>) -> Dyn<term>`

**Dimensions:** kernel rule `Custom("")`

*Source: rssn operator `stats.cdf`.*
## `let pdf`

```aevia
let pdf = pdf
```

Probability density function (or probability mass function) of distribution `d` at point `x`.

**Signature:** `pdf : (Distribution<d>, Real[d]) -> Real[1/d]`

**Kernel signature** (derived):
- `pdf : (Dyn<term>, Dyn<term>) -> Dyn<term>`

**Dimensions:** kernel rule `Custom("")`

*Source: rssn operator `stats.pdf`.*
## `let sample`

```aevia
let sample = sample
```

**Signature:** `sample : Distribution<d> -> IO Real[d]`
## `let sample_n`

```aevia
let sample_n = sample_n
```

**Signature:** `sample_n : (Distribution<d>, Int, Option<Int>) -> Tensor<1, Real[d]>`
