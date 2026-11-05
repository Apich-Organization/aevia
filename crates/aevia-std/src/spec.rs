//! The names and sections chapter 8 of the specification declares.
//!
//! `spec/08-stdlib.md` is the contract, so the list of names it promises is
//! *derived from the document*, never typed by hand: a hand-written list can
//! be shortened until it passes, which is exactly the failure mode the
//! coverage test exists to prevent.
//!
//! The extraction is deliberately literal. Inside a fenced block, a line whose
//! head is one or more identifiers followed by `:` declares those identifiers.
//! Everything else is skipped: `type`/`class`/`fn`/`module` heads, lines that
//! begin with an operator (`(&&)`, `|`) or a continuation arrow, and record
//! bodies (the lines between `{` and `}` that a `type … = {` opens). A record
//! body holds field declarations, not library names.
//!
//! Whitespace separates several names on one line: `gcd lcm : (Int, Int) ->
//! Int` declares both.
//!
//! [`sections`] keeps the same extraction but attributes each declaration to the
//! `## 8.x` / `### 8.x.y` section it appears under. The standard-library module
//! tree of §8.20 is generated from that attribution, so a module's export list
//! cannot silently drift from the section it claims to re-export.

/// A type name a section declares, with the parameters the specification
/// writes (`type Table<ρ>` → name `Table`, params `["ρ"]`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeName {
    /// The declared name.
    pub name: String,
    /// The type parameters, in declaration order.
    pub params: Vec<String>,
}

/// One section of chapter 8.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    /// `8.2` or `8.2.2`; empty for declarations outside a chapter-8 section
    /// (the spec's own unit tests feed bare snippets to [`names`]).
    pub number: String,
    /// Heading text after the number (`Math`, `初等函数`).
    pub title: String,
    /// The value/function names the section declares, in source order.
    pub names: Vec<String>,
    /// The types the section declares, in source order.
    pub types: Vec<TypeName>,
}

/// Every name `spec/08-stdlib.md` declares, sorted and de-duplicated.
#[must_use]
pub fn names(source: &str) -> Vec<String> {
    let mut out: Vec<String> = sections(source)
        .into_iter()
        .flat_map(|section| section.names)
        .collect();
    out.sort();
    out.dedup();
    out
}

/// The chapter-8 sections of `source`, in document order.
///
/// The number is the heading's (`8.1`, `8.2.2`); a declaration is attributed to
/// the *deepest* heading it appears under, so `8.2.2`'s names do not also land
/// in `8.2`. Declarations outside any chapter-8 heading (a bare snippet) land in
/// a section whose number is empty, which is what keeps [`names`] usable on
/// fragments.
#[must_use]
pub fn sections(source: &str) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    // Index of the section receiving declarations. `None` until the first
    // declaration or heading; a declaration before any heading materialises the
    // default (numberless) section.
    let mut current: Option<usize> = None;
    let mut in_fence = false;
    let mut depth: i32 = 0;
    for raw in source.lines() {
        if in_fence {
            if raw.trim_start().starts_with("```") {
                in_fence = false;
                depth = 0;
                continue;
            }
        } else {
            if raw.trim_start().starts_with("```") {
                in_fence = true;
                depth = 0;
                continue;
            }
            // A heading is prose, not part of a signature block.
            if let Some((number, title)) = chapter8_heading(raw) {
                out.push(Section {
                    number,
                    title,
                    names: Vec::new(),
                    types: Vec::new(),
                });
                current = Some(out.len() - 1);
                continue;
            }
            continue;
        }
        // A `#` starts a comment in the specification's signatures.
        let line = match raw.find('#') {
            Some(at) => &raw[..at],
            None => raw,
        };
        let opens = i32::try_from(line.matches('{').count()).unwrap_or(0);
        let closes = i32::try_from(line.matches('}').count()).unwrap_or(0);
        if depth > 0 {
            depth = (depth + opens - closes).max(0);
            continue;
        }
        depth = (depth + opens - closes).max(0);
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        let head_word = trimmed.split_whitespace().next().unwrap_or("");
        if matches!(
            head_word,
            "type" | "class" | "fn" | "module" | "instance" | "import" | "let"
        ) {
            if head_word == "type" {
                if let Some(declared) = declared_type(trimmed) {
                    let index = current.unwrap_or_else(|| {
                        out.push(Section {
                            number: String::new(),
                            title: String::new(),
                            names: Vec::new(),
                            types: Vec::new(),
                        });
                        out.len() - 1
                    });
                    out[index].types.push(declared);
                }
            }
            continue;
        }
        let Some(declared) = declared_names(trimmed) else {
            continue;
        };
        let index = current.unwrap_or_else(|| {
            out.push(Section {
                number: String::new(),
                title: String::new(),
                names: Vec::new(),
                types: Vec::new(),
            });
            out.len() - 1
        });
        out[index].names.extend(declared);
    }
    out
}

/// The number and title of a `## 8.x …` or `### 8.x.y …` heading.
fn chapter8_heading(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix("### ").or_else(|| line.strip_prefix("## "))?;
    let rest = rest.trim_start();
    let number_end = rest
        .find(char::is_whitespace)
        .unwrap_or(rest.len());
    let number = &rest[..number_end];
    let mut parts = number.split('.');
    if parts.next() != Some("8") {
        return None;
    }
    let mut seen = 0_usize;
    for part in parts {
        if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        seen += 1;
    }
    if seen == 0 {
        return None;
    }
    Some((number.to_string(), rest[number_end..].trim().to_string()))
}

/// The type declaration a `type Name<params> …` line declares.
fn declared_type(line: &str) -> Option<TypeName> {
    let rest = line.strip_prefix("type ")?;
    let rest = rest.trim_start();
    let name_end = rest
        .find(|ch: char| ch == '<' || ch == '=' || ch.is_whitespace())
        .unwrap_or(rest.len());
    let name = &rest[..name_end];
    if !is_identifier(name) {
        return None;
    }
    let params = if rest[name_end..].starts_with('<') {
        let tail = &rest[name_end + 1..];
        let end = tail.find('>')?;
        tail[..end]
            .split(',')
            .map(|param| param.trim().to_string())
            .filter(|param| !param.is_empty())
            .collect()
    } else {
        Vec::new()
    };
    Some(TypeName {
        name: name.to_string(),
        params,
    })
}

/// The identifiers a `name … : type` line declares, if it declares any.
fn declared_names(line: &str) -> Option<Vec<String>> {
    let colon = line.find(':')?;
    let head = line[..colon].trim();
    if head.is_empty() {
        return None;
    }
    // A type definition or a type function (`RateOf<s> = …`) is not a library
    // name. A signature never contains `=`, but it does contain `=>` for a
    // class constraint, so only a bare `=` counts.
    if line.replace("=>", "").contains('=') {
        return None;
    }
    // Drop type arguments and an empty parameter list: `from_dense<F>` declares
    // `from_dense`, and `type_rep<a>()` declares `type_rep`.
    let head = strip_type_arguments(head).replace("()", "");
    let mut names = Vec::new();
    for word in head.split_whitespace() {
        if !is_identifier(word) {
            return None;
        }
        names.push(word.to_string());
    }
    if names.is_empty() { None } else { Some(names) }
}

/// Removes every `<…>` group, keeping the identifier in front of it.
fn strip_type_arguments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut depth = 0usize;
    for character in text.chars() {
        match character {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            other if depth == 0 => out.push(other),
            _ => {}
        }
    }
    out
}

/// True for `name` or `Rssn.name`.
fn is_identifier(text: &str) -> bool {
    let mut parts = text.split('.');
    let first = parts.next().unwrap_or("");
    let rest: Vec<&str> = parts.collect();
    if rest.len() > 1 {
        return false;
    }
    let valid = |part: &str| {
        let mut chars = part.chars();
        matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    };
    valid(first) && rest.iter().all(|part| valid(part))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_signature_line() {
        let source = "```\nid       : a -> a\ngcd lcm  : (Int, Int) -> Int\n```\n";
        assert_eq!(names(source), vec!["gcd", "id", "lcm"]);
    }

    #[test]
    fn skips_record_bodies_and_operators() {
        let source = "\
```
type Normal<d>     = { mu: Real[d], sigma: Real[d] }
type OdeOpts<s> = {
    rtol: Float[1],
    method: OdeMethod,
}
(&&)     : (Bool, Bool) -> Bool
sqrt     : Scalar<ρ,d> -> Scalar<ρ,d^(1/2)>
```
";
        assert_eq!(names(source), vec!["sqrt"]);
    }

    #[test]
    fn keeps_the_namespace_of_a_qualified_name() {
        let source = "```\nRssn.Version       : String\nfrom_dense<F> : Tensor<shape, τ> -> Sparse<F, shape, τ>\n```\n";
        assert_eq!(names(source), vec!["Rssn.Version", "from_dense"]);
    }

    #[test]
    fn ignores_text_outside_fences() {
        let source = "prose: not a signature\n```\npi : Real[1]\n```\nmore: prose\n";
        assert_eq!(names(source), vec!["pi"]);
    }

    #[test]
    fn attributes_declarations_to_their_section() {
        let source = "\
## 8.2 Math

```
pi : Real[1]
sqrt : Scalar<ρ,d> -> Scalar<ρ,d^(1/2)>
```

### 8.2.2 特殊函数

```
gamma : Scalar<ρ,1> -> Scalar<ρ,1>
```

## 8.3 函数工具

```
curry : ((a, b) -> c) -> (a -> (b -> c))
```
";
        let sections = sections(source);
        let math = sections.iter().find(|s| s.number == "8.2").expect("8.2");
        assert_eq!(math.title, "Math");
        assert_eq!(math.names, ["pi", "sqrt"]);
        let special = sections
            .iter()
            .find(|s| s.number == "8.2.2")
            .expect("8.2.2");
        assert_eq!(special.title, "特殊函数");
        assert_eq!(special.names, ["gamma"]);
        let tools = sections.iter().find(|s| s.number == "8.3").expect("8.3");
        assert_eq!(tools.names, ["curry"]);
    }

    #[test]
    fn records_the_type_names_a_section_declares() {
        let source = "\
## 8.11 Data

```
type Table<ρ>                  # 行多态静态 schema
type Column
read_csv<ρ>   : String -> IO (Result<Table<ρ>, DataError>)
```
";
        let sections = sections(source);
        let data = sections.iter().find(|s| s.number == "8.11").expect("8.11");
        assert_eq!(
            data.types,
            [
                TypeName {
                    name: "Table".to_string(),
                    params: vec!["ρ".to_string()],
                },
                TypeName {
                    name: "Column".to_string(),
                    params: Vec::new(),
                },
            ]
        );
        assert_eq!(data.names, ["read_csv"]);
    }
}
