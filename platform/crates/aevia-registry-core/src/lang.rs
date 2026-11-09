//! The two languages the site speaks.
//!
//! # Why this lives in the core crate
//!
//! The language is not a property of a page: the API echoes it (a search result
//! says which language it was rendered for), the specification store uses it to
//! pick a title, and the pages use it for every label. Putting the enum here
//! keeps the server and the page layer from inventing two spellings of `zh`.
//!
//! # Why only two
//!
//! Aevia's specification is written in Chinese and its ecosystem is documented
//! in English, so both are needed on day one. A third language would need a
//! translator for every string on the site, and a half-translated interface is
//! worse than a consistently bilingual one — the type is deliberately not a
//! `String`, so adding a language is a compile error at every `match`, not a
//! silent gap in the UI.

/// A language the site can be read in.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    /// English.
    #[default]
    En,
    /// 中文.
    Zh,
}

impl Lang {
    /// The two-letter code used in URLs and cookies.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            | Self::En => "en",
            | Self::Zh => "zh",
        }
    }

    /// The language's own name, for the switcher.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            | Self::En => "English",
            | Self::Zh => "中文",
        }
    }

    /// The other language, for the switcher's target.
    #[must_use]
    pub fn other(self) -> Self {
        match self {
            | Self::En => Self::Zh,
            | Self::Zh => Self::En,
        }
    }

    /// Parses a code, accepting the forms a URL or a header may carry.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let lowered = text.trim().to_ascii_lowercase();
        // `zh-CN`, `zh-Hans`, `zh_CN` all mean Chinese.
        let primary = lowered.split(['-', '_']).next().unwrap_or(&lowered);
        match primary {
            | "en" | "eng" => Some(Self::En),
            | "zh" | "cn" | "chs" | "cht" => Some(Self::Zh),
            | _ => None,
        }
    }

    /// Picks a language from an `Accept-Language` header.
    ///
    /// The header is a weighted list; the first supported entry wins, and
    /// English is the fallback because the site's code and API are English.
    #[must_use]
    pub fn from_accept_language(header: &str) -> Option<Self> {
        let mut candidates: Vec<(f32, Self)> = Vec::new();
        for part in header.split(',') {
            let mut pieces = part.split(';');
            let code = pieces.next()?.trim();
            let weight = pieces
                .next()
                .and_then(|piece| piece.trim().strip_prefix("q="))
                .and_then(|value| value.parse::<f32>().ok())
                .unwrap_or(1.0);
            if let Some(lang) = Self::parse(code) {
                candidates.push((weight, lang));
            }
        }
        candidates
            .into_iter()
            .max_by(|left, right| left.0.total_cmp(&right.0))
            .map(|(_, lang)| lang)
    }
}

impl std::fmt::Display for Lang {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        formatter.write_str(self.code())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip() {
        for lang in [Lang::En, Lang::Zh] {
            assert_eq!(Lang::parse(lang.code()), Some(lang));
        }
    }

    #[test]
    fn regional_codes_fold_to_their_language() {
        assert_eq!(Lang::parse("zh-CN"), Some(Lang::Zh));
        assert_eq!(Lang::parse("zh_Hans"), Some(Lang::Zh));
        assert_eq!(Lang::parse("EN-gb"), Some(Lang::En));
        assert_eq!(Lang::parse("fr"), None);
    }

    #[test]
    fn the_header_picks_the_heaviest_supported_language() {
        assert_eq!(
            Lang::from_accept_language("fr-FR,fr;q=0.9,zh-CN;q=0.8,en;q=0.7"),
            Some(Lang::Zh)
        );
        assert_eq!(Lang::from_accept_language("en-US,en;q=0.9"), Some(Lang::En));
        assert_eq!(Lang::from_accept_language("de-DE"), None);
    }

    #[test]
    fn the_switcher_toggles() {
        assert_eq!(Lang::En.other(), Lang::Zh);
        assert_eq!(Lang::Zh.other().other(), Lang::Zh);
    }
}
