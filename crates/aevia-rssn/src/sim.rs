//! Typed simulation modules, generated from the rssn scenario catalog.
//!
//! rssn describes every simulation scenario itself (`rssn::sim::schema`):
//! parameter names, types and defaults are inferred from the scenario's own
//! request type, results are witnessed from a run, and a short annotation
//! table adds what inference cannot see (the accepted values of a method name,
//! the type of a field whose default is absent). This module turns each
//! description into an Aevia module, rendered on `import Rssn_Sim_<Name>`:
//!
//! ```text
//! import Rssn_Sim_PoissonMultigrid
//! let result = Rssn_Sim_PoissonMultigrid.run({ Rssn_Sim_PoissonMultigrid.defaults() with n = 65 })
//! ```
//!
//! * `Params` and `Output` are records; a string field with a fixed set of
//!   values becomes a variant type whose constructors are the values;
//!   `null`-able fields are `Option`s; rectangular arrays are tensors of the
//!   rank the catalog found.
//! * `defaults()` is the scenario's own default request (engine scenarios) or
//!   its built-in example; `run(p)` encodes `p`, calls the kernel and decodes
//!   the result into `Output`.
//!
//! Nothing here is written per scenario: a new scenario in rssn appears as a
//! new module, a new parameter as a new field, a new method name as a new
//! constructor. A scenario whose description uses a type Aevia cannot state
//! (or a field named like a keyword) gets no module, and [`module_names`]
//! does not list it.

use std::fmt::Write as _;

use rssn::sim::schema::Field;
use rssn::sim::schema::FieldType;
use rssn::sim::schema::ScenarioSchema;

/// Prefix of the generated module names.
pub const MODULE_PREFIX: &str = "Rssn_Sim_";

const KEYWORDS: &[&str] = &[
    "as",
    "class",
    "derive",
    "dimension",
    "do",
    "else",
    "false",
    "fn",
    "for",
    "if",
    "import",
    "in",
    "instance",
    "let",
    "match",
    "module",
    "open",
    "private",
    "rec",
    "test",
    "then",
    "true",
    "type",
    "unit",
    "where",
    "with",
];

/// Constructor names the prelude already uses.
const RESERVED_CTORS: &[&str] = &["None", "Some", "Ok", "Err", "True", "False", "Cpu", "Gpu"];

fn camel(name: &str) -> String {
    name.split(|c: char| c == '_' || c == '-' || c == ' ')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().collect::<String>() + chars.as_str()
            })
        })
        .collect()
}

/// The module name of a scenario: `Rssn_Sim_` and the CamelCase name.
#[must_use]
pub fn module_name(scenario: &str) -> String {
    format!("{MODULE_PREFIX}{}", camel(scenario))
}

/// The choice types of a module: field name, type name, constructors.
struct Choices(Vec<(String, String, Vec<(String, String)>)>);

impl Choices {
    fn collect(schema: &ScenarioSchema) -> Self {
        let mut out: Vec<(String, String, Vec<(String, String)>)> = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        let fields = schema.params.iter().chain(&schema.result);
        for field in fields {
            let Some(values) = choice_values(&field.ty) else {
                continue;
            };
            if out.iter().any(|(f, ..)| f == &field.name) {
                continue;
            }
            let type_name = camel(&field.name);
            let ctors = values
                .iter()
                .map(|value| {
                    let mut ctor = camel(value);
                    if ctor.is_empty() || !ctor.starts_with(|c: char| c.is_ascii_alphabetic()) {
                        ctor = format!("{type_name}{ctor}");
                    }
                    if RESERVED_CTORS.contains(&ctor.as_str())
                        || seen.contains(&ctor)
                        || ctor == type_name
                    {
                        ctor = format!("{type_name}{ctor}");
                    }
                    seen.push(ctor.clone());
                    (value.clone(), ctor)
                })
                .collect();
            out.push((field.name.clone(), type_name, ctors));
        }
        Self(out)
    }

    fn get(
        &self,
        field: &str,
    ) -> Option<&(String, String, Vec<(String, String)>)> {
        self.0.iter().find(|(f, ..)| f == field)
    }
}

fn choice_values(ty: &FieldType) -> Option<&[String]> {
    match ty {
        | FieldType::Choice { values } if !values.is_empty() => Some(values),
        | FieldType::Optional { inner } => choice_values(inner),
        | _ => None,
    }
}

/// The Aevia type of a field, or `None` when Aevia cannot state it.
fn aevia_type(
    field: &str,
    ty: &FieldType,
    choices: &Choices,
) -> Option<String> {
    Some(match ty {
        | FieldType::Bool => "Bool".to_owned(),
        | FieldType::Int => "Int".to_owned(),
        | FieldType::Float => "Float".to_owned(),
        | FieldType::Str => "String".to_owned(),
        | FieldType::Choice { .. } => choices.get(field)?.1.clone(),
        | FieldType::Tensor { rank, elem } => {
            format!("Tensor<{rank}, {}>", aevia_type(field, elem, choices)?)
        },
        | FieldType::List { elem } => format!("Tensor<1, {}>", aevia_type(field, elem, choices)?),
        | FieldType::Optional { inner } => {
            format!("Option<{}>", aevia_type(field, inner, choices)?)
        },
        | FieldType::Record { fields } => {
            let mut parts = Vec::new();
            for f in fields {
                if KEYWORDS.contains(&f.name.as_str()) {
                    return None;
                }
                parts.push(format!(
                    "{}: {}",
                    f.name,
                    aevia_type(&f.name, &f.ty, choices)?
                ));
            }
            format!("{{ {} }}", parts.join(", "))
        },
        | FieldType::Tuple { .. } | FieldType::Any => return None,
    })
}

/// An Aevia literal of `value` at type `ty`.
fn literal(
    field: &str,
    value: &serde_json::Value,
    ty: &FieldType,
    choices: &Choices,
) -> Option<String> {
    use serde_json::Value as J;
    Some(match (ty, value) {
        | (FieldType::Optional { .. }, J::Null) => "None".to_owned(),
        | (FieldType::Optional { inner }, v) => {
            format!("Some({})", literal(field, v, inner, choices)?)
        },
        | (FieldType::Bool, J::Bool(b)) => b.to_string(),
        | (FieldType::Int, J::Number(n)) => n.as_i64()?.to_string(),
        | (FieldType::Float, J::Number(n)) => float_literal(n.as_f64()?)?,
        | (FieldType::Str, J::String(s)) => format!("{s:?}"),
        | (FieldType::Choice { .. }, J::String(s)) => {
            let (_, _, ctors) = choices.get(field)?;
            ctors.iter().find(|(v, _)| v == s)?.1.clone()
        },
        | (FieldType::Tensor { rank, elem }, J::Object(map)) => {
            // `{v, dim, data}`: rebuild the nesting from the flat data.
            let dims: Vec<usize> = map
                .get("dim")?
                .as_array()?
                .iter()
                .map(|d| d.as_u64().and_then(|d| usize::try_from(d).ok()))
                .collect::<Option<_>>()?;
            let data = map.get("data")?.as_array()?;
            if dims.len() != *rank {
                return None;
            }
            nested(&dims, data, elem, field, choices)?
        },
        | (FieldType::Tensor { rank, elem }, J::Array(items)) => {
            let inner = if *rank > 1 {
                FieldType::Tensor {
                    rank: rank - 1,
                    elem: elem.clone(),
                }
            } else {
                (**elem).clone()
            };
            let parts: Option<Vec<String>> = items
                .iter()
                .map(|item| literal(field, item, &inner, choices))
                .collect();
            format!("[{}]", parts?.join(", "))
        },
        | (FieldType::List { elem }, J::Array(items)) => {
            let parts: Option<Vec<String>> = items
                .iter()
                .map(|item| literal(field, item, elem, choices))
                .collect();
            format!("[{}]", parts?.join(", "))
        },
        | (FieldType::Record { fields }, J::Object(map)) => {
            let mut parts = Vec::new();
            for f in fields {
                parts.push(format!(
                    "{} = {}",
                    f.name,
                    literal(&f.name, map.get(&f.name)?, &f.ty, choices)?
                ));
            }
            format!("{{ {} }}", parts.join(", "))
        },
        | _ => return None,
    })
}

fn nested(
    dims: &[usize],
    data: &[serde_json::Value],
    elem: &FieldType,
    field: &str,
    choices: &Choices,
) -> Option<String> {
    match dims {
        | [] => literal(field, data.first()?, elem, choices),
        | [n, rest @ ..] => {
            let stride: usize = rest.iter().product();
            let mut parts = Vec::with_capacity(*n);
            for k in 0..*n {
                let chunk = data.get(k * stride..(k + 1) * stride)?;
                parts.push(nested(rest, chunk, elem, field, choices)?);
            }
            Some(format!("[{}]", parts.join(", ")))
        },
    }
}

fn float_literal(x: f64) -> Option<String> {
    if !x.is_finite() {
        return None;
    }
    let text = format!("{x:?}");
    // `1e-10` -> `1.0e-10`: a float literal needs its decimal point.
    Some(if text.contains('.') {
        text
    } else if let Some((mantissa, exponent)) = text.split_once('e') {
        format!("{mantissa}.0e{exponent}")
    } else {
        format!("{text}.0")
    })
}

/// Encodes field `name` of `p` for the kernel: choices become their string.
fn encoder(
    name: &str,
    ty: &FieldType,
    choices: &Choices,
) -> String {
    match ty {
        | FieldType::Choice { .. } if choices.get(name).is_some() => {
            format!("{name}_name(p.{name})")
        },
        | FieldType::Optional { inner }
            if matches!(**inner, FieldType::Choice { .. }) && choices.get(name).is_some() =>
        {
            format!(
                "match p.{name} with\n        | Some(c) -> Some({name}_name(c))\n        | None -> None"
            )
        },
        | _ => format!("p.{name}"),
    }
}

/// Renders the module of one scenario, or `None` when Aevia cannot state one
/// of its types.
#[must_use]
pub fn render(schema: &ScenarioSchema) -> Option<String> {
    let module = module_name(&schema.name);
    let choices = Choices::collect(schema);
    let fields = |list: &[Field]| -> Option<Vec<(String, String, Option<String>)>> {
        list.iter()
            .map(|f| {
                if KEYWORDS.contains(&f.name.as_str()) {
                    return None;
                }
                Some((
                    f.name.clone(),
                    aevia_type(&f.name, &f.ty, &choices)?,
                    f.doc.clone(),
                ))
            })
            .collect()
    };
    let params = fields(&schema.params)?;
    let result = fields(&schema.result)?;
    let example: serde_json::Value = serde_json::from_str(&schema.example).ok()?;
    let mut out = String::new();
    let _ = writeln!(out, "## {}", schema.summary);
    let _ = writeln!(out, "##");
    let _ = writeln!(
        out,
        "## The rssn scenario `{}`, generated from the kernel's scenario catalog\n\
         ## (`rssn::sim::schema`) on import: parameter types and defaults come from\n\
         ## the scenario itself, result types from a witnessed run. Do not edit.",
        schema.name
    );
    let _ = writeln!(out, "module {module}\n");
    for (field, type_name, ctors) in &choices.0 {
        let _ = writeln!(out, "## Accepted values of `{field}`.");
        let _ = writeln!(
            out,
            "type {type_name} = {}\n",
            ctors
                .iter()
                .map(|(_, c)| format!("| {c}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        let _ = writeln!(
            out,
            "fn {field}_name(c: {type_name}): String =\n    match c with"
        );
        for (value, ctor) in ctors {
            let _ = writeln!(out, "    | {ctor} -> {value:?}");
        }
        out.push('\n');
    }
    let record = |out: &mut String, name: &str, list: &[(String, String, Option<String>)]| {
        let _ = writeln!(out, "type {name} = {{");
        for (field, ty, doc) in list {
            if let Some(doc) = doc {
                let _ = writeln!(out, "    ## {doc}");
            }
            let _ = writeln!(out, "    {field}: {ty},");
        }
        let _ = writeln!(out, "}}\n");
    };
    record(&mut out, "Params", &params);
    record(&mut out, "Output", &result);
    // defaults()
    let mut values = Vec::new();
    for f in &schema.params {
        let value = example.get(&f.name).unwrap_or(&serde_json::Value::Null);
        values.push(format!(
            "    {} = {},",
            f.name,
            literal(&f.name, value, &f.ty, &choices)?
        ));
    }
    let _ = writeln!(
        out,
        "## {} request of the scenario.\nfn defaults(): Params = {{\n{}\n}}\n",
        if schema.all_defaults {
            "The default"
        } else {
            "A small runnable"
        },
        values.join("\n")
    );
    // run()
    let encoded: Vec<String> = schema
        .params
        .iter()
        .map(|f| {
            format!(
                "        {} = {},",
                f.name,
                encoder(&f.name, &f.ty, &choices)
            )
        })
        .collect();
    let _ = writeln!(
        out,
        "## Runs the scenario.\nfn run(p: Params): Result<Output, SimError> =\n    match simulate({:?}, to_json({{\n{}\n    }})) with\n    | Ok(json) -> from_json(json)\n    | Err(error) -> Err(error)",
        schema.name,
        encoded.join("\n")
    );
    Some(out)
}

/// The scenario catalog of the linked kernel.
fn catalog() -> &'static [ScenarioSchema] {
    static CATALOG: std::sync::OnceLock<Vec<ScenarioSchema>> = std::sync::OnceLock::new();
    CATALOG.get_or_init(rssn::sim::schema::catalog)
}

/// The source of the module `module`, if it names a scenario Aevia can type.
#[must_use]
pub fn scenario_source(module: &str) -> Option<String> {
    let schema = catalog().iter().find(|s| module_name(&s.name) == module)?;
    render(schema)
}

/// Every generated module name.
#[must_use]
pub fn module_names() -> Vec<String> {
    catalog()
        .iter()
        .filter(|s| render(s).is_some())
        .map(|s| module_name(&s.name))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_scenario_gets_a_module() {
        let names = module_names();
        assert_eq!(names.len(), catalog().len(), "{names:?}");
        let source = scenario_source("Rssn_Sim_PoissonMultigrid").unwrap_or_default();
        assert!(source.contains("type Cycle = | V | W | Fmg"), "{source}");
        assert!(source.contains("cycle = cycle_name(p.cycle),"), "{source}");
        assert!(source.contains("fn defaults(): Params"), "{source}");
        let sod = scenario_source("Rssn_Sim_SodShockTube").unwrap_or_default();
        // `none` would collide with `Option`'s constructor.
        assert!(sod.contains("LimiterNone"), "{sod}");
    }

    #[test]
    fn float_literals_keep_a_decimal_point() {
        assert_eq!(float_literal(1e-10).as_deref(), Some("1.0e-10"));
        assert_eq!(float_literal(2.0).as_deref(), Some("2.0"));
        assert_eq!(float_literal(0.25).as_deref(), Some("0.25"));
        assert_eq!(float_literal(f64::NAN), None);
    }
}
