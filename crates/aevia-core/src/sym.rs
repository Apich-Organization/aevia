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
use num_traits::{One, Signed, ToPrimitive, Zero};
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

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
            Self::Sqrt => "sqrt",
            Self::Exp => "exp",
            Self::Ln => "ln",
            Self::Sin => "sin",
            Self::Cos => "cos",
            Self::Tan => "tan",
            Self::Abs => "abs",
            Self::Ite => "ite",
            Self::Other(name) => name,
        }
    }

    /// The numeric value, when it is defined.
    #[must_use]
    pub fn eval(self, x: f64) -> f64 {
        match self {
            Self::Sqrt => x.sqrt(),
            Self::Exp => x.exp(),
            Self::Ln => x.ln(),
            Self::Sin => x.sin(),
            Self::Cos => x.cos(),
            Self::Tan => x.tan(),
            Self::Abs => x.abs(),
            // `ite` is not a numeric function of one argument; it is only
            // folded by `Term::fun` when the condition is a number.
            Self::Ite => f64::NAN,
            Self::Other(_) => f64::NAN,
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
    pub fn foreign(sym: Sym, text: impl Into<Rc<str>>) -> Self {
        Self::Foreign {
            sym,
            text: text.into(),
        }
    }

    /// The kernel handle, when this term lives there.
    #[must_use]
    pub fn as_foreign(&self) -> Option<Sym> {
        match self {
            Self::Foreign { sym, .. } => Some(*sym),
            _ => None,
        }
    }

    /// True when the term has no free symbols.
    #[must_use]
    pub fn is_constant(&self) -> bool {
        match self {
            Self::Num(_) => true,
            Self::Sym(_) => false,
            Self::Add(items) | Self::Mul(items) => items.iter().all(Self::is_constant),
            Self::Pow(base, _) => base.is_constant(),
            Self::Fun(_, args) => args.iter().all(Self::is_constant),
            Self::Foreign { .. } => self.free_symbols().is_empty(),
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

    fn collect_symbols(&self, out: &mut Vec<String>) -> Result<(), EvalError> {
        match self {
            Self::Num(_) => {}
            Self::Sym(name) => {
                if !out.contains(name) {
                    out.push(name.clone());
                }
            }
            Self::Add(items) | Self::Mul(items) => {
                for item in items {
                    item.collect_symbols(out)?;
                }
            }
            Self::Pow(base, _) => base.collect_symbols(out)?,
            Self::Fun(_, args) => {
                for arg in args {
                    arg.collect_symbols(out)?;
                }
            }
            Self::Foreign { sym, .. } => {
                let names = aevia_rssn::kernel()
                    .try_free_symbols(*sym)
                    .map_err(|failure| crate::kernel::error(&failure))?;
                for name in names {
                    if !out.contains(&name) {
                        out.push(name);
                    }
                }
            }
        }
        Ok(())
    }

    /// Adds two terms, folding constants and collecting like terms.
    #[must_use]
    pub fn add(left: Term, right: Term) -> Term {
        let mut flat = Vec::new();
        flatten_add(left, &mut flat);
        flatten_add(right, &mut flat);
        let mut constant = BigRational::zero();
        let mut groups: Vec<(String, Term, BigRational)> = Vec::new();
        for term in flat {
            if let Term::Num(value) = &term {
                constant += value;
                continue;
            }
            let key = format!("{term:?}");
            if let Some(entry) = groups.iter_mut().find(|(existing, _, _)| *existing == key) {
                entry.2 += BigRational::one();
            } else {
                groups.push((key, term, BigRational::one()));
            }
        }
        let mut items = Vec::new();
        for (_, term, count) in groups {
            if count.is_one() {
                items.push(term);
            } else {
                items.push(Term::Mul(vec![rational(count), term]));
            }
        }
        // The constant goes last so that `x^3 - 3x + 1` reads in the usual
        // order rather than starting with the constant term.
        if !constant.is_zero() {
            items.push(rational(constant));
        }
        match items.len() {
            0 => zero(),
            1 => items.remove(0),
            _ => Term::Add(items),
        }
    }

    /// Multiplies two terms, folding constants and merging equal bases.
    #[must_use]
    pub fn mul(left: Term, right: Term) -> Term {
        let mut flat = Vec::new();
        flatten_mul(left, &mut flat);
        flatten_mul(right, &mut flat);
        let mut constant = BigRational::one();
        let mut factors: Vec<(String, Term, BigRational)> = Vec::new();
        for term in flat {
            match term {
                Term::Num(value) => constant *= value,
                Term::Pow(base, exponent) => {
                    let key = format!("{base:?}");
                    if let Some(entry) =
                        factors.iter_mut().find(|(existing, _, _)| *existing == key)
                    {
                        entry.2 += exponent;
                    } else {
                        factors.push((key, *base, exponent));
                    }
                }
                other => {
                    let key = format!("{other:?}");
                    if let Some(entry) =
                        factors.iter_mut().find(|(existing, _, _)| *existing == key)
                    {
                        entry.2 += BigRational::one();
                    } else {
                        factors.push((key, other, BigRational::one()));
                    }
                }
            }
        }
        if constant.is_zero() {
            return zero();
        }
        let mut items = Vec::new();
        if !constant.is_one() {
            items.push(rational(constant));
        }
        for (_, base, exponent) in factors {
            if exponent.is_zero() {
                continue;
            }
            items.push(Term::pow(base, exponent));
        }
        match items.len() {
            0 => one(),
            1 => items.remove(0),
            _ => Term::Mul(items),
        }
    }

    /// Raises a term to a rational power.
    #[must_use]
    pub fn pow(base: Term, exponent: BigRational) -> Term {
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
            if exponent.denom().is_one() {
                if let Some(power) = exponent.numer().to_i32() {
                    return rational(aevia_dim::rat::pow_int(value, power));
                }
            }
            // `u^(1/2)` is `sqrt(u)`, so a fractional power and a square root
            // are the same term and `sqrt(2)^2` folds (§6.3.1).
            if exponent == BigRational::new(BigInt::one(), BigInt::from(2)) {
                return Term::fun(Fun::Sqrt, vec![base]);
            }
        }
        // `sqrt(u)^(2k) = u^k`, so `sqrt(2)^2` is exactly `2` and not a float
        // that happens to print as `2` (§6.3.1).
        if let Term::Fun(Fun::Sqrt, args) = &base {
            if let (Some(inner), true) = (args.first(), exponent.denom().is_one()) {
                if let Some(power) = exponent.numer().to_i32() {
                    if power % 2 == 0 {
                        return Term::pow(
                            inner.clone(),
                            BigRational::from_integer(BigInt::from(power / 2)),
                        );
                    }
                }
            }
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
    pub fn sub(left: Term, right: Term) -> Term {
        Term::add(left, Term::neg(right))
    }

    /// Builds a function application, folding only what is exact.
    ///
    /// `sqrt(2)` must stay `sqrt(2)` (§6.3.1): folding it through `f64` would
    /// answer with `1.4142135623730951`, a `Float` the program never asked
    /// for, and `sqrt(2)^2` would then be `2.0000000000000004`. Only an exact
    /// result is folded here; everything else is a term.
    #[must_use]
    pub fn fun(function: Fun, args: Vec<Term>) -> Term {
        if let Fun::Ite = function {
            if args.len() == 3 {
                if let Term::Num(condition) = &args[0] {
                    return if condition.is_zero() {
                        args[2].clone()
                    } else {
                        args[1].clone()
                    };
                }
            }
            return Term::Fun(function, args);
        }
        if args.len() == 1 {
            if let Term::Num(value) = &args[0] {
                if let Some(result) = exact_function(function, value) {
                    return rational(result);
                }
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
    pub fn substitute(&self, symbol: &str, value: &Term) -> Result<Term, EvalError> {
        Ok(match self {
            Self::Sym(name) if name == symbol => value.clone(),
            Self::Num(_) | Self::Sym(_) => self.clone(),
            Self::Add(items) => items
                .iter()
                .map(|item| item.substitute(symbol, value))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .reduce(Term::add)
                .unwrap_or_else(zero),
            Self::Mul(items) => items
                .iter()
                .map(|item| item.substitute(symbol, value))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .reduce(Term::mul)
                .unwrap_or_else(one),
            Self::Pow(base, exponent) => {
                Term::pow(base.substitute(symbol, value)?, exponent.clone())
            }
            Self::Fun(function, args) => Term::fun(
                *function,
                args.iter()
                    .map(|arg| arg.substitute(symbol, value))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            // The kernel owns the structure, so ask it.
            Self::Foreign { .. } => {
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
            }
        })
    }

    /// Differentiates with respect to a symbol.
    ///
    /// # Errors
    ///
    /// A kernel handle the bridge cannot differentiate is a diagnostic. It
    /// must never become `0`: a wrong derivative is worse than a failure.
    pub fn diff(&self, symbol: &str) -> Result<Term, EvalError> {
        Ok(match self {
            Self::Num(_) => zero(),
            Self::Sym(name) => {
                if name == symbol {
                    one()
                } else {
                    zero()
                }
            }
            Self::Add(items) => items
                .iter()
                .map(|item| item.diff(symbol))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .reduce(Term::add)
                .unwrap_or_else(zero),
            Self::Mul(items) => {
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
            }
            Self::Pow(base, exponent) => {
                let derivative = base.diff(symbol)?;
                if derivative == zero() {
                    return Ok(zero());
                }
                let lowered = Term::pow((**base).clone(), exponent - BigRational::one());
                let coefficient = rational(exponent.clone());
                Term::mul(Term::mul(coefficient, lowered), derivative)
            }
            Self::Fun(function, args) => {
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
                    Fun::Sin => Term::fun(Fun::Cos, vec![arg.clone()]),
                    Fun::Cos => Term::neg(Term::fun(Fun::Sin, vec![arg.clone()])),
                    Fun::Exp => Term::fun(Fun::Exp, vec![arg.clone()]),
                    Fun::Ln => Term::pow(arg.clone(), BigRational::from_integer(BigInt::from(-1))),
                    Fun::Sqrt => {
                        // d/du sqrt(u) = 1 / (2 sqrt(u))
                        let two = Term::Num(BigRational::from_integer(BigInt::from(2)));
                        let root = Term::fun(Fun::Sqrt, vec![arg.clone()]);
                        Term::pow(
                            Term::mul(two, root),
                            BigRational::from_integer(BigInt::from(-1)),
                        )
                    }
                    Fun::Tan => {
                        let cos = Term::fun(Fun::Cos, vec![arg.clone()]);
                        Term::pow(cos, BigRational::from_integer(BigInt::from(-2)))
                    }
                    // |u| is differentiated formally as sign(u) = |u| / u.
                    Fun::Abs => Term::mul(
                        Term::fun(Fun::Abs, vec![arg.clone()]),
                        Term::pow(arg.clone(), BigRational::from_integer(BigInt::from(-1))),
                    ),
                    Fun::Ite => unreachable!("handled above"),
                    Fun::Other(name) => {
                        return Err(EvalError::new(
                            "E1001",
                            format!("differentiating `{name}` needs the kernel"),
                        ))
                    }
                };
                Term::mul(outer, inner)
            }
            Self::Foreign { .. } => {
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
            }
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
    pub fn eval(&self, bindings: &HashMap<String, f64>) -> Result<f64, String> {
        match self {
            Self::Num(value) => Ok(aevia_dim::rat::to_f64(value)),
            Self::Sym(name) => match bindings.get(name).copied() {
                Some(value) => Ok(value),
                // The mathematical constants are part of the prelude; a term
                // that only mentions them is numeric.
                None if name == "pi" => Ok(std::f64::consts::PI),
                None if name == "e" => Ok(std::f64::consts::E),
                None => Err(name.clone()),
            },
            Self::Add(items) => {
                let mut total = 0.0;
                for item in items {
                    total += item.eval(bindings)?;
                }
                Ok(total)
            }
            Self::Mul(items) => {
                let mut total = 1.0;
                for item in items {
                    total *= item.eval(bindings)?;
                }
                Ok(total)
            }
            Self::Pow(base, exponent) => {
                let base = base.eval(bindings)?;
                let exponent = aevia_dim::rat::to_f64(exponent);
                Ok(base.powf(exponent))
            }
            Self::Fun(function, args) => {
                let value = args
                    .first()
                    .map_or(Ok(f64::NAN), |arg| arg.eval(bindings))?;
                Ok(function.eval(value))
            }
            Self::Foreign { .. } => {
                crate::kernel::eval_term(self, bindings).ok_or_else(|| "a kernel term".to_string())
            }
        }
    }
}

fn flatten_add(term: Term, out: &mut Vec<Term>) {
    match term {
        Term::Add(items) => {
            for item in items {
                flatten_add(item, out);
            }
        }
        other => out.push(other),
    }
}

fn flatten_mul(term: Term, out: &mut Vec<Term>) {
    match term {
        Term::Mul(items) => {
            for item in items {
                flatten_mul(item, out);
            }
        }
        other => out.push(other),
    }
}

fn exact_function(function: Fun, value: &BigRational) -> Option<BigRational> {
    match function {
        Fun::Sqrt if value.is_positive() => {
            let numerator = value.numer().clone();
            let denominator = value.denom().clone();
            let root_num = perfect_root(&numerator, 2)?;
            let root_den = perfect_root(&denominator, 2)?;
            Some(BigRational::new(root_num, root_den))
        }
        Fun::Abs => Some(value.abs()),
        Fun::Exp if value.is_zero() => Some(BigRational::one()),
        Fun::Ln if value.is_one() => Some(BigRational::zero()),
        _ => None,
    }
}

fn perfect_root(value: &BigInt, degree: u32) -> Option<BigInt> {
    let approximate = value.to_f64()?;
    let root = approximate.abs().powf(1.0 / f64::from(degree));
    let candidate = BigInt::from(root.round() as i64);
    if candidate.pow(degree) == *value {
        Some(candidate)
    } else {
        None
    }
}

impl fmt::Display for Term {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.render(f, false)
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

    fn render(&self, f: &mut dyn fmt::Write, repr: bool) -> fmt::Result {
        match self {
            Self::Num(value) => {
                if repr {
                    f.write_str(&aevia_dim::rat::format_rational(value))
                } else {
                    f.write_str(&format_rational_decimal(value))
                }
            }
            // `symbol("x")` allocates `x`, `x#2`, ... so that two calls are
            // different symbols (§6.3.6); only that numeric suffix is hidden.
            // A user's `symbol("a#b")` keeps its name.
            Self::Sym(name) => {
                if repr {
                    f.write_str(name)
                } else {
                    f.write_str(display_name(name))
                }
            }
            Self::Add(items) => {
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
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
            }
            Self::Mul(items) => {
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
            }
            Self::Pow(base, exponent) => {
                let needs_parens = matches!(**base, Self::Add(_) | Self::Mul(_));
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
            }
            Self::Fun(function, args) => {
                write!(f, "{}(", function.as_str())?;
                for (index, arg) in args.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    arg.render(f, repr)?;
                }
                f.write_str(")")
            }
            Self::Foreign { text, .. } => f.write_str(text),
        }
    }
}

/// A `fmt::Write` that appends to a `String`.
struct ReprFormatter<'a>(&'a mut String);

impl fmt::Write for ReprFormatter<'_> {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.0.push_str(text);
        Ok(())
    }
}

/// Renders a term in LaTeX (`latex`) or Typst syntax.
#[must_use]
pub fn term_to_math(term: &Term, latex: bool) -> String {
    fn render(term: &Term, latex: bool, parent_prec: u8) -> String {
        // 2 = sum, 1 = product, 0 = atom.
        let (text, prec) = match term {
            Term::Num(value) => {
                if value.denom().is_one() {
                    (value.numer().to_string(), 0)
                } else if latex {
                    (
                        format!("\\frac{{{}}}{{{}}}", value.numer(), value.denom()),
                        0,
                    )
                } else {
                    (format!("frac({}, {})", value.numer(), value.denom()), 0)
                }
            }
            Term::Sym(name) => {
                let name = display_name(name);
                if latex {
                    (format!("\\mathrm{{{name}}}"), 0)
                } else {
                    (format!("italic({name})"), 0)
                }
            }
            Term::Add(items) => {
                let mut out = String::new();
                for (index, item) in items.iter().enumerate() {
                    if index == 0 {
                        out.push_str(&render(item, latex, 2));
                    } else if is_negative(item) {
                        out.push_str(" - ");
                        out.push_str(&render(&absolute(item), latex, 2));
                    } else {
                        out.push_str(" + ");
                        out.push_str(&render(item, latex, 2));
                    }
                }
                (out, 2)
            }
            Term::Mul(items) => {
                let mut out = String::new();
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push_str(if latex { " \\cdot " } else { " dot " });
                    }
                    out.push_str(&render(item, latex, 1));
                }
                (out, 1)
            }
            Term::Pow(base, exponent) => {
                let base_text = render(base, latex, 0);
                let exponent_text = if exponent.denom().is_one() {
                    exponent.numer().to_string()
                } else if latex {
                    format!("\\frac{{{}}}{{{}}}", exponent.numer(), exponent.denom())
                } else {
                    format!("frac({}, {})", exponent.numer(), exponent.denom())
                };
                let body = if latex {
                    format!("{base_text}^{{{exponent_text}}}")
                } else {
                    format!("{base_text}^({exponent_text})")
                };
                (body, 0)
            }
            Term::Fun(function, args) => {
                let rendered: Vec<String> = args.iter().map(|arg| render(arg, latex, 0)).collect();
                let name = function.as_str();
                match function {
                    Fun::Sqrt if args.len() == 1 => {
                        let body = if latex {
                            format!("\\sqrt{{{}}}", rendered[0])
                        } else {
                            format!("sqrt({})", rendered[0])
                        };
                        (body, 0)
                    }
                    Fun::Ite if args.len() == 3 => {
                        let body = if latex {
                            format!(
                                "\\begin{{cases}} {} & {} \\\\ {} & \\text{{otherwise}} \\end{{cases}}",
                                rendered[1], rendered[0], rendered[2]
                            )
                        } else {
                            format!("cases({} & {}, {} & otherwise)", rendered[1], rendered[0], rendered[2])
                        };
                        (body, 0)
                    }
                    _ => {
                        let call = format!("{name}({})", rendered.join(", "));
                        if latex {
                            (format!("\\operatorname{{{name}}}{call}"), 0)
                        } else {
                            (format!("{name}{call}"), 0)
                        }
                    }
                }
            }
            Term::Foreign { text, .. } => (text.to_string(), 0),
        };
        if prec > parent_prec {
            if latex {
                format!("\\left({text}\\right)")
            } else {
                format!("({text})")
            }
        } else {
            text
        }
    }
    // At the top level nothing needs parentheses: the loosest operator (a
    // sum, precedence 2) is allowed.
    render(term, latex, 2)
}

/// The name a symbol prints as.
fn display_name(name: &str) -> &str {
    match name.rsplit_once('#') {
        Some((base, suffix)) if !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()) => {
            base
        }
        _ => name,
    }
}

fn is_negative(term: &Term) -> bool {
    match term {
        Term::Num(value) => value.is_negative(),
        Term::Mul(factors) => factors
            .first()
            .is_some_and(|first| matches!(first, Term::Num(value) if value.is_negative())),
        _ => false,
    }
}

fn absolute(term: &Term) -> Term {
    match term {
        Term::Num(value) => Term::Num(value.abs()),
        Term::Mul(factors) => {
            let mut factors = factors.clone();
            if let Some(Term::Num(value)) = factors.first_mut() {
                *value = value.abs();
            }
            if factors.len() == 1 {
                factors.remove(0)
            } else {
                Term::Mul(factors)
            }
        }
        other => other.clone(),
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
            Term::add(
                Term::mul(Term::int(-3), x()),
                Term::int(1),
            ),
        );
        let derivative = term.diff("x").expect("differentiates");
        assert_eq!(derivative.to_string(), "3*x^2 - 3");
    }

    #[test]
    fn substitution_is_structural() {
        let term = Term::add(Term::Pow(Box::new(x()), BigRational::from_integer(BigInt::from(2))), Term::int(1));
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
}
