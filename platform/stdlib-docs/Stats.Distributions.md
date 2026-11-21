# `Stats.Distributions`

Source: `stdlib/Stats/Distributions.aev`

The `Stats/Distributions` module of §8.20.

## `type Normal`

```aevia
type Normal<d>
```

The normal (Gaussian) distribution with mean mu and standard deviation sigma, both in the dimension of the variable.
## `type Uniform`

```aevia
type Uniform<d>
```

The continuous uniform distribution on the interval from lo to hi, both in the dimension of the variable.
## `type Exponential`

```aevia
type Exponential<d>
```

The exponential distribution with the given rate, whose dimension is the reciprocal of the variable's. Its mean is 1/rate.
## `type Gamma`

```aevia
type Gamma<d>
```

The gamma distribution with a dimensionless shape and a rate whose dimension is the reciprocal of the variable's.
## `type Beta`

```aevia
type Beta
```

The beta distribution on the interval from 0 to 1 with dimensionless shape parameters alpha and beta.
## `type ChiSquared`

```aevia
type ChiSquared
```

The chi-squared distribution with df degrees of freedom.
## `type StudentT`

```aevia
type StudentT
```

Student's t distribution with df degrees of freedom.
## `type Poisson`

```aevia
type Poisson
```

The Poisson distribution of counts with mean lambda.
## `type Binomial`

```aevia
type Binomial
```

The binomial distribution: the number of successes in n independent trials that each succeed with probability p.
## `type LogNormal`

```aevia
type LogNormal
```

The log-normal distribution: the variable's logarithm is normal with mean mu and standard deviation sigma, both dimensionless.
## `type Weibull`

```aevia
type Weibull<d>
```

The Weibull distribution with a dimensionless shape and a scale in the dimension of the variable.
## `type Cauchy`

```aevia
type Cauchy<d>
```

The Cauchy distribution with location loc and scale, both in the dimension of the variable. It has no mean or variance.
## `type F`

```aevia
type F
```

The F distribution with df1 and df2 degrees of freedom, used for ratios of variances.
## `type NegativeBinomial`

```aevia
type NegativeBinomial
```

The negative binomial distribution: the number of failures before r successes, each trial succeeding with probability p. Counts start at 0.
## `type Geometric`

```aevia
type Geometric
```

The geometric distribution: the number of failures before the first success, each trial succeeding with probability p. Counts start at 0.
## `type Distribution`

```aevia
type Distribution<d>
```

A probability distribution, built with one of its constructors, for example Normal({ mu = 0.0, sigma = 1.0 }). Pass it to pdf, cdf, cdf_batch, dist_quantile, sample or sample_n. Parameters carry dimensions, so a Normal of lengths has mu and sigma in metres and its density is per metre.
## `let cdf`

```aevia
let cdf = cdf
```

cdf(dist, x) is the probability that a draw from the distribution is at most x: a dimensionless number between 0 and 1.

**Signature:** `cdf : (Distribution<d>, Real[d]) -> Real[1]`

**Kernel signature** (derived):
- `cdf : (Dyn<term>, Dyn<term>) -> Dyn<term>`

**Dimensions:** kernel rule `CompatibleDimensionless`

**Examples:**
```text
import Stats.Distributions { Normal }
cdf(Normal({ mu = 0.0, sigma = 1.0 }), 0.0)
```

*Source: Aevia standard library guide.*
## `let cdf_batch`

```aevia
let cdf_batch = cdf_batch
```

Evaluates cumulative probabilities for a vector of points in the distribution's dimension.

**Signature:** `cdf_batch : (Distribution<d>, Tensor<1, Real[d]>) -> Tensor<1, Real[1]>`

The results are dimensionless and retain point order and repetitions. A valid distribution and an empty input give an empty result. Invalid parameters or a NaN point raise a domain error.

**Examples:**
```text
import Stats.Distributions { cdf_batch, Normal }
cdf_batch(Normal({ mu = 0.0, sigma = 1.0 }), [-1.0, 0.0, 1.0])
```

*Source: Aevia standard library guide.*
## `let dist_quantile`

```aevia
let dist_quantile = dist_quantile
```

Returns the quantile of a distribution at a dimensionless probability in [0, 1].

**Signature:** `dist_quantile : (Distribution<d>, Real[1]) -> Real[d]`

The result has the distribution's dimension. At probabilities 0 and 1 it returns the lower and upper support endpoints, which can be infinite. Invalid parameters and probabilities raise a domain error.

**Examples:**
```text
import Stats.Distributions { Normal, dist_quantile }
dist_quantile(Normal({ mu = 2.0 m, sigma = 0.5 m }), 0.975)
```

*Source: Aevia standard library guide.*
## `let pdf`

```aevia
let pdf = pdf
```

pdf(dist, x) is the probability density (or mass, for discrete distributions) of a distribution at x.

**Signature:** `pdf : (Distribution<d>, Real[d]) -> Real[1/d]`

**Kernel signature** (derived):
- `pdf : (Dyn<term>, Dyn<term>) -> Dyn<term>`

**Dimensions:** kernel rule `Custom("")`

Its dimension is the reciprocal of x's dimension. Distributions are built with constructors such as Normal and Uniform from Stats.Distributions.

**Examples:**
```text
import Stats.Distributions { Normal }
pdf(Normal({ mu = 0.0, sigma = 1.0 }), 0.0)
```

*Source: Aevia standard library guide.*
## `let sample`

```aevia
let sample = sample
```

Draws one random value from a distribution.

**Signature:** `sample : Distribution<d> -> IO Real[d]`

It is an IO action because each call gives a different result; for a reproducible sequence use sample_n with a seed.

**Examples:**
```text
import Stats.Distributions { Normal }
sample(Normal({ mu = 0.0, sigma = 1.0 }))
```

*Source: Aevia standard library guide.*
## `let sample_n`

```aevia
let sample_n = sample_n
```

sample_n(dist, n, seed) draws n values.

**Signature:** `sample_n : (Distribution<d>, Int, Option<Int>) -> Tensor<1, Real[d]>`

With Some(seed) the result is reproducible; with None it is random.

**Examples:**
```text
import Stats.Distributions { Normal }
sample_n(Normal({ mu = 0.0, sigma = 1.0 }), 5, Some(42))
```

*Source: Aevia standard library guide.*
