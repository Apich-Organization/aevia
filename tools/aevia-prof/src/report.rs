//! Aggregation and output for captured stack samples.

use std::collections::HashMap;
use std::io;
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

const IDLE_SYMBOLS: &[&str] = &[
    "Wait",
    "ZwDelay",
    "NtDelay",
    "Sleep",
    "ZwRemoveIo",
    "NtRemoveIo",
    "RtlUserThreadStart",
];

#[derive(Default)]
struct Symbol {
    name: Arc<str>,
    idle: bool,
    self_samples: u64,
    inclusive_samples: u64,
    last_stack: Option<usize>,
}

#[derive(Default)]
struct SymbolTable {
    addresses: HashMap<u64, usize>,
    names: HashMap<Arc<str>, usize>,
    symbols: Vec<Symbol>,
}

impl SymbolTable {
    fn intern(
        &mut self,
        address: u64,
        resolve: &mut impl FnMut(u64) -> String,
    ) -> usize {
        *self.addresses.entry(address).or_insert_with(|| {
            let name = resolve(address);
            if let Some(&index) = self.names.get(name.as_str()) {
                return index;
            }
            let index = self.symbols.len();
            let idle = IDLE_SYMBOLS.iter().any(|symbol| name.contains(symbol));
            let name: Arc<str> = name.into();
            self.names.insert(Arc::clone(&name), index);
            self.symbols.push(Symbol {
                name,
                idle,
                ..Symbol::default()
            });
            index
        })
    }
}

pub struct Profile {
    samples: u64,
    symbols: Vec<Symbol>,
    folded: Option<HashMap<Vec<usize>, u64>>,
}

impl Profile {
    #[allow(
        clippy::single_call_fn,
        clippy::indexing_slicing,
        reason = "aggregation is tested without the sampler; interned IDs index the same symbol table"
    )]
    pub(super) fn aggregate(
        stacks: &HashMap<Vec<u64>, u64>,
        mut resolve: impl FnMut(u64) -> String,
        export_folded: bool,
    ) -> Self {
        let mut table = SymbolTable::default();
        let mut samples = 0u64;
        let mut folded = export_folded.then(HashMap::<Vec<usize>, u64>::new);
        let mut path = Vec::new();
        for (stack, (frames, &count)) in stacks.iter().enumerate() {
            let Some((&leaf, callers)) = frames.split_first() else {
                continue;
            };
            let leaf = table.intern(leaf, &mut resolve);
            let symbol = &mut table.symbols[leaf];
            if symbol.idle {
                continue;
            }
            samples = samples.saturating_add(count);
            symbol.self_samples = symbol.self_samples.saturating_add(count);
            symbol.inclusive_samples = symbol.inclusive_samples.saturating_add(count);
            symbol.last_stack = Some(stack);
            if export_folded {
                path.push(leaf);
            }

            for &address in callers {
                let index = table.intern(address, &mut resolve);
                let symbol = &mut table.symbols[index];
                if symbol.last_stack != Some(stack) {
                    symbol.last_stack = Some(stack);
                    symbol.inclusive_samples = symbol.inclusive_samples.saturating_add(count);
                }
                if export_folded {
                    path.push(index);
                }
            }
            if let Some(folded) = &mut folded {
                if let Some(weight) = folded.get_mut(path.as_slice()) {
                    *weight = weight.saturating_add(count);
                } else {
                    folded.insert(path.clone(), count);
                }
                path.clear();
            }
        }
        Self {
            samples,
            symbols: table.symbols,
            folded,
        }
    }

    #[allow(
        clippy::single_call_fn,
        reason = "the report ranks both self and inclusive counts"
    )]
    fn ranked(
        &self,
        count_of: impl Fn(&Symbol) -> u64,
        top: usize,
    ) -> Vec<(&str, u64)> {
        if top == 0 {
            return Vec::new();
        }
        let mut rows: Vec<_> = self
            .symbols
            .iter()
            .filter_map(|symbol| {
                let count = count_of(symbol);
                (count != 0).then_some((symbol.name.as_ref(), count))
            })
            .collect();
        let order = |a: &(&str, u64), b: &(&str, u64)| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0));
        if top < rows.len() {
            rows.select_nth_unstable_by(top, order);
            rows.truncate(top);
        }
        rows.sort_unstable_by(order);
        rows
    }

    #[allow(
        clippy::single_call_fn,
        reason = "unit tests check the sampler's text report"
    )]
    pub(super) fn write_summary(
        &self,
        mut output: impl Write,
        wall: Duration,
        top: usize,
    ) -> io::Result<()> {
        let samples = self.samples;
        for (title, include_callers) in [("self", false), ("inclusive", true)] {
            let rows = self.ranked(
                |symbol| {
                    if include_callers {
                        symbol.inclusive_samples
                    } else {
                        symbol.self_samples
                    }
                },
                top,
            );
            writeln!(
                output,
                "\n== {title} ({samples} busy samples, {:.2}s wall)",
                wall.as_secs_f64()
            )?;
            for (name, count) in rows {
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "displayed percentages are rounded to two decimal places"
                )]
                let pct = 100.0 * (count as f64) / (samples.max(1) as f64);
                let name = name.get(..name.floor_char_boundary(150)).unwrap_or(name);
                writeln!(output, "{pct:6.2}% {count:7}  {name}")?;
            }
        }
        Ok(())
    }

    #[allow(
        clippy::single_call_fn,
        reason = "folded stacks can be tested without a filesystem"
    )]
    pub(super) fn write_folded(
        &self,
        mut output: impl Write,
    ) -> io::Result<()> {
        let Some(folded) = &self.folded else {
            return Ok(());
        };
        let mut rows: Vec<_> = folded.iter().collect();
        rows.sort_unstable_by(|(a, _), (b, _)| self.frame_names(a).cmp(self.frame_names(b)));
        for (frames, count) in rows {
            for (index, name) in self.frame_names(frames).enumerate() {
                if index != 0 {
                    output.write_all(b";")?;
                }
                write_folded_name(&mut output, name)?;
            }
            writeln!(output, " {count}")?;
        }
        Ok(())
    }

    #[allow(
        clippy::indexing_slicing,
        reason = "folded stacks retain interned IDs from this table"
    )]
    fn frame_names<'a>(
        &'a self,
        frames: &'a [usize],
    ) -> impl Iterator<Item = &'a str> {
        frames
            .iter()
            .rev()
            .map(|&index| self.symbols[index].name.as_ref())
    }
}

#[allow(
    clippy::single_call_fn,
    reason = "folded format escaping has an independent syntax"
)]
fn write_folded_name(
    output: &mut impl Write,
    name: &str,
) -> io::Result<()> {
    for part in name.split_inclusive(['%', ';', '\r', '\n']) {
        let escape = match part.as_bytes().last() {
            | Some(b'%') => Some(b"%25"),
            | Some(b';') => Some(b"%3B"),
            | Some(b'\r') => Some(b"%0D"),
            | Some(b'\n') => Some(b"%0A"),
            | _ => None,
        };
        if let Some(escape) = escape {
            let prefix = part.get(..part.len().saturating_sub(1)).unwrap_or_default();
            output.write_all(prefix.as_bytes())?;
            output.write_all(escape)?;
        } else {
            output.write_all(part.as_bytes())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recursion_counts_once_per_symbol_and_preserves_folded_depth() -> io::Result<()> {
        let stacks = HashMap::from([
            (vec![1, 2, 1, 3], 4),
            (vec![4, 2, 5, 3], 3),
            (vec![2, 3], 2),
            (vec![6, 999], 20),
            (Vec::new(), 50),
        ]);
        let mut resolved = Vec::new();
        let profile = Profile::aggregate(
            &stacks,
            |address| {
                resolved.push(address);
                match address {
                    | 1 | 4 | 5 => "step",
                    | 2 => "worker",
                    | 3 => "root",
                    | 6 => "WaitForSingleObject",
                    | _ => "unknown",
                }
                .to_owned()
            },
            true,
        );
        resolved.sort_unstable();
        assert_eq!(resolved, [1, 2, 3, 4, 5, 6]);
        assert_eq!(profile.samples, 9);
        assert_eq!(
            profile.ranked(|item| item.self_samples, 10),
            [("step", 7), ("worker", 2)]
        );
        assert_eq!(
            profile.ranked(|item| item.inclusive_samples, 2),
            [("root", 9), ("worker", 9)]
        );

        let mut text = Vec::new();
        profile.write_folded(&mut text)?;
        assert_eq!(text, b"root;step;worker;step 7\nroot;worker 2\n");
        Ok(())
    }

    #[test]
    fn report_keeps_tie_order_utf8_and_zero_limit() -> io::Result<()> {
        let stacks = HashMap::from([(vec![1], 1), (vec![2], 1), (vec![3], 1)]);
        let name = format!("{}é suffix", "a".repeat(149));
        let profile = Profile::aggregate(
            &stacks,
            |address| {
                match address {
                    | 1 => "z".to_string(),
                    | 2 => "b".to_string(),
                    | _ => name.clone(),
                }
            },
            false,
        );
        let mut text = Vec::new();
        profile.write_summary(&mut text, Duration::from_secs(1), 0)?;
        assert_eq!(text, b"\n== self (3 busy samples, 1.00s wall)\n\n== inclusive (3 busy samples, 1.00s wall)\n");
        text.clear();
        profile.write_summary(&mut text, Duration::from_secs(1), 1)?;
        let text = String::from_utf8_lossy(&text);
        assert_eq!(text.matches(&format!("  {}\n", "a".repeat(149))).count(), 2);
        assert!(!text.contains('é'));
        let mut output = Vec::new();
        profile.write_folded(&mut output)?;
        assert_eq!(output, b"");
        Ok(())
    }

    #[test]
    fn folded_escaping_keeps_frames_and_samples_unambiguous() -> io::Result<()> {
        let stacks = HashMap::from([(vec![1, 2], u64::MAX), (vec![3, 2], 1)]);
        let profile = Profile::aggregate(
            &stacks,
            |address| {
                if address == 2 {
                    "根;%"
                } else {
                    "a\r\nb"
                }
                .to_string()
            },
            true,
        );
        let mut text = Vec::new();
        profile.write_folded(&mut text)?;
        assert_eq!(profile.samples, u64::MAX);
        assert_eq!(
            String::from_utf8_lossy(&text),
            format!("根%3B%25;a%0D%0Ab {}\n", u64::MAX)
        );
        Ok(())
    }
}
