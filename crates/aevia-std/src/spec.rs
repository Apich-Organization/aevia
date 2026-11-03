//! The names chapter 8 of the specification declares.
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

/// Every name `spec/08-stdlib.md` declares, sorted and de-duplicated.
#[must_use]
pub fn names(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_fence = false;
    let mut depth: i32 = 0;
    for raw in source.lines() {
        if raw.trim_start().starts_with("```") {
            in_fence = !in_fence;
            depth = 0;
            continue;
        }
        if !in_fence {
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
            continue;
        }
        let Some(declared) = declared_names(trimmed) else {
            continue;
        };
        out.extend(declared);
    }
    out.sort();
    out.dedup();
    out
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
}
