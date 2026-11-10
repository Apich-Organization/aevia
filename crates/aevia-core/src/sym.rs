//! A small symbolic term language for `Real`.
//!
//! The specification puts heavy symbolic work in rssn; this module is the
//! self-contained subset the interpreter needs so that `symbol`, `diff`,
//! `subst` and `approx` work without a backend. Terms are hash-free trees with
//! eager constant folding and like-term collection: enough for the examples of
//! chapter 6 (`df(2) == 9`, `x^3 - 3x + 1`, `sqrt(2)`), not a computer algebra
//! system.
//!
//! Everything here is pure and cheap to clone by `Rc` at the value layer.

use crate::eval::EvalError;
use aevia_dim::format_rational_decimal;
use aevia_rssn::Sym;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::One;
use num_traits::Signed;
use num_traits::ToPrimitive;
use num_traits::Zero;
use std::collections::HashMap;
use std::fmt;
use std::hash::Hash;
use std::hash::Hasher;
use std::rc::Rc;

/// A structural hash of a term, without allocating a key string.
///
/// `Term::add`/`Term::mul` used to key their operand groups by
/// `format!("{term:?}")`: one `String` allocation per operand and a linear
/// string comparison per probe, which is O(n²) allocations and time to build an
/// n-operand sum. A structural hash plus `Term`'s own `Eq` is the same
/// equivalence relation over a `u64` and an in-place comparison, so the
/// canonical printing order is unchanged.
#[must_use]
fn term_hash(term: &Term) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    term.hash(&mut hasher);
    hasher.finish()
}

impl Hash for Term {
    fn hash<H: Hasher>(
        &self,
        state: &mut H,
    ) {
        match self {
            | Self::Num(value) => {
                0u8.hash(state);
                value.hash(state);
            },
            | Self::Sym(name) => {
                1u8.hash(state);
                name.hash(state);
            },
            | Self::Add(items) => {
                2u8.hash(state);
                items.hash(state);
            },
            | Self::Mul(items) => {
                3u8.hash(state);
                items.hash(state);
            },
            | Self::Pow(base, exponent) => {
                4u8.hash(state);
                base.hash(state);
                exponent.hash(state);
            },
            | Self::Fun(function, args) => {
                5u8.hash(state);
                function.as_str().hash(state);
                args.hash(state);
            },
            | Self::Foreign { sym, text } => {
                6u8.hash(state);
                sym.hash(state);
                text.hash(state);
            },
        }
    }
}

/// Moves the operands of a sum into `flat`, consuming the term instead of
/// cloning it (the clone was the other O(n) allocation per add).
fn flatten_add_owned(
    term: Term,
    flat: &mut Vec<Term>,
) {
    match term {
        | Term::Add(items) => {
            for item in items {
                flatten_add_owned(item, flat);
            }
        },
        // `c * (a + b)` with a numeric `c` is distributed inside a sum so that
        // `(x + 1) - (x + 1)` cancels and `a - (x + 1)` collects with `x`.
        | Term::Mul(items)
            if items.len() == 2
                && matches!(items[0], Term::Num(_))
                && matches!(items[1], Term::Add(_)) =>
        {
            let mut items = items.into_iter();
            let (Some(Term::Num(coefficient)), Some(Term::Add(terms))) =
                (items.next(), items.next())
            else {
                unreachable!("shape checked above")
            };
            for item in terms {
                flatten_add_owned(Term::mul(Term::Num(coefficient.clone()), item), flat);
            }
        },
        | other => flat.push(other),
    }
}

/// Splits a summand into its rational coefficient and the rest:
/// `3*x*y` is `(3, x*y)`, `x` is `(1, x)`.
fn split_coefficient(term: Term) -> (BigRational, Term) {
    match term {
        | Term::Mul(mut items) if matches!(items.first(), Some(Term::Num(_))) => {
            let Term::Num(coefficient) = items.remove(0) else {
                unreachable!("checked above")
            };
            let rest = if items.len() == 1 {
                items.remove(0)
            } else {
                Term::Mul(items)
            };
            (coefficient, rest)
        },
        | other => (BigRational::one(), other),
    }
}

/// Moves the factors of a product into `flat`, consuming the term.
fn flatten_mul_owned(
    term: Term,
    flat: &mut Vec<Term>,
) {
    match term {
        | Term::Mul(items) => {
            for item in items {
                flatten_mul_owned(item, flat);
            }
        },
        | other => flat.push(other),
    }
}

/// A symbolic expression.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Term {
    /// An exact rational constant.
    Num(BigRational),
    /// A free symbol.
    Sym(String),
    /// A sum, always flattened and with constants folded.
    Add(Vec<Term>),
    /// A product, always flattened and with constants folded.
    Mul(Vec<Term>),
    /// A power with a rational exponent.
    Pow(Box<Term>, BigRational),
    /// A named elementary function.
    Fun(Fun, Vec<Term>),
    /// A term that lives in the rssn kernel.
    ///
    /// Heavy requests (`integral`, `solve`, `limit`, ...) answer with terms
    /// this module has no node for: `erf`, `RootOf`, `Sum`. Rather than
    /// pretend otherwise, the result is carried by handle and the kernel's
    /// own rendering is cached so that display never calls back.
    Foreign {
        /// The kernel handle.
        sym: Sym,
        /// The kernel's rendering.
        text: Rc<str>,
    },
}

/// The elementary functions the symbolic layer understands.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fun {
    /// Square root.
    Sqrt,
    /// Exponential.
    Exp,
    /// Natural logarithm.
    Ln,
    /// Sine.
    Sin,
    /// Cosine.
    Cos,
    /// Tangent.
    Tan,
    /// Absolute value.
    Abs,
    /// A conditional term, `ite(c, a, b)`.
    ///
    /// `by_cases` builds one when the proposition cannot be decided (§8.1.2);
    /// it is the only way to keep a piecewise definition symbolic, because
    /// `if` requires a concrete `Bool` (§6.13).
    Ite,
    /// An elementary function the term language has no rule for.
    ///
    /// `sinh(1)` is still a symbolic `Real` (§6.3.1); refusing to build a node
    /// for it would turn a defined operation into `E1001`. Differentiation
    /// reports the gap instead.
    Other(&'static str),
}

impl Fun {
    /// The source spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            | Self::Sqrt => "sqrt",
            | Self::Exp => "exp",
            | Self::Ln => "ln",
            | Self::Sin => "sin",
            | Self::Cos => "cos",
            | Self::Tan => "tan",
            | Self::Abs => "abs",
            | Self::Ite => "ite",
            | Self::Other(name) => name,
        }
    }

    /// The numeric value, when it is defined.
    #[must_use]
    pub fn eval(
        self,
        x: f64,
    ) -> f64 {
        match self {
            | Self::Sqrt => x.sqrt(),
            | Self::Exp => x.exp(),
            | Self::Ln => x.ln(),
            | Self::Sin => x.sin(),
            | Self::Cos => x.cos(),
            | Self::Tan => x.tan(),
            | Self::Abs => x.abs(),
            // `ite` is not a numeric function of one argument; it is only
            // folded by `Term::fun` when the condition is a number.
            | Self::Ite => f64::NAN,
            | Self::Other(_) => f64::NAN,
        }
    }
}

fn zero() -> Term {
    Term::Num(BigRational::zero())
}

fn one() -> Term {
    Term::Num(BigRational::one())
}

fn rational(value: BigRational) -> Term {
    Term::Num(value)
}

impl Term {
    /// Builds a constant from an integer.
    #[must_use]
    pub fn int(value: i64) -> Self {
        Term::Num(BigRational::from_integer(BigInt::from(value)))
    }

    /// Wraps a kernel term.
    #[must_use]
    pub fn foreign(
        sym: Sym,
        text: impl Into<Rc<str>>,
    ) -> Self {
        Self::Foreign {
            sym,
            text: text.into(),
        }
    }

    /// The kernel handle, when this term lives there.
    #[must_use]
    pub fn as_foreign(&self) -> Option<Sym> {
        match self {
            | Self::Foreign { sym, .. } => Some(*sym),
            | _ => None,
        }
    }

    /// True when the term has no free symbols.
    #[must_use]
    pub fn is_constant(&self) -> bool {
        match self {
            | Self::Num(_) => true,
            | Self::Sym(_) => false,
            | Self::Add(items) | Self::Mul(items) => items.iter().all(Self::is_constant),
            | Self::Pow(base, _) => base.is_constant(),
            | Self::Fun(_, args) => args.iter().all(Self::is_constant),
            | Self::Foreign { .. } => self.free_symbols().is_empty(),
        }
    }

    /// The free symbols, in first-appearance order.
    ///
    /// # Errors
    ///
    /// A kernel handle that no longer names a live term is `E0899`; treating
    /// it as "no free symbols" would make a stale handle look like a closed
    /// form and let `fold` answer with a number.
    pub fn try_free_symbols(&self) -> Result<Vec<String>, EvalError> {
        let mut out = Vec::new();
        self.collect_symbols(&mut out)?;
        Ok(out)
    }

    /// The free symbols, empty when a kernel handle cannot be read.
    ///
    /// Display path only; anything that decides a value uses
    /// [`Term::try_free_symbols`].
    #[must_use]
    pub fn free_symbols(&self) -> Vec<String> {
        self.try_free_symbols().unwrap_or_default()
    }

    fn collect_symbols(
        &self,
        out: &mut Vec<String>,
    ) -> Result<(), EvalError> {
        match self {
            | Self::Num(_) => {},
            | Self::Sym(name) => {
                if !out.contains(name) {
                    out.push(name.clone());
                }
            },
            | Self::Add(items) | Self::Mul(items) => {
                for item in items {
                    item.collect_symbols(out)?;
                }
            },
            | Self::Pow(base, _) => base.collect_symbols(out)?,
            | Self::Fun(_, args) => {
                for arg in args {
                    arg.collect_symbols(out)?;
                }
            },
            | Self::Foreign { sym, .. } => {
                let names = aevia_rssn::kernel()
                    .try_free_symbols(*sym)
                    .map_err(|failure| crate::kernel::error(&failure))?;
                for name in names {
                    if !out.contains(&name) {
                        out.push(name);
                    }
                }
            },
        }
        Ok(())
    }

    /// Adds two terms, folding constants and collecting like terms.
    #[must_use]
    pub fn add(
        left: Term,
        right: Term,
    ) -> Term {
        let mut flat = Vec::new();
        flatten_add_owned(left, &mut flat);
        flatten_add_owned(right, &mut flat);
        let mut constant = BigRational::zero();
        // `(hash, rest, coefficient)` in first-occurrence order: a structural
        // hash replaces the old `format!("{term:?}")` key (see [`term_hash`]).
        // Each summand is split into a rational coefficient and a monomial
        // rest, so `2*x + 3*x` is `5*x` and `x - x` is `0`.
        let mut groups: Vec<(u64, Term, BigRational)> = Vec::new();
        for term in flat {
            if let Term::Num(value) = &term {
                constant += value;
                continue;
            }
            let (coefficient, rest) = split_coefficient(term);
            let hash = term_hash(&rest);
            match groups
                .iter_mut()
                .find(|(existing_hash, existing, _)| *existing_hash == hash && *existing == rest)
            {
                | Some((_, _, total)) => *total += coefficient,
                | None => groups.push((hash, rest, coefficient)),
            }
        }
        let mut items = Vec::with_capacity(groups.len() + 1);
        for (_, rest, coefficient) in groups {
            if coefficient.is_zero() {
                continue;
            }
            if coefficient.is_one() {
                items.push(rest);
            } else {
                match rest {
                    | Term::Mul(mut factors) => {
                        factors.insert(0, rational(coefficient));
                        items.push(Term::Mul(factors));
                    },
                    | rest => items.push(Term::Mul(vec![rational(coefficient), rest])),
                }
            }
        }
        // The constant goes last so that `x^3 - 3x + 1` reads in the usual
        // order rather than starting with the constant term.
        if !constant.is_zero() {
            items.push(rational(constant));
        }
        match items.len() {
            | 0 => zero(),
            | 1 => items.remove(0),
            | _ => Term::Add(items),
        }
    }

    /// Multiplies two terms, folding constants and merging equal bases.
    #[must_use]
    pub fn mul(
        left: Term,
        right: Term,
    ) -> Term {
        let mut flat = Vec::new();
        flatten_mul_owned(left, &mut flat);
        flatten_mul_owned(right, &mut flat);
        let mut constant = BigRational::one();
        let mut factors: Vec<(u64, Term, BigRational)> = Vec::new();
        // `pi` reaches a product in two spellings — the prelude's kernel
        // handle and the `Sym("pi")` a unit scale builds (`deg`, `hbar` in
        // natural units). They are one base, so `pi/pi` cancels.
        let canonical = |term: Term| {
            if is_pi(&term) {
                Term::Sym("pi".to_string())
            } else {
                term
            }
        };
        for term in flat {
            match canonical(term) {
                | Term::Num(value) => constant *= value,
                | Term::Pow(base, exponent) => {
                    let base = canonical(*base);
                    let hash = term_hash(&base);
                    match factors.iter_mut().find(|(existing_hash, existing, _)| {
                        *existing_hash == hash && *existing == base
                    }) {
                        | Some((_, _, existing)) => *existing += exponent,
                        | None => factors.push((hash, base, exponent)),
                    }
                },
                | other => {
                    let hash = term_hash(&other);
                    match factors.iter_mut().find(|(existing_hash, existing, _)| {
                        *existing_hash == hash && *existing == other
                    }) {
                        | Some((_, _, count)) => *count += BigRational::one(),
                        | None => factors.push((hash, other, BigRational::one())),
                    }
                },
            }
        }
        if constant.is_zero() {
            return zero();
        }
        // A merged power can itself be a number (`sqrt(2)*sqrt(2)`); it belongs
        // in the coefficient, not beside it.
        let mut powers = Vec::new();
        for (_, base, exponent) in factors {
            if exponent.is_zero() {
                continue;
            }
            match Term::pow(base, exponent) {
                | Term::Num(value) => constant *= value,
                | other => powers.push(other),
            }
        }
        if constant.is_zero() {
            return zero();
        }
        let mut items = Vec::new();
        if !constant.is_one() {
            items.push(rational(constant));
        }
        items.extend(powers);
        match items.len() {
            | 0 => one(),
            | 1 => items.remove(0),
            | _ => Term::Mul(items),
        }
    }

    /// Raises a term to a rational power.
    #[must_use]
    pub fn pow(
        base: Term,
        exponent: BigRational,
    ) -> Term {
        if exponent.is_zero() {
            return one();
        }
        if exponent.is_one() {
            return base;
        }
        if let Term::Num(value) = &base {
            if value.is_one() {
                return one();
            }
            if value.is_zero() && exponent.is_positive() {
                return zero();
            }
            if value.is_zero() && exponent.is_negative() {
                return Term::Sym("oo".to_string());
            }
            if exponent.denom().is_one()
                && let Some(power) = exponent.numer().to_i32()
            {
                return rational(aevia_dim::rat::pow_int(value, power));
            }
            // `u^(1/2)` is `sqrt(u)`, so a fractional power and a square root
            // are the same term and `sqrt(2)^2` folds (§6.3.1).
            if exponent == BigRational::new(BigInt::one(), BigInt::from(2)) {
                return Term::fun(Fun::Sqrt, vec![base]);
            }
            // A rational power of an exact number is exact when the root is:
            // `(1/4)^(1/2) = 1/2`, `8^(1/3) = 2`, `(-8)^(1/3) = -2`. Otherwise
            // the term stays formal (`2^(1/2)` is `sqrt(2)`).
            if !exponent.denom().is_one()
                && let Some(degree) = exponent.denom().to_u32()
                && let (Some(numerator_root), Some(denominator_root)) = (
                    perfect_root(value.numer(), degree),
                    perfect_root(value.denom(), degree),
                )
            {
                let mut root = BigRational::new(numerator_root, denominator_root);
                let power = exponent.numer().clone();
                if power.is_negative() {
                    if root.is_zero() {
                        return Term::Sym("oo".to_string());
                    }
                    root = root.recip();
                }
                if let Some(power) = power.abs().to_i32() {
                    return rational(aevia_dim::rat::pow_int(&root, power));
                }
            }
        }
        // `sqrt(u)^(2k) = u^k`, so `sqrt(2)^2` is exactly `2` and not a float
        // that happens to print as `2` (§6.3.1).
        if let Term::Fun(Fun::Sqrt, args) = &base
            && let (Some(inner), true) = (args.first(), exponent.denom().is_one())
            && let Some(power) = exponent.numer().to_i32()
            && power % 2 == 0
        {
            return Term::pow(
                inner.clone(),
                BigRational::from_integer(BigInt::from(power / 2)),
            );
        }
        // `(a*b)^n = a^n * b^n` for a positive integer `n`: keeps `sqrt(8)^2`
        // (`(2*sqrt(2))^2`) exact and `(2*x)^2` as `4*x^2`.
        if let Term::Mul(factors) = &base
            && exponent.denom().is_one()
            && exponent.is_positive()
        {
            return factors
                .iter()
                .map(|factor| Term::pow(factor.clone(), exponent.clone()))
                .fold(one(), Term::mul);
        }
        if let Term::Pow(inner, inner_exponent) = &base {
            // (x^a)^b = x^(ab) only when b is an integer; with a fractional
            // outer exponent the result is a magnitude, not the base.
            if exponent.denom().is_one() {
                return Term::pow((**inner).clone(), inner_exponent * exponent);
            }
        }
        Term::Pow(Box::new(base), exponent)
    }

    /// Negates a term.
    #[must_use]
    pub fn neg(term: Term) -> Term {
        Term::mul(Term::int(-1), term)
    }

    /// Subtracts `right` from `left`.
    #[must_use]
    pub fn sub(
        left: Term,
        right: Term,
    ) -> Term {
        Term::add(left, Term::neg(right))
    }

    /// Builds a function application, folding only what is exact.
    ///
    /// `sqrt(2)` must stay `sqrt(2)` (§6.3.1): folding it through `f64` would
    /// answer with `1.4142135623730951`, a `Float` the program never asked
    /// for, and `sqrt(2)^2` would then be `2.0000000000000004`. Only an exact
    /// result is folded here; everything else is a term.
    #[must_use]
    pub fn fun(
        function: Fun,
        args: Vec<Term>,
    ) -> Term {
        if let Fun::Ite = function {
            if args.len() == 3
                && let Term::Num(condition) = &args[0]
            {
                return if condition.is_zero() {
                    args[2].clone()
                } else {
                    args[1].clone()
                };
            }
            return Term::Fun(function, args);
        }
        if args.len() == 1 {
            if let Term::Num(value) = &args[0] {
                if let Some(result) = exact_function(function, value) {
                    return rational(result);
                }
                if let Fun::Sqrt = function
                    && let Some(result) = extract_square_factor(value)
                {
                    return result;
                }
            }
            if let Some(result) = exact_special(function, &args[0]) {
                return result;
            }
        }
        Term::Fun(function, args)
    }

    /// Substitutes a symbol by a term.
    ///
    /// # Errors
    ///
    /// A kernel handle the bridge cannot rewrite is a diagnostic, never a
    /// silently unchanged term: returning the input unchanged would make
    /// `subst` look like it worked.
    pub fn substitute(
        &self,
        symbol: &str,
        value: &Term,
    ) -> Result<Term, EvalError> {
        Ok(match self {
            | Self::Sym(name) if name == symbol => value.clone(),
            | Self::Num(_) | Self::Sym(_) => self.clone(),
            | Self::Add(items) => {
                items
                    .iter()
                    .map(|item| item.substitute(symbol, value))
                    .collect::<Result<Vec<_>, _>>()?
                    .into_iter()
                    .reduce(Term::add)
                    .unwrap_or_else(zero)
            },
            | Self::Mul(items) => {
                items
                    .iter()
                    .map(|item| item.substitute(symbol, value))
                    .collect::<Result<Vec<_>, _>>()?
                    .into_iter()
                    .reduce(Term::mul)
                    .unwrap_or_else(one)
            },
            | Self::Pow(base, exponent) => {
                Term::pow(base.substitute(symbol, value)?, exponent.clone())
            },
            | Self::Fun(function, args) => {
                Term::fun(
                    *function,
                    args.iter()
                        .map(|arg| arg.substitute(symbol, value))
                        .collect::<Result<Vec<_>, _>>()?,
                )
            },
            // The kernel owns the structure, so ask it.
            | Self::Foreign { .. } => {
                let kernel = aevia_rssn::kernel();
                let from = crate::kernel::kernel_symbol(symbol);
                let target = crate::kernel::term_val(value)?;
                let handle = self.as_foreign().expect("matched Foreign");
                let result = kernel
                    .substitute(handle, from, &target)
                    .map_err(|failure| crate::kernel::error(&failure))?;
                crate::kernel::term_of(&result).ok_or_else(|| {
                    EvalError::new(
                        "E1003",
                        "the kernel answered with a value that has no term form",
                    )
                })?
            },
        })
    }

    /// Differentiates with respect to a symbol.
    ///
    /// # Errors
    ///
    /// A kernel handle the bridge cannot differentiate is a diagnostic. It
    /// must never become `0`: a wrong derivative is worse than a failure.
    pub fn diff(
        &self,
        symbol: &str,
    ) -> Result<Term, EvalError> {
        Ok(match self {
            | Self::Num(_) => zero(),
            | Self::Sym(name) => {
                if name == symbol {
                    one()
                } else {
                    zero()
                }
            },
            | Self::Add(items) => {
                items
                    .iter()
                    .map(|item| item.diff(symbol))
                    .collect::<Result<Vec<_>, _>>()?
                    .into_iter()
                    .reduce(Term::add)
                    .unwrap_or_else(zero)
            },
            | Self::Mul(items) => {
                // Product rule, term by term.
                let mut sum = zero();
                for (index, item) in items.iter().enumerate() {
                    let derivative = item.diff(symbol)?;
                    if derivative == zero() {
                        continue;
                    }
                    let mut product = derivative;
                    for (other_index, other) in items.iter().enumerate() {
                        if other_index != index {
                            product = Term::mul(product, other.clone());
                        }
                    }
                    sum = Term::add(sum, product);
                }
                sum
            },
            | Self::Pow(base, exponent) => {
                let derivative = base.diff(symbol)?;
                if derivative == zero() {
                    return Ok(zero());
                }
                let lowered = Term::pow((**base).clone(), exponent - BigRational::one());
                let coefficient = rational(exponent.clone());
                Term::mul(Term::mul(coefficient, lowered), derivative)
            },
            | Self::Fun(function, args) => {
                if let Fun::Ite = function {
                    // A conditional has no derivative of its own; the honest
                    // answer names the capability instead of guessing a branch.
                    return Err(EvalError::new(
                        "E1001",
                        "differentiating an `ite` term needs the kernel",
                    ));
                }
                let Some(arg) = args.first() else {
                    return Ok(zero());
                };
                let inner = arg.diff(symbol)?;
                if inner == zero() {
                    return Ok(zero());
                }
                let outer = match function {
                    | Fun::Sin => Term::fun(Fun::Cos, vec![arg.clone()]),
                    | Fun::Cos => Term::neg(Term::fun(Fun::Sin, vec![arg.clone()])),
                    | Fun::Exp => Term::fun(Fun::Exp, vec![arg.clone()]),
                    | Fun::Ln => {
                        Term::pow(arg.clone(), BigRational::from_integer(BigInt::from(-1)))
                    },
                    | Fun::Sqrt => {
                        // d/du sqrt(u) = 1 / (2 sqrt(u))
                        let two = Term::Num(BigRational::from_integer(BigInt::from(2)));
                        let root = Term::fun(Fun::Sqrt, vec![arg.clone()]);
                        Term::pow(
                            Term::mul(two, root),
                            BigRational::from_integer(BigInt::from(-1)),
                        )
                    },
                    | Fun::Tan => {
                        let cos = Term::fun(Fun::Cos, vec![arg.clone()]);
                        Term::pow(cos, BigRational::from_integer(BigInt::from(-2)))
                    },
                    // |u| is differentiated formally as sign(u) = |u| / u.
                    | Fun::Abs => {
                        Term::mul(
                            Term::fun(Fun::Abs, vec![arg.clone()]),
                            Term::pow(arg.clone(), BigRational::from_integer(BigInt::from(-1))),
                        )
                    },
                    | Fun::Ite => unreachable!("handled above"),
                    // A named special function (`erf`, `fresnels`, a plugin
                    // operator) is differentiated by the kernel, which owns
                    // its derivative rule: the whole term is built there and
                    // its derivative read back.
                    | Fun::Other(_) => {
                        let Ok(aevia_rssn::Val::Term(handle)) = crate::kernel::term_val(self)
                        else {
                            return Err(EvalError::new(
                                "E1003",
                                "the kernel could not build this term to differentiate it",
                            ));
                        };
                        let result = aevia_rssn::kernel()
                            .diff(handle, symbol)
                            .map_err(|failure| crate::kernel::error(&failure))?;
                        return crate::kernel::term_of(&result).ok_or_else(|| {
                            EvalError::new(
                                "E1003",
                                "the kernel answered with a value that has no term form",
                            )
                        });
                    },
                };
                Term::mul(outer, inner)
            },
            | Self::Foreign { .. } => {
                let handle = self.as_foreign().expect("matched Foreign");
                let result = aevia_rssn::kernel()
                    .diff(handle, symbol)
                    .map_err(|failure| crate::kernel::error(&failure))?;
                crate::kernel::term_of(&result).ok_or_else(|| {
                    EvalError::new(
                        "E1003",
                        "the kernel answered with a value that has no term form",
                    )
                })?
            },
        })
    }

    /// True when the term has no free symbols other than the mathematical
    /// constants `pi` and `e`.
    #[must_use]
    pub fn is_numeric(&self) -> bool {
        self.try_free_symbols()
            .map(|symbols| symbols.iter().all(|name| name == "pi" || name == "e"))
            .unwrap_or(false)
    }

    /// Evaluates to `f64` when every symbol is bound.
    ///
    /// # Errors
    ///
    /// Returns the name of the first unbound symbol.
    pub fn eval(
        &self,
        bindings: &HashMap<String, f64>,
    ) -> Result<f64, String> {
        match self {
            | Self::Num(value) => Ok(aevia_dim::rat::to_f64(value)),
            | Self::Sym(name) => {
                match bindings.get(name).copied() {
                    | Some(value) => Ok(value),
                    // The mathematical constants are part of the prelude; a term
                    // that only mentions them is numeric.
                    | None if name == "pi" => Ok(std::f64::consts::PI),
                    | None if name == "e" => Ok(std::f64::consts::E),
                    | None => Err(name.clone()),
                }
            },
            | Self::Add(items) => {
                let mut total = 0.0;
                for item in items {
                    total += item.eval(bindings)?;
                }
                Ok(total)
            },
            | Self::Mul(items) => {
                let mut total = 1.0;
                for item in items {
                    total *= item.eval(bindings)?;
                }
                Ok(total)
            },
            | Self::Pow(base, exponent) => {
                let base = base.eval(bindings)?;
                let exponent = aevia_dim::rat::to_f64(exponent);
                Ok(base.powf(exponent))
            },
            | Self::Fun(function, args) => {
                let value = args
                    .first()
                    .map_or(Ok(f64::NAN), |arg| arg.eval(bindings))?;
                Ok(function.eval(value))
            },
            | Self::Foreign { .. } => {
                crate::kernel::eval_term(self, bindings).ok_or_else(|| "a kernel term".to_string())
            },
        }
    }
}

fn exact_function(
    function: Fun,
    value: &BigRational,
) -> Option<BigRational> {
    match function {
        | Fun::Sqrt if value.is_positive() => {
            let numerator = value.numer().clone();
            let denominator = value.denom().clone();
            let root_num = perfect_root(&numerator, 2)?;
            let root_den = perfect_root(&denominator, 2)?;
            Some(BigRational::new(root_num, root_den))
        },
        | Fun::Abs => Some(value.abs()),
        | Fun::Exp if value.is_zero() => Some(BigRational::one()),
        | Fun::Ln if value.is_one() => Some(BigRational::zero()),
        | _ => None,
    }
}

fn perfect_root(
    value: &BigInt,
    degree: u32,
) -> Option<BigInt> {
    // An even root of a negative number is not real; an odd root keeps the
    // sign, so `(-8)^(1/3) = -2` (§6.3.4 leaves only negative *even* roots
    // formal).
    if degree == 0 {
        return None;
    }
    let negative = value.is_negative();
    if negative && degree.is_multiple_of(2) {
        return None;
    }
    // Exact integer root: no `f64` round trip, so integers above 2^53 are
    // decided correctly.
    let root = value.abs().nth_root(degree);
    let candidate = if negative { -root } else { root };
    if candidate.pow(degree) == *value {
        Some(candidate)
    } else {
        None
    }
}

/// The largest `s` with `s^2 | n` (found by trial division with a bounded
/// divisor, plus a whole-cofactor square test) and the cofactor `n / s^2`.
fn split_square(n: &BigInt) -> (BigInt, BigInt) {
    let mut square = BigInt::one();
    let mut rest = n.clone();
    let mut divisor = BigInt::from(2);
    let bound = BigInt::from(10_000);
    while divisor <= bound && &divisor * &divisor <= rest {
        let squared = &divisor * &divisor;
        while (&rest % &squared).is_zero() {
            rest /= &squared;
            square *= &divisor;
        }
        divisor += 1;
    }
    let root = rest.sqrt();
    if &root * &root == rest {
        square *= root;
        rest = BigInt::one();
    }
    (square, rest)
}

/// `sqrt(n/d)` for a positive rational that is not a perfect square, with the
/// square factor pulled out: `sqrt(8) = 2*sqrt(2)`, `sqrt(1/8) = 1/4*sqrt(2)`.
fn extract_square_factor(value: &BigRational) -> Option<Term> {
    if !value.is_positive() {
        return None;
    }
    // sqrt(n/d) = sqrt(n*d) / d
    let product = value.numer() * value.denom();
    let (square, rest) = split_square(&product);
    if rest.is_one() {
        // A perfect square; `exact_function` owns that case.
        return None;
    }
    let coefficient = BigRational::new(square, value.denom().clone());
    let root = Term::Fun(Fun::Sqrt, vec![Term::Num(BigRational::from_integer(rest))]);
    if coefficient.is_one() {
        Some(root)
    } else {
        Some(Term::Mul(vec![Term::Num(coefficient), root]))
    }
}

/// Whether a term is the constant `pi`: the prelude binds it to a kernel
/// handle whose rendering is `pi`, and terms built in this module use
/// `Sym("pi")`.
fn is_pi(term: &Term) -> bool {
    match term {
        | Term::Sym(name) => name == "pi",
        | Term::Foreign { text, .. } => text.as_ref() == "pi",
        | _ => false,
    }
}

/// `c` when `term` is `c*pi` (`pi` itself is `c = 1`).
fn pi_multiple(term: &Term) -> Option<BigRational> {
    if is_pi(term) {
        return Some(BigRational::one());
    }
    match term {
        | Term::Mul(items) => {
            match items.as_slice() {
                | [Term::Num(coefficient), pi] if is_pi(pi) => Some(coefficient.clone()),
                | _ => None,
            }
        },
        | _ => None,
    }
}

/// `sin`, `cos`, `tan` at a multiple of `pi` with denominator 1, 2, 3, 4 or 6,
/// and the inverse pairs `exp(ln u)`, `ln(exp u)` (symbols are real).
fn exact_special(
    function: Fun,
    arg: &Term,
) -> Option<Term> {
    match function {
        | Fun::Sin | Fun::Cos | Fun::Tan => {
            if let Term::Num(value) = arg {
                if value.is_zero() {
                    return Some(match function {
                        | Fun::Cos => one(),
                        | _ => zero(),
                    });
                }
                return None;
            }
            let multiple = pi_multiple(arg)?;
            if !matches!(multiple.denom().to_u32(), Some(1 | 2 | 3 | 4 | 6)) {
                return None;
            }
            // `c*pi` is `180*c` degrees, an integer for these denominators.
            let degrees = (multiple * BigRational::from_integer(BigInt::from(180))).to_integer();
            exact_trig(function, &degrees)
        },
        | Fun::Exp => {
            match arg {
                | Term::Fun(Fun::Ln, inner) if inner.len() == 1 => Some(inner[0].clone()),
                | _ => None,
            }
        },
        | Fun::Ln => {
            match arg {
                | Term::Fun(Fun::Exp, inner) if inner.len() == 1 => Some(inner[0].clone()),
                | _ => None,
            }
        },
        | _ => None,
    }
}

/// `degrees mod modulus`, in `0..modulus`.
fn degrees_mod(
    degrees: &BigInt,
    modulus: i64,
) -> i64 {
    let modulus = BigInt::from(modulus);
    (((degrees % &modulus) + &modulus) % &modulus)
        .to_i64()
        .expect("remainder is below the modulus")
}

fn exact_trig(
    function: Fun,
    degrees: &BigInt,
) -> Option<Term> {
    let radical = |coefficient: BigRational, root: i64| -> Term {
        Term::Mul(vec![
            rational(coefficient),
            Term::Fun(Fun::Sqrt, vec![Term::int(root)]),
        ])
    };
    let ratio = |numerator: i64, denominator: i64| {
        BigRational::new(BigInt::from(numerator), BigInt::from(denominator))
    };
    let signed = |negative: bool, value: Term| {
        if negative {
            Term::mul(Term::int(-1), value)
        } else {
            value
        }
    };
    // sin of a first-quadrant reference angle in degrees.
    let sin_ref = |angle: i64| -> Option<Term> {
        Some(match angle {
            | 0 => zero(),
            | 30 => rational(ratio(1, 2)),
            | 45 => radical(ratio(1, 2), 2),
            | 60 => radical(ratio(1, 2), 3),
            | 90 => one(),
            | _ => return None,
        })
    };
    let sine = |deg: &BigInt| -> Option<Term> {
        let full = degrees_mod(deg, 360);
        let (negative, within) = if full >= 180 {
            (true, full - 180)
        } else {
            (false, full)
        };
        let reference = within.min(180 - within);
        Some(signed(negative, sin_ref(reference)?))
    };
    match function {
        | Fun::Sin => sine(degrees),
        | Fun::Cos => sine(&(degrees + BigInt::from(90))),
        | Fun::Tan => {
            let full = degrees_mod(degrees, 180);
            if full == 90 {
                // Undefined: stay unevaluated rather than invent a value.
                return None;
            }
            let (negative, reference) = if full > 90 {
                (true, 180 - full)
            } else {
                (false, full)
            };
            let value = match reference {
                | 0 => zero(),
                | 30 => radical(ratio(1, 3), 3),
                | 45 => one(),
                | 60 => Term::Fun(Fun::Sqrt, vec![Term::int(3)]),
                | _ => return None,
            };
            Some(signed(negative, value))
        },
        | _ => None,
    }
}

/// Whether a term is the positive infinity constant (`oo`, possibly held as a
/// kernel handle whose rendered text is `oo`).
fn is_infinity_term(term: &Term) -> bool {
    match term {
        | Term::Sym(name) => name == "oo",
        | Term::Foreign { text, .. } => text.as_ref() == "oo",
        | _ => false,
    }
}

/// `-1`/`+1` for a symbolic infinity term, `None` otherwise.
fn infinity_sign_of(term: &Term) -> Option<i8> {
    if is_infinity_term(term) {
        return Some(1);
    }
    match term {
        | Term::Mul(items) => {
            match items.as_slice() {
                | [Term::Num(coefficient), rest]
                    if coefficient == &BigRational::from_integer(BigInt::from(-1))
                        && is_infinity_term(rest) =>
                {
                    Some(-1)
                },
                | _ => None,
            }
        },
        | _ => None,
    }
}

/// Whether an infinity occurs anywhere in `term`.
///
/// The exact (Real) arithmetic path is *partial* on the extended reals: when an
/// operation on `oo` would collapse to a finite number only through an
/// undefined form (`oo * 0`, `oo / oo`, `oo ^ 0`, …), the evaluator keeps the
/// expression unevaluated instead of inventing a finite answer. The Float path
/// stays IEEE, where the same operations are `nan`.
#[must_use]
pub fn contains_infinity(term: &Term) -> bool {
    match term {
        | Term::Num(_) => false,
        | Term::Sym(_) | Term::Foreign { .. } => infinity_sign_of(term).is_some(),
        | Term::Add(items) | Term::Mul(items) => items.iter().any(contains_infinity),
        | Term::Pow(base, _) => contains_infinity(base),
        | Term::Fun(_, args) => args.iter().any(contains_infinity),
    }
}

impl fmt::Display for Term {
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        match self {
            | Self::Num(value) => f.write_str(&format_rational_decimal(value)),
            | _ => self.render(f, false),
        }
    }
}

impl Term {
    /// Renders the term with its internal symbol identity kept.
    ///
    /// `repr` of a symbolic value has to name the symbol the runtime holds
    /// (§6.12.3), so `symbol("x")` twice reads as `x` and `x#2` rather than
    /// `x + x`, which is not the value that was computed.
    #[must_use]
    pub fn to_string_repr(&self) -> String {
        let mut out = String::new();
        let mut formatter = ReprFormatter(&mut out);
        let _ = self.render(&mut formatter, true);
        out
    }

    fn render(
        &self,
        f: &mut dyn fmt::Write,
        repr: bool,
    ) -> fmt::Result {
        match self {
            // Inside a compound term a rational is always a fraction, so an
            // exact `1/4` coefficient never reads as the decimal `0.25`; a bare
            // number follows the value layer's display convention (see the
            // `Display` impl).
            | Self::Num(value) => f.write_str(&aevia_dim::rat::format_rational(value)),
            // `symbol("x")` allocates `x`, `x#2`, ... so that two calls are
            // different symbols (§6.3.6); only that numeric suffix is hidden.
            // A user's `symbol("a#b")` keeps its name.
            | Self::Sym(name) => {
                if repr {
                    f.write_str(name)
                } else {
                    f.write_str(display_name(name))
                }
            },
            | Self::Add(items) => {
                // Polynomial terms print by descending degree, the order a
                // reader expects (`x^3 - 3*x + 1`); terms of equal degree keep
                // their canonical order, and constants come last.
                let mut ordered: Vec<&Term> = items.iter().collect();
                ordered.sort_by_key(|item| std::cmp::Reverse(display_degree(item)));
                for (index, item) in ordered.into_iter().enumerate() {
                    if index > 0 {
                        // `… - oo` is the negative-infinity term, not `-1*oo`.
                        if infinity_sign_of(item) == Some(-1) {
                            f.write_str(" - oo")?;
                            continue;
                        }
                        if is_negative(item) {
                            f.write_str(" - ")?;
                            absolute(item).render(f, repr)?;
                            continue;
                        }
                        f.write_str(" + ")?;
                    }
                    item.render(f, repr)?;
                }
                Ok(())
            },
            | Self::Mul(items) => {
                // `-1 * oo` is the extended-real negative infinity and reads as
                // `-oo`, not `-1*oo` (§6.3.4: `ln(0)` returns `-oo`).
                if let [Self::Num(coefficient), rest] = items.as_slice()
                    && coefficient == &BigRational::from_integer(BigInt::from(-1))
                    && is_infinity_term(rest)
                {
                    f.write_str("-oo")?;
                    return Ok(());
                }
                // A leading `-1` reads as a sign: `-x`, `-sqrt(3)`, `-(a + b)`.
                if let [Self::Num(coefficient), rest @ ..] = items.as_slice()
                    && coefficient == &BigRational::from_integer(BigInt::from(-1))
                    && !rest.is_empty()
                {
                    f.write_str("-")?;
                    let tail = if rest.len() == 1 {
                        rest[0].clone()
                    } else {
                        Self::Mul(rest.to_vec())
                    };
                    if matches!(tail, Self::Add(_)) {
                        f.write_str("(")?;
                        tail.render(f, repr)?;
                        return f.write_str(")");
                    }
                    return tail.render(f, repr);
                }
                // Factors with a negative power form the denominator:
                // `2*(x^2 + 1)^-1*x` reads as `2*x/(x^2 + 1)`, and an
                // unevaluated `oo / oo` stays legible. The value is unchanged.
                let (denominator, numerator): (Vec<&Term>, Vec<&Term>) = items.iter().partition(
                    |item| matches!(item, Self::Pow(_, exponent) if exponent.is_negative()),
                );
                if !denominator.is_empty() {
                    let inverted: Vec<Term> = denominator
                        .iter()
                        .map(|item| {
                            match item {
                                | Self::Pow(base, exponent) if exponent == &-BigRational::one() => {
                                    (**base).clone()
                                },
                                | Self::Pow(base, exponent) => Self::Pow(base.clone(), -exponent),
                                | other => (*other).clone(),
                            }
                        })
                        .collect();
                    let top = match numerator.as_slice() {
                        | [] => Self::Num(BigRational::one()),
                        | [single] => (*single).clone(),
                        | many => Self::Mul(many.iter().map(|item| (*item).clone()).collect()),
                    };
                    let bottom = if inverted.len() == 1 {
                        inverted
                            .into_iter()
                            .next()
                            .unwrap_or(Self::Num(BigRational::one()))
                    } else {
                        Self::Mul(inverted)
                    };
                    if matches!(top, Self::Add(_)) {
                        f.write_str("(")?;
                        top.render(f, repr)?;
                        f.write_str(")")?;
                    } else {
                        top.render(f, repr)?;
                    }
                    f.write_str("/")?;
                    // `^` binds tighter than `/`, so `x/y^2` needs no brackets.
                    let wrap = matches!(bottom, Self::Add(_) | Self::Mul(_))
                        || matches!(&bottom, Self::Num(value) if !value.denom().is_one())
                        || is_negative(&bottom);
                    if wrap {
                        f.write_str("(")?;
                        bottom.render(f, repr)?;
                        return f.write_str(")");
                    }
                    return bottom.render(f, repr);
                }
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        f.write_str("*")?;
                    }
                    let needs_parens = matches!(item, Self::Add(_));
                    if needs_parens {
                        f.write_str("(")?;
                        item.render(f, repr)?;
                        f.write_str(")")?;
                    } else {
                        item.render(f, repr)?;
                    }
                }
                Ok(())
            },
            // `b^(1/2)` is `sqrt(b)` (§8.2) and reads that way; `b^(-1/2)` is
            // `1/sqrt(b)`.
            | Self::Pow(base, exponent)
                if exponent.denom() == &BigInt::from(2)
                    && exponent.numer().abs() == BigInt::one() =>
            {
                if exponent.is_negative() {
                    f.write_str("1/")?;
                }
                f.write_str("sqrt(")?;
                base.render(f, repr)?;
                f.write_str(")")
            },
            // `b^-n` is `1/b^n`, the form a reader writes.
            | Self::Pow(base, exponent) if exponent.is_negative() => {
                f.write_str("1/")?;
                let positive = if exponent == &-BigRational::one() {
                    (**base).clone()
                } else {
                    Self::Pow(base.clone(), -exponent)
                };
                if matches!(positive, Self::Add(_) | Self::Mul(_))
                    || matches!(&positive, Self::Num(value) if !value.denom().is_one())
                    || is_negative(&positive)
                {
                    f.write_str("(")?;
                    positive.render(f, repr)?;
                    return f.write_str(")");
                }
                positive.render(f, repr)
            },
            | Self::Pow(base, exponent) => {
                // A negative base needs parentheses: `-8^(1/3)` would read as
                // the negation of `8^(1/3)`, not as `(-8)^(1/3)`.
                let needs_parens = matches!(**base, Self::Add(_) | Self::Mul(_))
                    || matches!(&**base, Self::Num(value) if value.is_negative() || !value.denom().is_one());
                if needs_parens {
                    f.write_str("(")?;
                    base.render(f, repr)?;
                    f.write_str(")")?;
                } else {
                    base.render(f, repr)?;
                }
                if exponent.denom().is_one() {
                    write!(f, "^{}", exponent.numer())
                } else {
                    write!(f, "^({})", aevia_dim::format_rational(exponent))
                }
            },
            | Self::Fun(function, args) => {
                write!(f, "{}(", function.as_str())?;
                for (index, arg) in args.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    arg.render(f, repr)?;
                }
                f.write_str(")")
            },
            | Self::Foreign { text, .. } => f.write_str(text),
        }
    }
}

/// The polynomial degree used to order a sum for display.
///
/// Symbols other than the constants `pi`/`e` have degree one, powers multiply
/// it, products add it; anything else (numbers, functions, kernel terms) has
/// degree zero. The key only orders display: it never changes the value.
fn display_degree(term: &Term) -> BigRational {
    match term {
        | Term::Sym(name) if name != "pi" && name != "e" && name != "oo" => BigRational::one(),
        | Term::Pow(base, exponent) => display_degree(base) * exponent,
        | Term::Mul(items) => items.iter().map(display_degree).sum(),
        | _ => BigRational::zero(),
    }
}

/// A `fmt::Write` that appends to a `String`.
struct ReprFormatter<'a>(&'a mut String);

impl fmt::Write for ReprFormatter<'_> {
    fn write_str(
        &mut self,
        text: &str,
    ) -> fmt::Result {
        self.0.push_str(text);
        Ok(())
    }
}

/// Renders a term in LaTeX (`latex`) or Typst syntax.
///
/// The output follows mathematical typesetting rather than the source form:
/// single-letter symbols are italic variables, Greek names become their
/// letters, a product with negative powers is a fraction, `b^(1/2)` is a
/// root, the elementary functions use their upright operator names, and sums
/// print by descending degree like `show` does.
#[must_use]
pub fn term_to_math(
    term: &Term,
    latex: bool,
) -> String {
    // Precedences: 2 = sum, 1 = product, 0 = atom. A child whose precedence is
    // above what its position accepts is bracketed.
    render_math(term, latex, 2)
}

fn render_math(
    term: &Term,
    latex: bool,
    parent_prec: u8,
) -> String {
    let (text, prec) = math_parts(term, latex);
    if prec > parent_prec {
        if latex {
            format!(r"\left({text}\right)")
        } else {
            format!("({text})")
        }
    } else {
        text
    }
}

fn math_fraction(
    numerator: &str,
    denominator: &str,
    latex: bool,
) -> String {
    if latex {
        format!(r"\frac{{{numerator}}}{{{denominator}}}")
    } else {
        format!("frac({numerator}, {denominator})")
    }
}

/// The Greek letters a symbol name can spell.
const GREEK: &[&str] = &[
    "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa",
    "lambda", "mu", "nu", "xi", "pi", "rho", "sigma", "tau", "upsilon", "phi", "chi", "psi",
    "omega", "Gamma", "Delta", "Theta", "Lambda", "Xi", "Pi", "Sigma", "Upsilon", "Phi", "Psi",
    "Omega",
];

fn math_symbol(
    name: &str,
    latex: bool,
) -> String {
    let name = display_name(name);
    // `x_1` is `x` with subscript `1`.
    if let Some((base, sub)) = name.split_once('_')
        && !base.is_empty()
        && !sub.is_empty()
    {
        let base = math_symbol(base, latex);
        let sub = math_symbol(sub, latex);
        return if latex {
            format!("{base}_{{{sub}}}")
        } else {
            format!("{base}_({sub})")
        };
    }
    if GREEK.contains(&name) {
        return if latex {
            format!(r"\{name}")
        } else {
            name.to_string()
        };
    }
    if name.chars().count() == 1 || name.chars().all(|c| c.is_ascii_digit()) {
        return name.to_string();
    }
    if latex {
        format!(r"\mathrm{{{name}}}")
    } else {
        format!("\"{name}\"")
    }
}

fn math_parts(
    term: &Term,
    latex: bool,
) -> (String, u8) {
    match term {
        | Term::Num(value) => {
            if value.denom().is_one() {
                let text = value.numer().to_string();
                // A negative number is a sum-level form: `2 \cdot (-3)`.
                let prec = u8::from(value.is_negative()) * 2;
                (text, prec)
            } else {
                let fraction = math_fraction(
                    &value.numer().abs().to_string(),
                    &value.denom().to_string(),
                    latex,
                );
                if value.is_negative() {
                    (format!("-{fraction}"), 2)
                } else {
                    (fraction, 0)
                }
            }
        },
        | Term::Sym(name) => (math_symbol(name, latex), 0),
        | Term::Add(items) => {
            let mut ordered: Vec<&Term> = items.iter().collect();
            ordered.sort_by_key(|item| std::cmp::Reverse(display_degree(item)));
            let mut out = String::new();
            for (index, item) in ordered.into_iter().enumerate() {
                if index == 0 {
                    out.push_str(&render_math(item, latex, 2));
                } else if is_negative(item) {
                    out.push_str(" - ");
                    out.push_str(&render_math(&absolute(item), latex, 1));
                } else {
                    out.push_str(" + ");
                    out.push_str(&render_math(item, latex, 1));
                }
            }
            (out, 2)
        },
        | Term::Mul(items) => math_product(items, latex),
        | Term::Pow(base, exponent) => {
            if exponent.is_negative() {
                let positive = Term::pow((**base).clone(), -exponent.clone());
                return (
                    math_fraction("1", &render_math(&positive, latex, 2), latex),
                    0,
                );
            }
            if exponent.denom() == &BigInt::from(2) && exponent.numer().is_one() {
                let inner = render_math(base, latex, 2);
                return if latex {
                    (format!(r"\sqrt{{{inner}}}"), 0)
                } else {
                    (format!("sqrt({inner})"), 0)
                };
            }
            // A function or a power as a base is bracketed: `(x^2)^3`.
            let base_text = match **base {
                | Term::Pow(..) | Term::Fun(..) => {
                    let inner = render_math(base, latex, 2);
                    if latex {
                        format!(r"\left({inner}\right)")
                    } else {
                        format!("({inner})")
                    }
                },
                | _ => render_math(base, latex, 0),
            };
            let exponent_text = if exponent.denom().is_one() {
                exponent.numer().to_string()
            } else {
                math_fraction(
                    &exponent.numer().to_string(),
                    &exponent.denom().to_string(),
                    latex,
                )
            };
            if latex {
                (format!("{base_text}^{{{exponent_text}}}"), 0)
            } else {
                (format!("{base_text}^({exponent_text})"), 0)
            }
        },
        | Term::Fun(function, args) => {
            let rendered: Vec<String> = args.iter().map(|arg| render_math(arg, latex, 2)).collect();
            let name = function.as_str();
            let bracket = |inner: &str| {
                if latex {
                    format!(r"\left({inner}\right)")
                } else {
                    format!("({inner})")
                }
            };
            match function {
                | Fun::Sqrt if args.len() == 1 => {
                    if latex {
                        (format!(r"\sqrt{{{}}}", rendered[0]), 0)
                    } else {
                        (format!("sqrt({})", rendered[0]), 0)
                    }
                },
                | Fun::Exp if args.len() == 1 => {
                    if latex {
                        (format!("e^{{{}}}", rendered[0]), 0)
                    } else {
                        (format!("e^({})", rendered[0]), 0)
                    }
                },
                | Fun::Abs if args.len() == 1 => {
                    if latex {
                        (format!(r"\left|{}\right|", rendered[0]), 0)
                    } else {
                        (format!("abs({})", rendered[0]), 0)
                    }
                },
                | Fun::Ite if args.len() == 3 => {
                    let body = if latex {
                        format!(
                            r"\begin{{cases}} {} & {} \\ {} & \text{{otherwise}} \end{{cases}}",
                            rendered[1], rendered[0], rendered[2]
                        )
                    } else {
                        format!(
                            "cases({} & {}, {} & \"otherwise\")",
                            rendered[1], rendered[0], rendered[2]
                        )
                    };
                    (body, 0)
                },
                | _ => {
                    let arguments = bracket(&rendered.join(", "));
                    let known = matches!(
                        name,
                        "sin"
                            | "cos"
                            | "tan"
                            | "ln"
                            | "log"
                            | "sinh"
                            | "cosh"
                            | "tanh"
                            | "exp"
                            | "arcsin"
                            | "arccos"
                            | "arctan"
                            | "cot"
                            | "sec"
                            | "csc"
                    );
                    if latex {
                        if known {
                            (format!(r"\{name}{arguments}"), 0)
                        } else {
                            (format!(r"\operatorname{{{name}}}{arguments}"), 0)
                        }
                    } else if known {
                        (format!("{name}{arguments}"), 0)
                    } else {
                        (format!("op(\"{name}\"){arguments}"), 0)
                    }
                },
            }
        },
        | Term::Foreign { text, .. } => (text.to_string(), 0),
    }
}

/// A product as numerator over denominator, with the sign in front.
fn math_product(
    items: &[Term],
    latex: bool,
) -> (String, u8) {
    let mut negative = false;
    let mut coefficient_numerator = BigInt::one();
    let mut coefficient_denominator = BigInt::one();
    let mut numerator: Vec<Term> = Vec::new();
    let mut denominator: Vec<Term> = Vec::new();
    for item in items {
        match item {
            | Term::Num(value) => {
                if value.is_negative() {
                    negative = !negative;
                }
                coefficient_numerator *= value.numer().abs();
                coefficient_denominator *= value.denom();
            },
            | Term::Pow(base, exponent) if exponent.is_negative() => {
                denominator.push(Term::pow((**base).clone(), -exponent.clone()));
            },
            | other => numerator.push(other.clone()),
        }
    }
    let join = |factors: &[Term], coefficient: &BigInt, in_fraction: bool| -> String {
        let mut parts: Vec<String> = Vec::new();
        if !coefficient.is_one() || factors.is_empty() {
            parts.push(coefficient.to_string());
        }
        // A lone factor fills its side of a fraction bar and needs no
        // brackets: `\frac{x}{y + 1}`, not `\frac{x}{\left(y + 1\right)}`.
        let alone = in_fraction && parts.is_empty() && factors.len() == 1;
        for factor in factors {
            parts.push(render_math(factor, latex, if alone { 2 } else { 1 }));
        }
        let mut out = String::new();
        for (index, part) in parts.iter().enumerate() {
            if index > 0 {
                // Juxtaposition, except where two numbers would run together.
                let digit_next = part.starts_with(|c: char| c.is_ascii_digit());
                out.push_str(match (latex, digit_next) {
                    | (true, true) => r" \cdot ",
                    | (true, false) => r" ",
                    | (false, true) => " dot ",
                    | (false, false) => " ",
                });
            }
            out.push_str(part);
        }
        out
    };
    let body = if denominator.is_empty() && coefficient_denominator.is_one() {
        join(&numerator, &coefficient_numerator, false)
    } else {
        math_fraction(
            &join(&numerator, &coefficient_numerator, true),
            &join(&denominator, &coefficient_denominator, true),
            latex,
        )
    };
    if negative {
        (format!("-{body}"), 2)
    } else {
        (body, 1)
    }
}

/// The name a symbol prints as.
fn display_name(name: &str) -> &str {
    match name.rsplit_once('#') {
        | Some((base, suffix))
            if !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()) =>
        {
            base
        },
        | _ => name,
    }
}

fn is_negative(term: &Term) -> bool {
    match term {
        | Term::Num(value) => value.is_negative(),
        | Term::Mul(factors) => {
            factors
                .first()
                .is_some_and(|first| matches!(first, Term::Num(value) if value.is_negative()))
        },
        | _ => false,
    }
}

fn absolute(term: &Term) -> Term {
    match term {
        | Term::Num(value) => Term::Num(value.abs()),
        | Term::Mul(factors) => {
            let mut factors = factors.clone();
            if let Some(Term::Num(value)) = factors.first_mut() {
                *value = value.abs();
            }
            // `|-1|*x` is just `x`: a unit coefficient is not printed.
            if matches!(factors.first(), Some(Term::Num(value)) if value.is_one())
                && factors.len() > 1
            {
                factors.remove(0);
            }
            if factors.len() == 1 {
                factors.remove(0)
            } else {
                Term::Mul(factors)
            }
        },
        | other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn x() -> Term {
        Term::Sym("x".to_string())
    }

    #[test]
    fn like_terms_are_collected() {
        let term = Term::add(x(), x());
        assert_eq!(term.to_string(), "2*x");
    }

    #[test]
    fn constants_fold() {
        assert_eq!(Term::add(Term::int(1), Term::int(2)).to_string(), "3");
        assert_eq!(Term::mul(Term::int(2), Term::int(3)).to_string(), "6");
    }

    #[test]
    fn differentiation_uses_the_power_rule() {
        let term = Term::Pow(Box::new(x()), BigRational::from_integer(BigInt::from(3)));
        let derivative = term.diff("x").expect("differentiates");
        assert_eq!(derivative.to_string(), "3*x^2");
    }

    #[test]
    fn differentiation_of_a_polynomial_matches_the_spec() {
        // x^3 - 3x + 1 -> 3x^2 - 3
        let term = Term::add(
            Term::Pow(Box::new(x()), BigRational::from_integer(BigInt::from(3))),
            Term::add(Term::mul(Term::int(-3), x()), Term::int(1)),
        );
        let derivative = term.diff("x").expect("differentiates");
        assert_eq!(derivative.to_string(), "3*x^2 - 3");
    }

    #[test]
    fn substitution_is_structural() {
        let term = Term::add(
            Term::Pow(Box::new(x()), BigRational::from_integer(BigInt::from(2))),
            Term::int(1),
        );
        let replaced = term.substitute("x", &Term::int(3)).expect("substitutes");
        assert_eq!(replaced.to_string(), "10");
    }

    #[test]
    fn numeric_evaluation_reports_free_symbols() {
        let term = Term::add(x(), Term::int(1));
        let mut bindings = HashMap::new();
        assert_eq!(term.eval(&bindings), Err("x".to_string()));
        bindings.insert("x".to_string(), 2.0);
        assert_eq!(term.eval(&bindings), Ok(3.0));
    }

    fn sym(name: &str) -> Term {
        Term::Sym(name.to_string())
    }

    fn pi_times(
        numerator: i64,
        denominator: i64,
    ) -> Term {
        Term::mul(
            Term::Num(BigRational::new(
                BigInt::from(numerator),
                BigInt::from(denominator),
            )),
            sym("pi"),
        )
    }

    fn half() -> Term {
        Term::Num(BigRational::new(BigInt::from(1), BigInt::from(2)))
    }

    #[test]
    fn coefficients_are_collected() {
        assert_eq!(Term::sub(x(), x()), zero());
        assert_eq!(
            Term::add(Term::mul(Term::int(2), x()), Term::mul(Term::int(3), x())),
            Term::mul(Term::int(5), x())
        );
        let sum = Term::add(
            Term::add(x(), Term::mul(Term::int(2), x())),
            Term::neg(Term::mul(Term::int(3), x())),
        );
        assert_eq!(sum, zero());
        let shifted = Term::add(x(), Term::int(1));
        assert_eq!(Term::sub(shifted.clone(), shifted), zero());
        // Monomials with several factors collect too.
        let xy = Term::mul(x(), sym("y"));
        assert_eq!(
            Term::sub(Term::mul(Term::int(2), xy.clone()), xy.clone()),
            xy
        );
    }

    #[test]
    fn trigonometric_values_are_exact() {
        let sin = |t: Term| Term::fun(Fun::Sin, vec![t]);
        let cos = |t: Term| Term::fun(Fun::Cos, vec![t]);
        let tan = |t: Term| Term::fun(Fun::Tan, vec![t]);
        assert_eq!(sin(pi_times(1, 6)), half());
        assert_eq!(cos(pi_times(1, 3)), half());
        assert_eq!(sin(sym("pi")), zero());
        assert_eq!(cos(Term::int(0)), one());
        assert_eq!(tan(pi_times(1, 4)), one());
        assert_eq!(tan(pi_times(5, 4)), one());
        assert_eq!(cos(sym("pi")), Term::int(-1));
        assert_eq!(sin(pi_times(-1, 2)), Term::int(-1));
        assert_eq!(sin(pi_times(5, 6)), half());
        assert_eq!(sin(pi_times(-5, 6)), Term::neg(half()));
        // tan(pi/2) stays a term.
        assert!(matches!(tan(pi_times(1, 2)), Term::Fun(Fun::Tan, _)));
        // The radical values square back to rationals.
        let s = sin(pi_times(1, 4));
        assert_eq!(Term::mul(s.clone(), s), half());
        let t = tan(pi_times(1, 3));
        assert_eq!(Term::mul(t.clone(), t), Term::int(3));
        // Unsupported denominators and symbolic arguments are untouched.
        assert!(matches!(sin(pi_times(1, 5)), Term::Fun(Fun::Sin, _)));
        assert!(matches!(sin(x()), Term::Fun(Fun::Sin, _)));
    }

    #[test]
    fn exp_and_ln_cancel() {
        let exp = |t: Term| Term::fun(Fun::Exp, vec![t]);
        let ln = |t: Term| Term::fun(Fun::Ln, vec![t]);
        assert_eq!(ln(exp(Term::int(2))), Term::int(2));
        assert_eq!(exp(ln(x())), x());
        assert_eq!(ln(exp(x())), x());
    }

    #[test]
    fn square_roots_pull_out_square_factors() {
        let sqrt = |n: i64, d: i64| {
            Term::fun(
                Fun::Sqrt,
                vec![Term::Num(BigRational::new(
                    BigInt::from(n),
                    BigInt::from(d),
                ))],
            )
        };
        let root2 = Term::Fun(Fun::Sqrt, vec![Term::int(2)]);
        assert_eq!(sqrt(8, 1), Term::mul(Term::int(2), root2.clone()));
        assert_eq!(
            sqrt(1, 8),
            Term::mul(
                Term::Num(BigRational::new(BigInt::from(1), BigInt::from(4))),
                root2.clone()
            )
        );
        assert_eq!(sqrt(1, 4), half());
        assert_eq!(sqrt(2, 1), root2);
        assert_eq!(sqrt(12, 1).to_string(), "2*sqrt(3)");
        assert_eq!(sqrt(-8, 1).to_string(), "sqrt(-8)");
    }

    #[test]
    fn perfect_roots_are_exact_above_2_pow_53() {
        let n: BigInt = "9007199254740993".parse().unwrap();
        assert_eq!(perfect_root(&(&n * &n), 2), Some(n.clone()));
        assert_eq!(perfect_root(&(&n * &n + 1), 2), None);
        assert_eq!(perfect_root(&(-(&n * &n * &n)), 3), Some(-n));
        assert_eq!(perfect_root(&BigInt::from(-4), 2), None);
    }

    #[test]
    fn sums_print_by_descending_degree_and_half_powers_as_roots() {
        let x = || sym("x");
        let r = |n: i64, d: i64| Term::Num(BigRational::new(BigInt::from(n), BigInt::from(d)));
        let series = Term::Add(vec![
            Term::mul(
                r(1, 120),
                Term::pow(x(), BigRational::from_integer(BigInt::from(5))),
            ),
            Term::mul(
                r(-1, 5040),
                Term::pow(x(), BigRational::from_integer(BigInt::from(7))),
            ),
            Term::mul(
                r(-1, 6),
                Term::pow(x(), BigRational::from_integer(BigInt::from(3))),
            ),
            x(),
        ]);
        assert_eq!(series.to_string(), "-1/5040*x^7 + 1/120*x^5 - 1/6*x^3 + x");
        let root = Term::Pow(
            Box::new(sym("pi")),
            BigRational::new(BigInt::from(1), BigInt::from(2)),
        );
        assert_eq!(root.to_string(), "sqrt(pi)");
        let inverse_root = Term::Pow(
            Box::new(sym("y")),
            BigRational::new(BigInt::from(-1), BigInt::from(2)),
        );
        assert_eq!(inverse_root.to_string(), "1/sqrt(y)");
    }

    #[test]
    fn latex_typesets_fractions_variables_and_functions() {
        let x = || sym("x");
        let y = || sym("y");
        let quotient = Term::mul(
            Term::pow(x(), BigRational::from_integer(BigInt::from(2))),
            Term::pow(
                Term::add(y(), Term::int(1)),
                BigRational::from_integer(BigInt::from(-1)),
            ),
        );
        assert_eq!(term_to_math(&quotient, true), r"\frac{x^{2}}{y + 1}");
        assert_eq!(term_to_math(&quotient, false), "frac(x^(2), y + 1)");
        let mixed = Term::mul(Term::int(3), Term::fun(Fun::Sin, vec![sym("theta")]));
        assert_eq!(term_to_math(&mixed, true), r"3 \sin\left(\theta\right)");
        let half = Term::mul(
            Term::Num(BigRational::new(BigInt::from(-1), BigInt::from(2))),
            Term::pow(sym("velocity"), BigRational::from_integer(BigInt::from(2))),
        );
        assert_eq!(
            term_to_math(&half, true),
            r"-\frac{\mathrm{velocity}^{2}}{2}"
        );
        let subscript = sym("x_1");
        assert_eq!(term_to_math(&subscript, true), "x_{1}");
        let root = Term::Pow(
            Box::new(x()),
            BigRational::new(BigInt::from(1), BigInt::from(2)),
        );
        assert_eq!(term_to_math(&root, true), r"\sqrt{x}");
        let growth = Term::fun(Fun::Exp, vec![Term::mul(Term::int(-2), sym("t"))]);
        assert_eq!(term_to_math(&growth, true), "e^{-2 t}");
    }

    #[test]
    fn rationals_display_as_fractions_inside_terms() {
        assert_eq!(pi_times(1, 4).to_string(), "1/4*pi");
        assert_eq!(
            Term::Num(BigRational::new(BigInt::from(1), BigInt::from(4))).to_string(),
            "0.25"
        );
    }

    #[test]
    fn differentiation_and_substitution_stay_simplified() {
        let term = Term::sub(
            Term::pow(x(), BigRational::from_integer(BigInt::from(2))),
            x(),
        );
        assert_eq!(
            term.diff("x").expect("differentiates").to_string(),
            "2*x - 1"
        );
        assert_eq!(
            term.substitute("x", &Term::int(3)).expect("substitutes"),
            Term::int(6)
        );
    }

    #[test]
    fn zero_negative_powers_and_rendering() {
        assert_eq!(
            Term::pow(Term::int(0), BigRational::from_integer(BigInt::from(-1))),
            Term::Sym("oo".to_string())
        );
        let div_neg = Term::mul(
            sym("y"),
            Term::pow(
                Term::mul(Term::int(-1), sym("x")),
                BigRational::from_integer(BigInt::from(-1)),
            ),
        );
        assert_eq!(div_neg.to_string(), "y/(-x)");
    }
}
