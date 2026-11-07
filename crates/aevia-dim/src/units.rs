//! The unit registry: exact scales, SI prefixes, affine units.
//!
//! A unit contributes three things and nothing else (spec §5.1): a dimension,
//! an exact scale relative to the coherent SI unit, and an optional affine
//! offset. Units never enter a type, so `3 km` and `3000 m` have the same type
//! and the same value; only the display unit differs.
//!
//! The registry is the single source of truth for `Units.SI`; the type checker
//! and the runtime both read it. User `unit` declarations are inserted into the
//! same table, which is why `unit EUR : Currency = 1.08 USD` behaves exactly
//! like a built-in unit.

use crate::dim::Dim;
use crate::dim::DimEnv;
use crate::rat::format_rational;
use crate::rat::parse_decimal;
use crate::rat::pow_int;
use crate::rat::ratio;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::One;
use num_traits::ToPrimitive;
use num_traits::Zero;
use std::collections::HashMap;
use std::fmt;

/// An exact scale: `rat * pi^pi_exp`.
///
/// `pi` is kept as a separate exponent so that `deg` (`pi/180`) stays exact
/// and `30 deg` can print as `pi/6` rather than a float.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scale {
    /// The rational factor.
    pub rat: BigRational,
    /// The power of pi multiplying the rational factor.
    pub pi: i32,
}

impl Scale {
    /// The scale `1`.
    #[must_use]
    pub fn one() -> Self {
        Self {
            rat: BigRational::one(),
            pi: 0,
        }
    }

    /// A rational scale without a factor of pi.
    #[must_use]
    pub fn rational(rat: BigRational) -> Self {
        Self { rat, pi: 0 }
    }

    /// Parses `1`, `1/1000`, `0.3048`, `pi`, `pi/180`, `-2.5`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if let Some(rest) = text.strip_prefix("pi") {
            let rest = rest.trim();
            if rest.is_empty() {
                return Some(Self {
                    rat: BigRational::one(),
                    pi: 1,
                });
            }
            let rest = rest.strip_prefix('/')?;
            let denominator = parse_decimal(rest)?;
            if denominator.is_zero() {
                return None;
            }
            return Some(Self {
                rat: denominator.recip(),
                pi: 1,
            });
        }
        match text.split_once('/') {
            | Some((numerator, denominator)) => {
                let numerator = parse_decimal(numerator)?;
                let denominator = parse_decimal(denominator)?;
                if denominator.is_zero() {
                    return None;
                }
                Some(Self::rational(numerator / denominator))
            },
            | None => Some(Self::rational(parse_decimal(text)?)),
        }
    }

    /// Multiplies two scales.
    #[must_use]
    pub fn mul(
        &self,
        other: &Self,
    ) -> Self {
        Self {
            rat: &self.rat * &other.rat,
            pi: self.pi + other.pi,
        }
    }

    /// Divides two scales.
    #[must_use]
    pub fn div(
        &self,
        other: &Self,
    ) -> Self {
        Self {
            rat: &self.rat / &other.rat,
            pi: self.pi - other.pi,
        }
    }

    /// Raises the scale to a rational power, when the result stays exact.
    ///
    /// `(m^2)^(1/2)` is exact; `(km)^(1/2)` is `sqrt(1000)` and has no exact
    /// rational form, so it is rejected (the unit expression is ill-formed
    /// rather than silently approximated).
    #[must_use]
    pub fn pow(
        &self,
        exponent: &BigRational,
    ) -> Option<Self> {
        if exponent.is_zero() {
            return Some(Self::one());
        }
        if exponent.denom().is_one() {
            let power = exponent.numer().to_i32()?;
            if power.unsigned_abs() > 1024 {
                return None;
            }
            return Some(Self {
                rat: pow_int(&self.rat, power),
                pi: self.pi.checked_mul(power)?,
            });
        }
        // Fractional exponent: only exact when the rational is a perfect power
        // and pi is absent (or the pi exponent also divides evenly).
        if self.pi != 0 {
            return None;
        }
        if self.rat.is_one() {
            return Some(Self::one());
        }
        let degree = exponent.denom().to_u32()?;
        let numerator = self.rat.numer().clone().nth_root(degree);
        let denominator = self.rat.denom().clone().nth_root(degree);
        let candidate = BigRational::new(numerator, denominator);
        if candidate.pow(i32::try_from(degree).unwrap_or(i32::MAX)) != self.rat {
            return None;
        }
        Some(Self::rational(pow_int(
            &candidate,
            exponent.numer().to_i32()?,
        )))
    }

    /// The numeric value of the scale, for kernels and printing.
    #[must_use]
    pub fn to_f64(&self) -> f64 {
        let base = self.rat.to_f64().unwrap_or(f64::NAN);
        if self.pi == 0 {
            base
        } else {
            base * std::f64::consts::PI.powi(self.pi)
        }
    }

    /// True when the scale is exactly one.
    #[must_use]
    pub fn is_one(&self) -> bool {
        self.pi == 0 && self.rat.is_one()
    }

    /// True when the scale is exactly zero.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.rat.is_zero()
    }
}

impl fmt::Display for Scale {
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        if self.pi == 0 {
            f.write_str(&format_rational(&self.rat))
        } else if self.rat.is_one() {
            match self.pi {
                | 1 => f.write_str("pi"),
                | _ => write!(f, "pi^{}", self.pi),
            }
        } else {
            write!(f, "{}*pi^{}", format_rational(&self.rat), self.pi)
        }
    }
}

/// One unit definition.
#[derive(Clone, Debug)]
pub struct UnitDef {
    /// The canonical spelling, e.g. `m`.
    pub name: String,
    /// Alternative spellings that resolve to the same unit, e.g. `meter`.
    pub aliases: Vec<String>,
    /// The dimension the unit measures.
    pub dim: Dim,
    /// The exact scale relative to the coherent SI unit of `dim`.
    pub scale: Scale,
    /// The affine offset in coherent units, for `°C` and `°F`.
    pub offset: Option<Scale>,
    /// Whether SI prefixes may be applied.
    pub prefixable: bool,
    /// Whether the unit is affine (an offset scale).
    pub affine: bool,
    /// The spelling used when printing.
    pub display: String,
    /// A coarse grouping such as `si-base` or `imperial`.
    pub category: String,
}

impl UnitDef {
    /// Builds a linear unit.
    #[must_use]
    pub fn linear(
        name: &str,
        dim: Dim,
        scale: Scale,
    ) -> Self {
        Self {
            name: name.to_string(),
            aliases: Vec::new(),
            dim,
            scale,
            offset: None,
            prefixable: false,
            affine: false,
            display: name.to_string(),
            category: String::new(),
        }
    }

    /// Adds an alias.
    #[must_use]
    pub fn with_alias(
        mut self,
        alias: &str,
    ) -> Self {
        self.aliases.push(alias.to_string());
        self
    }

    /// Marks the unit as prefixable.
    #[must_use]
    pub fn prefixable(mut self) -> Self {
        self.prefixable = true;
        self
    }

    /// Marks the unit as affine with the given offset.
    #[must_use]
    pub fn affine(
        mut self,
        offset: Scale,
    ) -> Self {
        self.affine = true;
        self.offset = Some(offset);
        self
    }

    /// Sets the category.
    #[must_use]
    pub fn category(
        mut self,
        category: &str,
    ) -> Self {
        self.category = category.to_string();
        self
    }

    /// The coherent-SI value of `amount` of this unit.
    #[must_use]
    pub fn to_coherent(
        &self,
        amount: f64,
    ) -> f64 {
        let scaled = amount * self.scale.to_f64();
        match &self.offset {
            | Some(offset) => scaled + offset.to_f64(),
            | None => scaled,
        }
    }
}

/// A unit expression as written after a numeric literal.
///
/// The parser produces this; resolution against a [`UnitTable`] happens during
/// name resolution, which is why the parser itself never needs a symbol table.
#[derive(Clone, Debug, PartialEq)]
pub enum UnitExpr {
    /// A single unit name, possibly qualified (`Units.SI.km`).
    Name(String),
    /// `%`.
    Percent,
    /// `a*b`.
    Mul(Box<UnitExpr>, Box<UnitExpr>),
    /// `a/b`.
    Div(Box<UnitExpr>, Box<UnitExpr>),
    /// `a^n` or `a²`.
    Pow(Box<UnitExpr>, BigRational),
    /// `(a)`.
    Paren(Box<UnitExpr>),
}

impl UnitExpr {
    /// Renders the expression back to source form, for diagnostics.
    #[must_use]
    pub fn to_source(&self) -> String {
        match self {
            | Self::Name(name) => name.clone(),
            | Self::Percent => "%".to_string(),
            | Self::Mul(left, right) => format!("{}*{}", left.to_source(), right.to_source()),
            | Self::Div(left, right) => format!("{}/{}", left.to_source(), right.to_source()),
            | Self::Pow(base, exponent) => {
                if exponent.denom().is_one() {
                    format!("{}^{}", base.to_source(), exponent.numer())
                } else {
                    format!("{}^({})", base.to_source(), format_rational(exponent))
                }
            },
            | Self::Paren(inner) => format!("({})", inner.to_source()),
        }
    }
}

/// A resolved unit expression.
#[derive(Clone, Debug)]
pub struct ResolvedUnit {
    /// The canonical spelling, or the composed expression text.
    pub name: String,
    /// The dimension of the unit.
    pub dim: Dim,
    /// The exact scale relative to coherent SI.
    pub scale: Scale,
    /// The affine offset, when the expression is a single affine unit.
    pub offset: Option<Scale>,
    /// Whether the expression is a single affine unit.
    pub affine: bool,
}

impl ResolvedUnit {
    /// Converts a numeric coefficient in this unit to coherent SI.
    #[must_use]
    pub fn to_coherent(
        &self,
        amount: f64,
    ) -> f64 {
        let scaled = amount * self.scale.to_f64();
        match &self.offset {
            | Some(offset) => scaled + offset.to_f64(),
            | None => scaled,
        }
    }
}

/// Errors raised while resolving unit names and expressions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnitError {
    /// The name is not a unit and no prefix decomposes it.
    Unknown(String),
    /// An affine unit appeared in a product, quotient or power.
    AffineInProduct(String),
    /// A scale power had no exact rational result.
    InexactPower(String),
    /// Two definitions share a name.
    Duplicate(String),
    /// A prefix was applied to a unit that does not accept prefixes.
    NotPrefixable(String),
}

impl UnitError {
    /// The diagnostic code from appendix B.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            | Self::Unknown(_) => "E0501",
            | Self::AffineInProduct(_) => "E0508",
            | Self::InexactPower(_) => "E0506",
            | Self::Duplicate(_) => "E0505",
            | Self::NotPrefixable(_) => "E0503",
        }
    }
}

impl fmt::Display for UnitError {
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        match self {
            | Self::Unknown(name) => write!(f, "unknown unit `{name}`"),
            | Self::AffineInProduct(name) => {
                write!(
                    f,
                    "affine unit `{name}` cannot appear in a product or power"
                )
            },
            | Self::InexactPower(name) => {
                write!(
                    f,
                    "the scale of `{name}` has no exact value for that exponent"
                )
            },
            | Self::Duplicate(name) => write!(f, "duplicate unit `{name}`"),
            | Self::NotPrefixable(name) => write!(f, "`{name}` does not accept SI prefixes"),
        }
    }
}

impl std::error::Error for UnitError {}

/// The SI prefixes of §5.1.4, longest symbol first so matching is greedy.
const PREFIXES: &[(&str, i32)] = &[
    ("da", 1),
    ("Q", 30),
    ("R", 27),
    ("Y", 24),
    ("Z", 21),
    ("E", 18),
    ("P", 15),
    ("T", 12),
    ("G", 9),
    ("M", 6),
    ("k", 3),
    ("h", 2),
    ("d", -1),
    ("c", -2),
    ("m", -3),
    ("µ", -6),
    ("μ", -6),
    ("n", -9),
    ("p", -12),
    ("f", -15),
    ("a", -18),
    ("z", -21),
    ("y", -24),
    ("r", -27),
    ("q", -30),
];

/// The registry of units visible to a compilation.
#[derive(Clone, Debug)]
pub struct UnitTable {
    defs: Vec<UnitDef>,
    index: HashMap<String, usize>,
}

impl Default for UnitTable {
    fn default() -> Self {
        Self::empty()
    }
}

impl UnitTable {
    /// An empty registry.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            defs: Vec::new(),
            index: HashMap::new(),
        }
    }

    /// A table from a list of definitions.
    ///
    /// This is the constructor the compiler uses: the units come from the
    /// kernel's manifest, not from a list kept here. A duplicate name is
    /// skipped rather than fatal, because a manifest is data and a malformed
    /// one must not abort a check.
    #[must_use]
    pub fn from_defs(defs: impl IntoIterator<Item = UnitDef>) -> Self {
        let mut table = Self::empty();
        for unit in defs {
            let _ = table.insert(unit);
        }
        table
    }

    /// The hand-written SI table of §5.3.
    ///
    /// Kept as a *test fixture*: the compiler builds its table from the Kernel
    /// Manifest with [`UnitTable::from_defs`], and a conformance test in
    /// `aevia-rssn` checks that every unit here also exists there with the
    /// same dimension and scale. Nothing outside tests should call this.
    #[must_use]
    pub fn si() -> Self {
        let mut table = Self::empty();
        let env = DimEnv::si();
        let dim = |text: &str| {
            env.parse(text)
                .unwrap_or_else(|error| panic!("builtin unit dimension `{text}`: {error}"))
        };
        let scale = |text: &str| {
            Scale::parse(text)
                .unwrap_or_else(|| panic!("builtin unit scale `{text}` is not a number"))
        };
        let mut add = |unit: UnitDef| {
            table
                .insert(unit)
                .expect("builtin unit table has unique names");
        };

        // §5.3.1 — SI base units.
        add(UnitDef::linear("m", dim("L"), scale("1"))
            .with_alias("meter")
            .with_alias("metre")
            .prefixable()
            .category("si-base"));
        add(UnitDef::linear("kg", dim("M"), scale("1")).category("si-base"));
        add(UnitDef::linear("g", dim("M"), scale("1/1000"))
            .with_alias("gram")
            .prefixable()
            .category("si-base"));
        add(UnitDef::linear("s", dim("T"), scale("1"))
            .with_alias("sec")
            .prefixable()
            .category("si-base"));
        add(UnitDef::linear("A", dim("I"), scale("1"))
            .with_alias("ampere")
            .prefixable()
            .category("si-base"));
        add(UnitDef::linear("K", dim("Θ"), scale("1"))
            .with_alias("kelvin")
            .prefixable()
            .category("si-base"));
        add(UnitDef::linear("mol", dim("Amt"), scale("1"))
            .prefixable()
            .category("si-base"));
        add(UnitDef::linear("cd", dim("Lum"), scale("1"))
            .with_alias("candela")
            .prefixable()
            .category("si-base"));

        // §5.3.2 — SI derived units (scale 1 in coherent units).
        for (name, alias, dimension) in [
            ("Hz", Some("hertz"), "1/T"),
            ("N", Some("newton"), "M*L/T^2"),
            ("Pa", Some("pascal"), "M/(L*T^2)"),
            ("J", Some("joule"), "M*L^2/T^2"),
            ("W", Some("watt"), "M*L^2/T^3"),
            ("C", Some("coulomb"), "I*T"),
            ("V", Some("volt"), "M*L^2/(I*T^3)"),
            ("F", Some("farad"), "I^2*T^4/(M*L^2)"),
            ("Ω", Some("ohm"), "M*L^2/(I^2*T^3)"),
            ("S", Some("siemens"), "I^2*T^3/(M*L^2)"),
            ("Wb", Some("weber"), "M*L^2/(I*T^2)"),
            ("T", Some("tesla"), "M/(I*T^2)"),
            ("H", Some("henry"), "M*L^2/(I^2*T^2)"),
            ("lm", Some("lumen"), "Lum"),
            ("lx", Some("lux"), "Lum/L^2"),
            ("Bq", Some("becquerel"), "1/T"),
            ("Gy", Some("gray"), "L^2/T^2"),
            ("Sv", Some("sievert"), "L^2/T^2"),
            ("kat", Some("katal"), "Amt/T"),
            ("rad", Some("radian"), "1"),
            ("sr", Some("steradian"), "1"),
        ] {
            let mut unit = UnitDef::linear(name, dim(dimension), scale("1"))
                .category("si-derived")
                .prefixable();
            if let Some(alias) = alias {
                unit = unit.with_alias(alias);
            }
            add(unit);
        }

        // §5.3.3 — common non-SI units.
        for (name, dimension, value, category) in [
            ("min", "T", "60", "time"),
            ("h", "T", "3600", "time"),
            ("d", "T", "86400", "time"),
            ("deg", "1", "pi/180", "angle"),
            ("arcmin", "1", "pi/10800", "angle"),
            ("arcsec", "1", "pi/648000", "angle"),
            ("l", "L^3", "1/1000", "volume"),
            ("t", "M", "1000", "mass"),
            ("eV", "M*L^2/T^2", "1.602176634e-19", "energy"),
            ("Da", "M", "1.66053906660e-27", "mass"),
            ("au", "L", "149597870700", "astronomy"),
            ("pc", "L", "3.0856775814913673e16", "astronomy"),
            ("ly", "L", "9460730472580800", "astronomy"),
            ("Å", "L", "1e-10", "length"),
            ("bar", "M/(L*T^2)", "100000", "pressure"),
            ("atm", "M/(L*T^2)", "101325", "pressure"),
            ("mmHg", "M/(L*T^2)", "133.322387415", "pressure"),
            ("cal", "M*L^2/T^2", "4.184", "energy"),
            ("kWh", "M*L^2/T^2", "3.6e6", "energy"),
            ("hp", "M*L^2/T^3", "745.69987158227022", "power"),
            ("kn", "L/T", "1852/3600", "velocity"),
            ("mph", "L/T", "1609.344/3600", "velocity"),
            ("ft", "L", "0.3048", "imperial"),
            ("in", "L", "0.0254", "imperial"),
            ("yd", "L", "0.9144", "imperial"),
            ("mi", "L", "1609.344", "imperial"),
            ("nmi", "L", "1852", "imperial"),
            ("acre", "L^2", "4046.8564224", "imperial"),
            ("gal", "L^3", "0.003785411784", "imperial"),
            ("oz", "M", "0.028349523125", "imperial"),
            ("lb", "M", "0.45359237", "imperial"),
            ("psi", "M/(L*T^2)", "6894.757293168", "imperial"),
            ("bit", "1", "1", "information"),
            ("B", "1", "8", "information"),
            ("KiB", "1", "8192", "information"),
        ] {
            add(UnitDef::linear(name, dim(dimension), scale(value)).category(category));
        }

        // §5.3.3 — affine temperature scales.
        add(UnitDef::linear("°C", dim("Θ"), scale("1"))
            .with_alias("degC")
            .with_alias("celsius")
            .affine(scale("273.15"))
            .category("temperature"));
        add(UnitDef::linear("°F", dim("Θ"), scale("5/9"))
            .with_alias("degF")
            .with_alias("fahrenheit")
            .affine(scale("255.3722222222222"))
            .category("temperature"));

        drop(add);

        // Units outside the coherent set that SI nevertheless prefixes.
        for name in [
            "eV", "bar", "cal", "B", "bit", "l", "t", "Da", "pc", "au", "ly", "Å",
        ] {
            if let Some(index) = table.index.get(name).copied() {
                table.defs[index].prefixable = true;
            }
        }

        // Aliases for the non-SI spellings the spec lists explicitly.
        for (alias, canonical) in [
            ("°", "deg"),
            ("degree", "deg"),
            ("′", "arcmin"),
            ("″", "arcsec"),
            ("liter", "l"),
            ("litre", "l"),
            ("angstrom", "Å"),
            ("u", "Da"),
            ("byte", "B"),
        ] {
            if let Some(index) = table.index.get(canonical).copied() {
                let mut unit = table.defs[index].clone();
                unit.aliases.push(alias.to_string());
                table.defs[index] = unit.clone();
                table.index.insert(alias.to_string(), index);
            }
        }

        table
    }

    /// Inserts a unit definition.
    ///
    /// # Errors
    ///
    /// Returns [`UnitError::Duplicate`] when the name or an alias is taken.
    pub fn insert(
        &mut self,
        unit: UnitDef,
    ) -> Result<(), UnitError> {
        if let Some(existing) = self.index.get(&unit.name) {
            let existing_name = self.defs[*existing].name.clone();
            return Err(UnitError::Duplicate(existing_name));
        }
        for alias in &unit.aliases {
            if let Some(existing) = self.index.get(alias) {
                let existing_name = self.defs[*existing].name.clone();
                return Err(UnitError::Duplicate(existing_name));
            }
        }
        let position = self.defs.len();
        self.index.insert(unit.name.clone(), position);
        for alias in &unit.aliases {
            self.index.insert(alias.clone(), position);
        }
        self.defs.push(unit);
        Ok(())
    }

    /// All definitions in insertion order.
    #[must_use]
    pub fn list(&self) -> &[UnitDef] {
        &self.defs
    }

    /// Looks a unit up by name, resolving SI prefixes when needed.
    ///
    /// Exact names win over prefixed readings: `kg` is the base unit, not
    /// `k`+`g`, and `min` is minute, not `m`+`in` (§5.1.4).
    #[must_use]
    pub fn lookup(
        &self,
        name: &str,
    ) -> Option<UnitDef> {
        if let Some(index) = self.index.get(name) {
            return Some(self.defs[*index].clone());
        }
        for (prefix, exponent) in PREFIXES {
            let Some(rest) = name.strip_prefix(*prefix) else {
                continue;
            };
            if rest.is_empty() {
                continue;
            }
            let Some(index) = self.index.get(rest) else {
                continue;
            };
            let base = &self.defs[*index];
            if !base.prefixable {
                continue;
            }
            // Stacked prefixes are impossible here: `rest` must be an exact
            // unit name, and a prefixed spelling such as `km` is not in the
            // index, so `kkm` fails the lookup above.
            let factor = Scale::rational(pow_int(
                &BigRational::from_integer(BigInt::from(10)),
                *exponent,
            ));
            return Some(UnitDef {
                name: name.to_string(),
                aliases: Vec::new(),
                dim: base.dim.clone(),
                scale: base.scale.mul(&factor),
                offset: None,
                prefixable: false,
                affine: false,
                display: name.to_string(),
                category: base.category.clone(),
            });
        }
        None
    }

    /// True when the name resolves to a unit.
    #[must_use]
    pub fn contains(
        &self,
        name: &str,
    ) -> bool {
        self.lookup(name).is_some()
    }

    /// Resolves a single name to a resolved unit, keeping affine information.
    ///
    /// # Errors
    ///
    /// Returns [`UnitError::Unknown`] for an unregistered name.
    pub fn resolve_name(
        &self,
        name: &str,
    ) -> Result<ResolvedUnit, UnitError> {
        // §1.8.2: a unit may be written with its module path, `3 Units.SI.km`.
        // The unit table is one namespace shared by every `Units.*` module,
        // so the path selects nothing further; it must still name a units
        // module, otherwise `3 p.x` would be a unit literal.
        let local = match name.rsplit_once('.') {
            | Some((module, unit)) if module == "Units" || module.starts_with("Units.") => unit,
            | _ => name,
        };
        let unit = self
            .lookup(local)
            .ok_or_else(|| UnitError::Unknown(name.to_string()))?;
        Ok(ResolvedUnit {
            name: unit.display.clone(),
            dim: unit.dim,
            scale: unit.scale,
            offset: unit.offset,
            affine: unit.affine,
        })
    }

    /// Resolves a unit expression to a dimension and an exact scale.
    ///
    /// # Errors
    ///
    /// Reports unknown names, affine units in products (§5.6.5) and scales
    /// whose rational power is not exact.
    pub fn resolve(
        &self,
        expr: &UnitExpr,
    ) -> Result<ResolvedUnit, UnitError> {
        match expr {
            | UnitExpr::Name(name) => self.resolve_name(name),
            | UnitExpr::Percent => {
                Ok(ResolvedUnit {
                    name: "%".to_string(),
                    dim: Dim::one(),
                    scale: Scale::rational(ratio(1, 100)),
                    offset: None,
                    affine: false,
                })
            },
            | UnitExpr::Paren(inner) => self.resolve(inner),
            | UnitExpr::Mul(left, right) => {
                let left = self.resolve(left)?;
                let right = self.resolve(right)?;
                reject_affine(&left)?;
                reject_affine(&right)?;
                Ok(ResolvedUnit {
                    name: format!("{}*{}", left.name, right.name),
                    dim: left.dim.mul(&right.dim),
                    scale: left.scale.mul(&right.scale),
                    offset: None,
                    affine: false,
                })
            },
            | UnitExpr::Div(left, right) => {
                let left = self.resolve(left)?;
                let right = self.resolve(right)?;
                reject_affine(&left)?;
                reject_affine(&right)?;
                Ok(ResolvedUnit {
                    name: format!("{}/{}", left.name, right.name),
                    dim: left.dim.div(&right.dim),
                    scale: left.scale.div(&right.scale),
                    offset: None,
                    affine: false,
                })
            },
            | UnitExpr::Pow(base, exponent) => {
                let base = self.resolve(base)?;
                reject_affine(&base)?;
                let scale = base
                    .scale
                    .pow(exponent)
                    .ok_or_else(|| UnitError::InexactPower(base.name.clone()))?;
                Ok(ResolvedUnit {
                    name: format!("{}^{}", base.name, format_rational(exponent)),
                    dim: base.dim.pow(exponent),
                    scale,
                    offset: None,
                    affine: false,
                })
            },
        }
    }
}

fn reject_affine(unit: &ResolvedUnit) -> Result<(), UnitError> {
    if unit.affine {
        Err(UnitError::AffineInProduct(unit.name.clone()))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dim::DimEnv;

    fn env() -> DimEnv {
        DimEnv::si()
    }

    #[test]
    fn prefixes_resolve_greedily() {
        let table = UnitTable::si();
        assert_eq!(table.lookup("km").unwrap().scale.to_f64(), 1000.0);
        assert_eq!(table.lookup("dam").unwrap().scale.to_f64(), 10.0);
        assert_eq!(table.lookup("µm").unwrap().scale.to_f64(), 1e-6);
        assert_eq!(table.lookup("ms").unwrap().scale.to_f64(), 1e-3);
        assert_eq!(table.lookup("MHz").unwrap().scale.to_f64(), 1e6);
        assert!(table.lookup("kkm").is_none());
        assert!(table.lookup("nope").is_none());
    }

    #[test]
    fn qualified_unit_names_resolve_through_a_units_module() {
        // §1.8.2: `3 Units.SI.km` is legal; a path that is not a units module
        // is not a unit.
        let table = UnitTable::si();
        let km = table.resolve_name("Units.SI.km").unwrap();
        assert_eq!(km.scale.to_f64(), 1000.0);
        assert_eq!(km.name, table.resolve_name("km").unwrap().name);
        assert!(table.resolve_name("Units.CGS.s").is_ok());
        assert!(matches!(
            table.resolve_name("p.x"),
            Err(UnitError::Unknown(name)) if name == "p.x"
        ));
        assert!(table.resolve_name("Physics.m").is_err());
    }

    #[test]
    fn exact_names_win_over_prefixes() {
        let table = UnitTable::si();
        assert_eq!(table.lookup("kg").unwrap().scale.to_f64(), 1.0);
        assert_eq!(table.lookup("min").unwrap().scale.to_f64(), 60.0);
        assert_eq!(table.lookup("h").unwrap().scale.to_f64(), 3600.0);
        assert_eq!(
            table.lookup("T").unwrap().dim,
            env().parse("M/(I*T^2)").unwrap()
        );
    }

    #[test]
    fn derived_units_have_the_documented_dimensions() {
        let table = UnitTable::si();
        let env = env();
        for (unit, dimension) in [
            ("N", "M*L/T^2"),
            ("J", "M*L^2/T^2"),
            ("Pa", "M/(L*T^2)"),
            ("V", "M*L^2/(I*T^3)"),
            ("ohm", "M*L^2/(I^2*T^3)"),
            ("Hz", "1/T"),
        ] {
            assert_eq!(
                table.lookup(unit).unwrap().dim,
                env.parse(dimension).unwrap()
            );
        }
    }

    #[test]
    fn affine_units_convert_with_offset() {
        let table = UnitTable::si();
        let celsius = table.lookup("°C").unwrap();
        assert!((celsius.to_coherent(25.0) - 298.15).abs() < 1e-9);
        let fahrenheit = table.lookup("°F").unwrap();
        assert!((fahrenheit.to_coherent(32.0) - 273.15).abs() < 1e-6);
        assert!((fahrenheit.to_coherent(212.0) - 373.15).abs() < 1e-6);
    }

    #[test]
    fn affine_units_are_rejected_in_products() {
        let table = UnitTable::si();
        let expr = UnitExpr::Div(
            Box::new(UnitExpr::Name("°C".to_string())),
            Box::new(UnitExpr::Name("s".to_string())),
        );
        let error = table.resolve(&expr).expect_err("affine in product");
        assert_eq!(error.code(), "E0508");
    }

    #[test]
    fn unit_expressions_combine_dimensions_and_scales() {
        let table = UnitTable::si();
        let expr = UnitExpr::Div(
            Box::new(UnitExpr::Mul(
                Box::new(UnitExpr::Name("kg".to_string())),
                Box::new(UnitExpr::Pow(
                    Box::new(UnitExpr::Name("m".to_string())),
                    BigRational::from_integer(BigInt::from(2)),
                )),
            )),
            Box::new(UnitExpr::Pow(
                Box::new(UnitExpr::Name("s".to_string())),
                BigRational::from_integer(BigInt::from(2)),
            )),
        );
        let resolved = table.resolve(&expr).unwrap();
        assert_eq!(resolved.dim, env().parse("M*L^2/T^2").unwrap());
        assert_eq!(resolved.scale.to_f64(), 1.0);
    }

    #[test]
    fn percent_is_one_hundredth() {
        let table = UnitTable::si();
        let resolved = table.resolve(&UnitExpr::Percent).unwrap();
        assert!(resolved.dim.is_one());
        assert_eq!(resolved.scale.to_f64(), 0.01);
    }

    #[test]
    fn scales_stay_exact() {
        assert_eq!(Scale::parse("0.3048").unwrap().rat, ratio(381, 1250));
        assert_eq!(Scale::parse("pi/180").unwrap().rat, ratio(1, 180));
        assert_eq!(Scale::parse("pi/180").unwrap().pi, 1);
        assert_eq!(Scale::parse("1852/3600").unwrap().rat, ratio(463, 900));
    }
}

#[cfg(test)]
mod regression_tests {
    use super::*;
    use crate::dim::DimEnv;

    #[test]
    fn luminous_units_have_the_luminosity_dimension() {
        let table = UnitTable::si();
        let env = DimEnv::si();
        assert_eq!(table.lookup("lm").unwrap().dim, env.parse("Lum").unwrap());
        assert_eq!(
            table.lookup("lx").unwrap().dim,
            env.parse("Lum/L^2").unwrap()
        );
    }

    #[test]
    fn an_absurd_exponent_is_rejected_not_panicked() {
        let scale = Scale::parse("1000").unwrap();
        let exponent = BigRational::from_integer(BigInt::from(3_000_000_000_i64));
        assert!(scale.pow(&exponent).is_none());
    }
}
