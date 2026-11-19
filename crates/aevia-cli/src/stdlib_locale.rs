//! Localized standard-library prose applied to the same parsed declarations.

use aevia_doc::ModuleDoc;
use aevia_doc::StdlibDocBundle;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt::Write as _;

const MODULES: &str = include_str!("../../aevia-stdlib-model/data/zh-modules.tsv");
const DECLARATIONS: &[&str] = &[
    include_str!("../../aevia-stdlib-model/data/zh-math.tsv"),
    include_str!("../../aevia-stdlib-model/data/zh-numerics.tsv"),
    include_str!("../../aevia-stdlib-model/data/zh-platform.tsv"),
];

#[derive(serde::Serialize)]
pub(super) struct BilingualBundle<'a> {
    #[serde(flatten)]
    pub(super) original: &'a StdlibDocBundle,
    pub(super) zh_modules: &'a [ModuleDoc],
}

fn rows(source: &str) -> impl Iterator<Item = &str> {
    source
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
}

#[allow(
    clippy::single_call_fn,
    reason = "validates Chinese documentation coverage by source declaration identity"
)]
pub(super) fn chinese_modules(modules: &[ModuleDoc]) -> Result<Vec<ModuleDoc>, String> {
    let mut summaries = BTreeMap::new();
    for line in rows(MODULES) {
        let (path, summary) = line
            .split_once('\t')
            .ok_or_else(|| format!("invalid translated module row: {line}"))?;
        if summary.trim().is_empty() || summaries.insert(path, summary).is_some() {
            return Err(format!("blank or duplicate translated module: {path}"));
        }
    }
    let mut translations = BTreeMap::new();
    for source in DECLARATIONS {
        for line in rows(source) {
            let (path, rest) = line
                .split_once('\t')
                .ok_or_else(|| format!("invalid translated declaration row: {line}"))?;
            let (name, prose) = rest
                .split_once('\t')
                .ok_or_else(|| format!("missing translated declaration prose: {line}"))?;
            if name.is_empty()
                || prose.trim().is_empty()
                || translations.insert((path, name), prose).is_some()
            {
                return Err(format!(
                    "blank or duplicate translated declaration: {path}.{name}"
                ));
            }
        }
    }
    let mut used = BTreeSet::new();
    let mut localized = modules.to_vec();
    for module in &mut localized {
        let path = module.path.as_str();
        let summary = summaries
            .remove(path)
            .ok_or_else(|| format!("missing Chinese module description: {path}"))?;
        let unit_table = module
            .doc
            .as_deref()
            .and_then(|text| text.split_once("\n| Unit 单位"));
        module.doc = Some(match unit_table {
            | Some((_, table)) => {
                format!(
                    "{summary}\n\nSI 相干单位下的值等于读数乘比例，再加偏置。\n\n| Unit 单位{table}"
                )
            },
            | None => summary.to_owned(),
        });
        for decl in &mut module.decls {
            let name = decl.name.as_str();
            let key = if matches!(
                path,
                "Units.Natural" | "Units.Planck" | "Units.Atomic" | "Units.Geometrized"
            ) {
                ("Units.Constants", name)
            } else {
                (path, name)
            };
            let (&key, &prose) = translations
                .get_key_value(&key)
                .ok_or_else(|| format!("missing Chinese declaration description: {path}.{name}"))?;
            used.insert(key);
            let mut text = prose.replace("\\n", "\n");
            if key.0 != path && decl.signature.contains("Float") {
                text.push_str("\n\n换算到本单位制时需要用到分数次幂，因此该常数以 `Float` 表示。");
            }
            if let Some(original) = &decl.doc {
                for signature in original
                    .lines()
                    .filter_map(|line| line.strip_prefix("**Signature:** "))
                {
                    let _ = write!(text, "\n\n**签名** {signature}");
                }
                if let Some((_, examples)) = original.split_once("**Examples:**") {
                    let examples = examples
                        .split("\n\n*Source:")
                        .next()
                        .unwrap_or_default()
                        .trim();
                    if !examples.is_empty() {
                        let _ = write!(text, "\n\n**示例**\n\n{examples}");
                    }
                }
            }
            decl.doc = Some(text);
        }
    }
    if let Some(path) = summaries.keys().next() {
        return Err(format!("unused Chinese module description: {path}"));
    }
    if let Some((path, name)) = translations.keys().find(|key| !used.contains(key)) {
        return Err(format!(
            "unused Chinese declaration description: {path}.{name}"
        ));
    }
    Ok(localized)
}

#[allow(
    clippy::single_call_fn,
    reason = "renders the Chinese standard library index from the validated records"
)]
pub(super) fn chinese_index(modules: &[ModuleDoc]) -> String {
    let mut out = String::from("# Aevia 标准库\n\n| 模块 | 公开声明 |\n|---|---|\n");
    for module in modules {
        let _ = writeln!(
            out,
            "| [`{}`]({}) | {} |",
            module.path,
            module.page(),
            module.public_decls().count()
        );
    }
    out
}
