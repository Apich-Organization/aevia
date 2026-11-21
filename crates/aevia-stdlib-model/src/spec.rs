//! The names and sections chapter 8 of the specification declares.
//!
//! `spec/08-stdlib.md` is the contract, so the list of names it promises is
//! *derived from the document*, never typed by hand: a hand-written list can
//! be shortened until it passes, which is exactly the failure mode the
//! coverage test exists to prevent.
//!
//! Inside a bare, `aevia`, or `text` fence, a line whose head is one or more
//! identifiers followed by `:` declares those identifiers.
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

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

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
#[derive(Clone, Debug, Default, PartialEq, Eq)]
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
    read_chapter(source, false).sections
}

/// The signature text the specification gives each name, `name -> [signature]`.
///
/// A name declared more than once (an overload in another section) keeps every
/// distinct signature, in document order. A signature that wraps onto indented
/// continuation lines is joined with single spaces; `#` comments are dropped.
#[must_use]
pub fn signatures(source: &str) -> BTreeMap<String, Vec<String>> {
    read_chapter(source, true).details.signatures
}

/// A chapter's declaration index and the source used for its generated modules.
pub(crate) struct Chapter<'a> {
    /// Numbered sections in document order, plus any unnumbered fragment.
    pub(crate) sections: Vec<Section>,
    details: Details<'a>,
}

impl Chapter<'_> {
    /// Distinct signatures in their order of appearance.
    pub(crate) fn signatures(
        &self,
        name: &str,
    ) -> &[String] {
        self.details.signatures.get(name).map_or(&[], Vec::as_slice)
    }

    /// Formats the unique constructible declaration in its authoritative section.
    pub(crate) fn type_source(
        &self,
        section: &str,
        name: &str,
    ) -> Result<String, String> {
        let source = self
            .details
            .types
            .get(section)
            .and_then(|types| types.get(name))
            .ok_or_else(|| format!("missing type `{name}` in section {section}"))?;
        if source.duplicate {
            return Err(format!("duplicate type `{name}` in section {section}"));
        }
        Ok(render_type_lines(&source.lines))
    }
}

struct TypeSource<'a> {
    lines: Vec<&'a str>,
    duplicate: bool,
}

#[derive(Default)]
struct Details<'a> {
    signatures: BTreeMap<String, Vec<String>>,
    types: BTreeMap<&'a str, BTreeMap<&'a str, TypeSource<'a>>>,
    signature: Option<(Vec<String>, String)>,
    definition: Option<(&'a str, Vec<&'a str>)>,
}

impl<'a> Details<'a> {
    fn finish_signature(&mut self) {
        let Some((names, text)) = self.signature.take() else {
            return;
        };
        let mut normalized = String::with_capacity(text.len());
        for word in text.split_whitespace() {
            if !normalized.is_empty() {
                normalized.push(' ');
            }
            normalized.push_str(word);
        }
        for name in names {
            let entry = self.signatures.entry(name).or_default();
            if !entry.contains(&normalized) {
                entry.push(normalized.clone());
            }
        }
    }

    fn finish_type(
        &mut self,
        section: &'a str,
    ) {
        let Some((name, lines)) = self.definition.take() else {
            return;
        };
        match self.types.entry(section).or_default().entry(name) {
            | Entry::Vacant(entry) => {
                entry.insert(TypeSource {
                    lines,
                    duplicate: false,
                });
            },
            | Entry::Occupied(mut entry) => entry.get_mut().duplicate = true,
        }
    }

    #[allow(
        clippy::single_call_fn,
        reason = "constructible declarations have different continuation syntax from signatures"
    )]
    fn type_line(
        &mut self,
        section: &'a str,
        line: &'a str,
        nested: bool,
    ) {
        if nested {
            if let Some((_, lines)) = &mut self.definition {
                lines.push(line);
            }
        } else if let Some((name, _)) = type_head(line) {
            self.finish_type(section);
            self.definition = Some((name, vec![line]));
        } else if line.is_empty() || (!line.starts_with(char::is_whitespace) && line != "}") {
            self.finish_type(section);
        } else if let Some((_, lines)) = &mut self.definition {
            lines.push(line);
        }
    }
}

#[allow(
    clippy::single_call_fn,
    reason = "renders constructible definitions on demand in canonical declaration format"
)]
fn render_type_lines(lines: &[&str]) -> String {
    let mut out = String::new();
    for (index, line) in lines.iter().enumerate() {
        if index != 0 && line.trim_start().starts_with("| ") {
            for variant in line.trim().trim_start_matches("| ").split(" | ") {
                out.push_str("\n    | ");
                out.push_str(variant);
            }
        } else {
            if index != 0 {
                out.push('\n');
            }
            out.push_str(line);
        }
    }
    out
}

struct Fence {
    delimiter: u8,
    width: usize,
    declarations: bool,
}

impl Fence {
    #[allow(
        clippy::single_call_fn,
        reason = "keeps the opening and closing delimiter rules together"
    )]
    fn opens(line: &str) -> Option<Self> {
        let line = line.trim_start();
        let delimiter = line.bytes().next()?;
        if !matches!(delimiter, b'`' | b'~') {
            return None;
        }
        let width = line.bytes().take_while(|&byte| byte == delimiter).count();
        let language = line.get(width..)?.split_whitespace().next().unwrap_or("");
        (width >= 3).then_some(Self {
            delimiter,
            width,
            declarations: matches!(language, "" | "aevia" | "text"),
        })
    }

    fn closes(
        &self,
        line: &str,
    ) -> bool {
        let line = line.trim_start();
        let width = line
            .bytes()
            .take_while(|&byte| byte == self.delimiter)
            .count();
        width >= self.width && line.get(width..).is_some_and(|tail| tail.trim().is_empty())
    }
}

/// Reads section titles, value declarations, and the optional details from one
/// stream of chapter-8 source lines.
#[allow(
    clippy::too_many_lines,
    reason = "keeps the fence, section, and declaration transitions together"
)]
pub(crate) fn read_chapter(
    source: &str,
    with_details: bool,
) -> Chapter<'_> {
    let mut sections = Vec::new();
    let mut details = Details::default();
    let mut section_number = "";
    let mut current: Option<Section> = None;
    let mut fence: Option<Fence> = None;
    let mut depth = 0usize;
    for raw in source.lines() {
        if let Some(open) = &fence {
            if open.closes(raw) {
                fence = None;
                depth = 0;
                if with_details {
                    details.finish_signature();
                    details.finish_type(section_number);
                }
                continue;
            }
            if !open.declarations {
                continue;
            }
        } else {
            fence = Fence::opens(raw);
            if fence.is_none()
                && let Some((number, title)) = chapter8_heading(raw)
            {
                section_number = number;
                sections.extend(current.replace(Section {
                    number: number.to_owned(),
                    title: title.to_owned(),
                    ..Section::default()
                }));
            }
            continue;
        }

        let line = raw
            .split_once('#')
            .map_or(raw, |(before, _)| before)
            .trim_end();
        let nested = depth != 0;
        if with_details {
            details.type_line(section_number, line, nested);
        }
        depth = depth
            .saturating_add(line.matches('{').count())
            .saturating_sub(line.matches('}').count());
        let trimmed = line.trim_start();
        if nested || trimmed.is_empty() {
            continue;
        }

        let indented = trimmed.len() < line.len();
        if with_details {
            if indented {
                if let Some((_, signature)) = &mut details.signature {
                    signature.push(' ');
                    signature.push_str(trimmed);
                }
            } else {
                details.finish_signature();
            }
        }
        let head = trimmed.split_whitespace().next().unwrap_or("");
        if matches!(
            head,
            "type" | "class" | "fn" | "module" | "instance" | "import" | "let"
        ) {
            if head == "type"
                && let Some(declared) = declared_type(trimmed)
            {
                current
                    .get_or_insert_with(Section::default)
                    .types
                    .push(declared);
            }
        } else if let Some((declared, signature)) = declared_names(trimmed) {
            if !indented && with_details {
                details.signature = Some((declared.clone(), signature.trim().to_owned()));
            }
            current
                .get_or_insert_with(Section::default)
                .names
                .extend(declared);
        }
    }
    if with_details {
        details.finish_signature();
        details.finish_type(section_number);
    }
    sections.extend(current);
    Chapter { sections, details }
}

/// The number and title of a `## 8.x …` or `### 8.x.y …` heading.
#[allow(
    clippy::single_call_fn,
    reason = "isolates the numbered heading grammar"
)]
fn chapter8_heading(line: &str) -> Option<(&str, &str)> {
    let rest = line
        .strip_prefix("### ")
        .or_else(|| line.strip_prefix("## "))?;
    let rest = rest.trim_start();
    let number_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let (number, title) = rest.split_at_checked(number_end)?;
    let segments = number.strip_prefix("8.")?;
    if !segments
        .split('.')
        .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return None;
    }
    Some((number, title.trim()))
}

/// The type declaration a `type Name<params> …` line declares.
#[allow(
    clippy::single_call_fn,
    reason = "isolates the declaration head grammar"
)]
fn declared_type(line: &str) -> Option<TypeName> {
    let (name, suffix) = type_head(line)?;
    let params = if let Some(tail) = suffix.strip_prefix('<') {
        let (params, _) = tail.split_once('>')?;
        params
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

fn type_head(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix("type ")?.trim_start();
    let name_end = rest
        .find(|ch: char| ch == '<' || ch == '=' || ch.is_whitespace())
        .unwrap_or(rest.len());
    let (name, suffix) = rest.split_at_checked(name_end)?;
    is_identifier(name).then_some((name, suffix))
}

/// The identifiers a `name … : type` line declares, if it declares any.
#[allow(
    clippy::single_call_fn,
    reason = "isolates the declaration head grammar from the chapter traversal"
)]
fn declared_names(line: &str) -> Option<(Vec<String>, &str)> {
    let (head, signature) = line.split_once(':')?;
    let head = head.trim();
    if head.is_empty() {
        return None;
    }
    // A type definition or a type function (`RateOf<s> = …`) is not a library
    // name. A signature never contains `=`, but it does contain `=>` for a
    // class constraint, so only a bare `=` counts.
    if line
        .match_indices('=')
        .any(|(at, _)| !line.get(at..).is_some_and(|tail| tail.starts_with("=>")))
    {
        return None;
    }
    // Drop type arguments and an empty parameter list: `from_dense<F>` declares
    // `from_dense`, and `type_rep<a>()` declares `type_rep`.
    let head = if head.contains(['<', '>', '(']) {
        Cow::Owned(strip_type_arguments(head).replace("()", ""))
    } else {
        Cow::Borrowed(head)
    };
    let mut names = Vec::new();
    for word in head.split_whitespace() {
        if !is_identifier(word) {
            return None;
        }
        names.push(word.to_string());
    }
    if names.is_empty() {
        None
    } else {
        Some((names, signature))
    }
}

/// Removes every `<…>` group, keeping the identifier in front of it.
#[allow(
    clippy::single_call_fn,
    reason = "keeps generic syntax scanning separate from signature extraction"
)]
fn strip_type_arguments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut depth = 0usize;
    for character in text.chars() {
        match character {
            | '<' => depth = depth.saturating_add(1),
            | '>' => depth = depth.saturating_sub(1),
            | other if depth == 0 => out.push(other),
            | _ => {},
        }
    }
    out
}

/// True for `name` or `Rssn.name`.
fn is_identifier(text: &str) -> bool {
    let valid = |part: &str| {
        let mut chars = part.chars();
        matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    };
    text.split_once('.').map_or_else(
        || valid(text),
        |(first, second)| valid(first) && valid(second),
    )
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
    fn groups_declarations_before_the_first_numbered_heading() {
        let parsed = sections("```text\ntype Table<ρ>\nread write : Int\n```\n## 8.2 Math\n");
        let [unnumbered, math] = parsed.as_slice() else {
            panic!("expected two sections: {parsed:?}")
        };
        assert!(unnumbered.number.is_empty());
        assert_eq!(unnumbered.names, ["read", "write"]);
        assert_eq!(
            unnumbered.types.first().map(|ty| ty.name.as_str()),
            Some("Table")
        );
        assert_eq!(math.number, "8.2");
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
    fn fences_use_their_delimiter_and_width_and_exclude_quoted_examples() {
        let source = "\
## 8.2 Math\r\n\
````markdown\r\n\
```aevia\r\n\
quoted : Int\r\n\
```\r\n\
## 8.9 Quoted heading\r\n\
`````\r\n\
~~~aevia\r\n\
first : Int\r\n\
~~~text\r\n\
second : Int\r\n\
~~~~\r\n";
        let parsed = sections(source);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].number, "8.2");
        assert_eq!(parsed[0].names, ["first", "second"]);
        assert_eq!(
            signatures(source).into_keys().collect::<Vec<_>>(),
            ["first", "second"]
        );
    }

    #[test]
    fn signatures_preserve_distinct_overloads_in_document_order() {
        let source = "```text\nrepeat<F<X>>() : Eq<X> => X\n    -> X # continuation\n\
                      repeat : Eq<X> => X -> X\nrepeat : Int -> Int\n```\n";
        assert_eq!(names(source), ["repeat"]);
        assert_eq!(
            signatures(source).get("repeat").unwrap(),
            &["Eq<X> => X -> X", "Int -> Int"]
        );
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
