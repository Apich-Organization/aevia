//! Bounded, read-only views of values at a debugger stop.

use super::MODULE_BINDINGS;
use aevia_core::value::Scope;
use aevia_core::value::Value;
use aevia_core::value::format_scalar;
use serde_json::Value as Json;
use serde_json::json;
use std::collections::BTreeMap;

/// DAP variable references live until the next resume.
#[derive(Default)]
pub(super) struct VariableStore {
    lists: Vec<Vec<Json>>,
}

impl VariableStore {
    #[allow(
        clippy::single_call_fn,
        reason = "takes a bounded lexical scope snapshot for the wire protocol"
    )]
    pub(super) fn capture(mut scope: &Scope) -> Self {
        let mut snapshot = Self {
            lists: vec![Vec::new(); MODULE_BINDINGS],
        };
        let mut locals = BTreeMap::new();
        while let Some(parent) = &scope.parent {
            snapshot.bindings(&mut locals, scope);
            scope = parent;
        }
        let mut module = BTreeMap::new();
        snapshot.bindings(&mut module, scope);
        if let [local_entries, module_entries, ..] = snapshot.lists.as_mut_slice() {
            *local_entries = locals.into_values().collect();
            *module_entries = module.into_values().collect();
        }
        snapshot
    }

    pub(super) fn list(
        &self,
        reference: usize,
    ) -> &[Json] {
        reference
            .checked_sub(1)
            .and_then(|index| self.lists.get(index))
            .map_or(&[], Vec::as_slice)
    }

    fn bindings(
        &mut self,
        bindings: &mut BTreeMap<String, Json>,
        scope: &Scope,
    ) {
        const MAX_BINDINGS: usize = 128;
        if bindings.len() >= MAX_BINDINGS {
            return;
        }
        let vars = scope.vars.borrow();
        let mut vars = vars
            .iter()
            .filter(|(name, _)| !name.starts_with('$'))
            .collect::<Vec<_>>();
        vars.sort_unstable_by_key(|(name, _)| *name);
        for (name, value) in vars {
            if bindings.len() >= MAX_BINDINGS {
                break;
            }
            bindings
                .entry(name.clone())
                .or_insert_with(|| self.item(name, value, 0));
        }
    }

    fn item(
        &mut self,
        name: &str,
        value: &Value,
        depth: usize,
    ) -> Json {
        let mut item = json!({
            "name": name,
            "type": value.type_name(),
            "value": preview(value),
            "variablesReference": self.children(value, depth),
        });
        if depth == 0 {
            super::set_field(&mut item, "evaluateName", json!(name));
        }
        item
    }

    fn children(
        &mut self,
        value: &Value,
        depth: usize,
    ) -> usize {
        const MAX_NESTING: usize = 3;
        const MAX_LISTS: usize = 256;
        const MAX_CHILDREN: usize = 12;
        if depth >= MAX_NESTING || self.lists.len() >= MAX_LISTS {
            return 0;
        }
        let index = self.lists.len();
        self.lists.push(Vec::new());
        let depth = depth.saturating_add(1);
        let items = match value {
            | Value::Record(record) => {
                record
                    .fields
                    .iter()
                    .take(MAX_CHILDREN)
                    .map(|(name, value)| self.item(name, value, depth))
                    .collect::<Vec<_>>()
            },
            | Value::Variant(variant) => {
                variant
                    .fields
                    .iter()
                    .take(MAX_CHILDREN)
                    .enumerate()
                    .map(|(index, value)| self.item(&format!("[{index}]"), value, depth))
                    .collect()
            },
            | Value::Tensor(tensor) => {
                (0..MAX_CHILDREN)
                    .map_while(|index| {
                        tensor
                            .get_flat(index)
                            .map(|value| self.item(&format!("[{index}]"), &value, depth))
                    })
                    .collect()
            },
            | _ => Vec::new(),
        };
        if items.is_empty() {
            self.lists.pop();
            return 0;
        }
        let Some(slot) = self.lists.get_mut(index) else {
            return 0;
        };
        *slot = items;
        index.saturating_add(1)
    }
}

#[allow(
    clippy::single_call_fn,
    reason = "keeps value rendering separate from nested variable reference allocation"
)]
fn preview(value: &Value) -> String {
    match value {
        | Value::Str(text) => {
            let mut chars = text.chars();
            let prefix = chars.by_ref().take(160).collect::<String>();
            let ellipsis = if chars.next().is_some() {
                "…"
            } else {
                ""
            };
            format!("{prefix:?}{ellipsis}")
        },
        | Value::Num(value) => format_scalar(value),
        | Value::Bool(value) => value.to_string(),
        | Value::Unit => "()".into(),
        | Value::Tensor(tensor) => format!("Tensor {:?} <{}>", tensor.shape, tensor.dim),
        | Value::Record(record) => {
            format!(
                "{{ {} }}",
                record
                    .fields
                    .keys()
                    .take(12)
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        },
        | Value::Variant(variant) => format!("{} ({} fields)", variant.name, variant.fields.len()),
        | _ => format!("<{}>", value.type_name()),
    }
}
