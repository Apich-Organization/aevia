//! View models: the data the server hands to the Leptos pages.
//!
//! The pages are pure functions of these structs. Keeping them in the shared
//! crate means the SQL layer and the markup cannot drift: a field the page
//! reads is a field the query fills, and neither side needs the other's source
//! to know that. They are also what the JSON API returns for the same objects,
//! so a page and its API endpoint can never disagree about a version's fields.

use crate::name::PackageId;

/// Counts shown on the front page.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SiteStats {
    /// Packages with at least one version.
    pub packages: i64,
    /// Versions across all packages.
    pub versions: i64,
    /// Archive downloads.
    pub downloads: i64,
    /// Verified publishers.
    pub publishers: i64,
}

/// A package in a list.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CrateCard {
    /// `owner/name`.
    pub id: PackageId,
    /// One sentence, when the author wrote one.
    pub description: Option<String>,
    /// The newest version.
    pub version: String,
    /// When the package last changed, RFC 3339.
    pub updated_at: String,
    /// Archive downloads.
    pub downloads: i64,
    /// Whether documentation was built.
    pub has_docs: bool,
}

/// A page of search results.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CrateList {
    /// Total matches.
    pub total: i64,
    /// The page, one-based.
    pub page: i64,
    /// Results per page.
    pub per_page: i64,
    /// The query as typed.
    pub query: String,
    /// `relevance`, `downloads`, `recent` or `name`.
    pub sort: String,
    /// The matches.
    pub items: Vec<CrateCard>,
}

impl CrateList {
    /// The last page number.
    #[must_use]
    pub fn last_page(&self) -> i64 {
        if self.per_page <= 0 {
            return 1;
        }
        ((self.total + self.per_page - 1) / self.per_page).max(1)
    }

    /// Whether a previous page exists.
    #[must_use]
    pub fn has_previous(&self) -> bool {
        self.page > 1
    }

    /// Whether a next page exists.
    #[must_use]
    pub fn has_next(&self) -> bool {
        self.page < self.last_page()
    }
}

/// One version of a package.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct VersionView {
    /// `major.minor.patch`.
    pub version: String,
    /// `sha256:<hex>`.
    pub checksum: String,
    /// Archive size in bytes.
    pub size: u64,
    /// When it was published, RFC 3339.
    pub published_at: String,
    /// Whether it is yanked.
    pub yanked: bool,
    /// Whether documentation was built.
    pub docs: bool,
    /// `openpgp` or `ssh`.
    pub key_kind: String,
    /// The signing key's fingerprint.
    pub fingerprint: String,
    /// Archive downloads.
    pub downloads: i64,
}

impl VersionView {
    /// The size, in the units a person reads.
    #[must_use]
    pub fn size_human(&self) -> String {
        human_bytes(self.size)
    }
}

/// A publisher identity.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OwnerView {
    /// `github` or `orcid`.
    pub kind: String,
    /// The GitHub login or ORCID iD.
    pub handle: String,
    /// The name the provider shows.
    pub display_name: Option<String>,
    /// The provider's avatar.
    pub avatar_url: Option<String>,
}

impl OwnerView {
    /// Where the identity can be checked.
    #[must_use]
    pub fn profile_url(&self) -> String {
        match self.kind.as_str() {
            | "orcid" => format!("https://orcid.org/{}", self.handle),
            | _ => format!("https://github.com/{}", self.handle),
        }
    }

    /// The provider's name, capitalised.
    #[must_use]
    pub fn provider(&self) -> &'static str {
        match self.kind.as_str() {
            | "orcid" => "ORCID",
            | _ => "GitHub",
        }
    }
}

/// A dependency shown on a package page.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DependencyView {
    /// The module root it is imported under.
    pub import: String,
    /// The package it resolves to, `owner/name`.
    pub package: String,
    /// The version requirement.
    pub req: String,
}

/// A published version that depends on the package being shown.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DependentView {
    /// The dependent package, `owner/name`.
    pub package: String,
    /// The newest non-yanked version of it that depends on this one.
    pub version: String,
    /// The requirement that version states.
    pub req: String,
}

/// Everything a package page shows.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CratePage {
    /// `owner/name`.
    pub id: PackageId,
    /// One sentence.
    pub description: Option<String>,
    /// An SPDX identifier.
    pub license: Option<String>,
    /// Where the source lives.
    pub repository: Option<String>,
    /// Up to five keywords.
    pub keywords: Vec<String>,
    /// The README, rendered.
    pub readme_html: Option<String>,
    /// Newest first.
    pub versions: Vec<VersionView>,
    /// The version the page describes.
    pub selected: VersionView,
    /// Who publishes it.
    pub owners: Vec<OwnerView>,
    /// The selected version's dependencies.
    pub dependencies: Vec<DependencyView>,
    /// Packages that depend on this one (reverse dependencies).
    #[serde(default)]
    pub dependents: Vec<DependentView>,
    /// When the package first appeared.
    pub created_at: String,
    /// When it last changed.
    pub updated_at: String,
    /// Downloads across all versions.
    pub downloads: i64,
    /// Whether the viewer may yank or add keys.
    pub can_manage: bool,
}

impl CratePage {
    /// The install line a reader copies.
    #[must_use]
    pub fn install_command(&self) -> String {
        format!("aevia add {}@{}", self.id, self.selected.version)
    }
}

/// A publisher's public page.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct UserPage {
    /// Who they are.
    pub owner: OwnerView,
    /// When they first signed in, RFC 3339.
    pub joined_at: String,
    /// Downloads across everything they publish.
    pub downloads: i64,
    /// Their packages, newest change first.
    pub packages: Vec<CrateCard>,
}

/// One module in the documentation sidebar.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ModuleLink {
    /// `Kinematics` or `Thermo.IdealGas`.
    pub path: String,
    /// How many declarations it documents.
    pub decls: usize,
    /// How many have no `##` comment.
    pub undocumented: usize,
}

/// The documentation landing page for one version.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DocsIndexPage {
    /// `owner/name`.
    pub id: PackageId,
    /// The documented version.
    pub version: String,
    /// The modules, in order.
    pub modules: Vec<ModuleLink>,
    /// Declarations across all modules.
    pub total_decls: usize,
    /// Declarations without a `##` comment.
    pub undocumented: usize,
    /// The README, rendered, when the package has one.
    pub readme_html: Option<String>,
    /// When the documentation was built, RFC 3339.
    pub generated_at: String,
    /// Other versions that have documentation.
    pub other_versions: Vec<String>,
}

impl DocsIndexPage {
    /// The share of declarations that carry a `##` comment, 0–100.
    #[must_use]
    pub fn documented_percent(&self) -> u32 {
        if self.total_decls == 0 {
            return 100;
        }
        let documented = self.total_decls.saturating_sub(self.undocumented);
        u32::try_from(documented * 100 / self.total_decls).unwrap_or(0)
    }
}

/// One rendered module page.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DocModulePage {
    /// `owner/name`.
    pub id: PackageId,
    /// The version.
    pub version: String,
    /// The module path.
    pub module: String,
    /// The rendered declarations.
    pub html: String,
    /// The sidebar.
    pub modules: Vec<ModuleLink>,
    /// The source file the module came from.
    pub source_file: String,
    /// Declarations in this module.
    pub decls: usize,
    /// Declarations in this module without a `##` comment.
    pub undocumented: usize,
}

/// The signed-in publisher, as the chrome shows them.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Viewer {
    /// The GitHub login or ORCID iD.
    pub handle: String,
    /// `github` or `orcid`.
    pub kind: String,
    /// The provider's name.
    pub display_name: Option<String>,
    /// The provider's avatar.
    pub avatar_url: Option<String>,
}

impl Viewer {
    /// The identity as an [`OwnerView`].
    #[must_use]
    pub fn as_owner(&self) -> OwnerView {
        OwnerView {
            kind: self.kind.clone(),
            handle: self.handle.clone(),
            display_name: self.display_name.clone(),
            avatar_url: self.avatar_url.clone(),
        }
    }
}

/// A signing key on the account page.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct KeyView {
    /// The row's id.
    pub id: i64,
    /// `openpgp` or `ssh`.
    pub kind: String,
    /// The fingerprint.
    pub fingerprint: String,
    /// A label the publisher chose.
    pub label: Option<String>,
    /// When it was bound.
    pub created_at: String,
    /// Whether it has been revoked.
    pub revoked: bool,
}

/// The account page.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AccountPage {
    /// Who is signed in.
    pub viewer: Viewer,
    /// Their keys.
    pub keys: Vec<KeyView>,
    /// Their packages.
    pub packages: Vec<CrateCard>,
}

/// One entry in the transparency log.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LogEntryView {
    /// The entry's position, from zero.
    pub seq: i64,
    /// `publish`, `yank`, `key` or `publisher`.
    pub kind: String,
    /// What happened, in one line.
    pub summary: String,
    /// This entry's hash.
    pub hash: String,
    /// The previous entry's hash.
    pub prev: String,
    /// When it was appended, RFC 3339.
    pub at: String,
}

/// One heading inside a specification chapter.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SpecHeading {
    /// 2, 3 or 4.
    pub level: u8,
    /// The heading text, tags stripped.
    pub text: String,
    /// The anchor id the page carries.
    pub id: String,
}

/// One chapter of the language specification.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SpecChapter {
    /// The URL segment, `04-dimensions`.
    pub slug: String,
    /// The chapter number as written, `04` or `A`.
    pub number: String,
    /// The chapter's own heading, in the language it was written in.
    pub title: String,
    /// The heading with its leading `NN — ` removed, for lists that show the
    /// number in their own column.
    pub short_title: String,
    /// The English title, always present so an English reader can navigate.
    pub title_en: String,
    /// A short English abstract, when one has been written.
    pub summary_en: Option<String>,
    /// The chapter body, rendered.
    pub html: String,
    /// Its headings, for the table of contents.
    pub toc: Vec<SpecHeading>,
    /// Approximate word count, for the index.
    pub words: usize,
    /// The English edition's body, rendered, when `spec/en/` has one.
    #[serde(default)]
    pub html_en: Option<String>,
    /// The English edition's headings.
    #[serde(default)]
    pub toc_en: Vec<SpecHeading>,
    /// The English edition's word count.
    #[serde(default)]
    pub words_en: usize,
    /// Whether the chapter is normative.
    pub normative: bool,
}

/// The whole specification, in reading order.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SpecBook {
    /// The revision string the text declares, e.g. `12`.
    pub revision: String,
    /// The chapters, in order.
    pub chapters: Vec<SpecChapter>,
}

impl SpecBook {
    /// The chapter with this slug.
    #[must_use]
    pub fn chapter(
        &self,
        slug: &str,
    ) -> Option<&SpecChapter> {
        self.chapters.iter().find(|chapter| chapter.slug == slug)
    }

    /// The chapter before `slug`, for the previous link.
    #[must_use]
    pub fn previous(
        &self,
        slug: &str,
    ) -> Option<&SpecChapter> {
        let index = self
            .chapters
            .iter()
            .position(|chapter| chapter.slug == slug)?;
        index
            .checked_sub(1)
            .and_then(|index| self.chapters.get(index))
    }

    /// The chapter after `slug`, for the next link.
    #[must_use]
    pub fn next(
        &self,
        slug: &str,
    ) -> Option<&SpecChapter> {
        let index = self
            .chapters
            .iter()
            .position(|chapter| chapter.slug == slug)?;
        self.chapters.get(index + 1)
    }

    /// Total words across every chapter.
    #[must_use]
    pub fn words(&self) -> usize {
        self.chapters.iter().map(|chapter| chapter.words).sum()
    }
}

/// Formats a byte count the way a person reads it.
#[must_use]
pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paging_arithmetic_is_inclusive() {
        let list = CrateList {
            total: 21,
            page: 1,
            per_page: 10,
            ..CrateList::default()
        };
        assert_eq!(list.last_page(), 3);
        assert!(!list.has_previous());
        assert!(list.has_next());
        let last = CrateList { page: 3, ..list };
        assert!(!last.has_next());
        assert!(last.has_previous());
    }

    #[test]
    fn an_empty_list_still_has_one_page() {
        let list = CrateList {
            total: 0,
            page: 1,
            per_page: 20,
            ..CrateList::default()
        };
        assert_eq!(list.last_page(), 1);
    }

    #[test]
    fn sizes_are_human() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(2048), "2.0 KiB");
        assert_eq!(human_bytes(5 * 1024 * 1024), "5.0 MiB");
    }

    #[test]
    fn the_documented_share_is_a_percentage() {
        let page = DocsIndexPage {
            id: "apich/x".parse().expect("name"),
            version: "1.0.0".to_string(),
            modules: Vec::new(),
            total_decls: 10,
            undocumented: 3,
            readme_html: None,
            generated_at: String::new(),
            other_versions: Vec::new(),
        };
        assert_eq!(page.documented_percent(), 70);
    }

    #[test]
    fn identities_link_to_their_provider() {
        let owner = OwnerView {
            kind: "orcid".to_string(),
            handle: "0000-0002-1825-0097".to_string(),
            display_name: None,
            avatar_url: None,
        };
        assert_eq!(owner.profile_url(), "https://orcid.org/0000-0002-1825-0097");
        assert_eq!(owner.provider(), "ORCID");
    }
}
