//! Dependency views over the catalogue: who depends on a package, and what a
//! set of requirements resolves to.
//!
//! # `POST /api/v1/resolve`
//!
//! The sparse index is enough for a client to resolve on its own, and
//! `aevia update` does. This endpoint exists for clients that would rather not
//! download one index file per transitive package — a CI job, an editor, a
//! script — and for checking what the registry itself would choose. It runs the
//! same resolver (`aevia-resolve`) over the same records the index serves, so
//! the answer is the one a toolchain would compute, not an approximation.
//!
//! The resolver is synchronous and the catalogue is not, so resolution runs in
//! rounds: resolve against what is loaded; if the index reports a package as
//! not loaded, load it and run again. A [`RecordSource`] supplies the records;
//! the database is one, a table of strings in a test is another.

use std::collections::HashSet;

use axum::Json;
use axum::extract::Path;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Deserialize;
use serde::Serialize;

use aevia_registry_core::PackageId;
use aevia_registry_db::repo;
use aevia_resolve::Dependency;
use aevia_resolve::Index;
use aevia_resolve::IndexError;
use aevia_resolve::IndexErrorKind;
use aevia_resolve::Options;
use aevia_resolve::ResolveError;
use aevia_resolve::Root;
use aevia_resolve::SparseIndex;
use aevia_resolve::SparseRecord;
use aevia_resolve::Summary;
use aevia_resolve::Unify;
use aevia_resolve::Version;
use aevia_resolve::VersionReq;

use crate::api::ApiError;
use crate::api::ApiResult;
use crate::state::AppState;

/// The most requirements one request may carry.
pub const MAX_REQUESTED: usize = 256;

/// The most packages one resolution may load from the catalogue.
pub const MAX_LOADED: usize = 1024;

/// One requirement in a resolve request.
#[derive(Clone, Debug, Deserialize)]
pub struct RequestedDependency {
    /// `owner/name`.
    pub package: String,
    /// A version requirement, `^1.2`.
    pub req: String,
    /// The import root the caller uses, echoed back on the edge.
    #[serde(default)]
    pub import: Option<String>,
}

/// A version the caller's lock already holds.
#[derive(Clone, Debug, Deserialize)]
pub struct LockedVersion {
    /// `owner/name`.
    pub package: String,
    /// The locked version.
    pub version: String,
}

/// `POST /api/v1/resolve`
#[derive(Clone, Debug, Deserialize)]
pub struct ResolveRequest {
    /// What the application requires.
    pub dependencies: Vec<RequestedDependency>,
    /// Versions to prefer; a yanked one stays selectable.
    #[serde(default)]
    pub locked: Vec<LockedVersion>,
    /// `series` (default) or `package`.
    #[serde(default)]
    pub unify: Option<String>,
}

/// One chosen package version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ResolvedPackage {
    /// `owner/name`, canonical spelling.
    pub package: String,
    /// The version.
    pub version: String,
    /// `sha256:<hex>` of its archive.
    pub checksum: String,
    /// Whether it is yanked (only possible when the caller locked it).
    pub yanked: bool,
}

/// One dependency edge of the solution.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ResolvedEdge {
    /// `request` for the caller, else `owner/name version`.
    pub from: String,
    /// The import root.
    pub import: String,
    /// `owner/name version`.
    pub to: String,
}

/// The reply.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ResolveResponse {
    /// Every chosen version, sorted.
    pub packages: Vec<ResolvedPackage>,
    /// Every edge, sorted.
    pub edges: Vec<ResolvedEdge>,
}

/// Where records come from.
pub trait RecordSource {
    /// All records of `package` (`owner/name`), or `None` when it does not
    /// exist.
    ///
    /// # Errors
    ///
    /// The catalogue's error.
    fn records(
        &self,
        package: &str,
    ) -> impl std::future::Future<Output = aevia_registry_core::Result<Option<Vec<SparseRecord>>>> + Send;
}

/// An index that has loaded some packages and knows others do not exist.
#[derive(Default)]
struct LoadedIndex {
    records: SparseIndex,
    absent: HashSet<String>,
}

impl Index for LoadedIndex {
    fn summaries(
        &self,
        package: &str,
    ) -> Result<Vec<Summary>, IndexError> {
        if self.records.records(package).is_some() {
            return self.records.summaries(package);
        }
        if self.absent.contains(&package.to_ascii_lowercase()) {
            return Err(IndexError::missing(package));
        }
        Err(IndexError::unavailable(package, "not loaded yet"))
    }
}

fn bad(message: String) -> ApiError {
    ApiError::bad_request("E_DEPENDENCY", message)
}

/// Resolves a request against a record source.
///
/// # Errors
///
/// `400 E_DEPENDENCY` for a malformed package or requirement, `422 E_RESOLVE`
/// with the resolver's explanation when no solution exists.
pub async fn resolve_request(
    source: &impl RecordSource,
    request: &ResolveRequest,
) -> ApiResult<ResolveResponse> {
    if request.dependencies.len() > MAX_REQUESTED || request.locked.len() > MAX_REQUESTED * 4 {
        return Err(bad(format!(
            "a request may carry at most {MAX_REQUESTED} requirements"
        )));
    }
    let mut dependencies = Vec::new();
    for requested in &request.dependencies {
        let package = canonical(&requested.package)?;
        let req = VersionReq::parse(&requested.req)
            .map_err(|error| bad(format!("`{}`: {error}", requested.package)))?;
        dependencies.push(Dependency {
            label: requested
                .import
                .clone()
                .unwrap_or_else(|| aevia_resolve::default_import(&package)),
            package,
            req,
        });
    }
    let mut options = Options {
        unify: match request.unify.as_deref() {
            | None | Some("series") => Unify::Series,
            | Some("package") => Unify::Package,
            | Some(other) => {
                return Err(bad(format!(
                    "`unify` is `series` or `package`, not `{other}`"
                )));
            },
        },
        ..Options::default()
    };
    for locked in &request.locked {
        let version = Version::parse(&locked.version)
            .map_err(|error| bad(format!("locked `{}`: {error}", locked.package)))?;
        options
            .locked
            .entry(canonical(&locked.package)?)
            .or_default()
            .insert(version);
    }
    let roots = [Root {
        id: "request".to_string(),
        dependencies,
    }];
    let mut index = LoadedIndex::default();
    let mut loaded = 0;
    loop {
        match aevia_resolve::resolve(&roots, &index, &options) {
            | Ok(resolution) => {
                return Ok(ResolveResponse {
                    packages: resolution
                        .packages
                        .iter()
                        .map(|chosen| {
                            let record = index.records.record(&chosen.package, &chosen.version);
                            ResolvedPackage {
                                package: chosen.package.clone(),
                                version: chosen.version.to_string(),
                                checksum: record.map(|r| r.checksum.clone()).unwrap_or_default(),
                                yanked: record.is_some_and(|r| r.yanked),
                            }
                        })
                        .collect(),
                    edges: resolution
                        .edges
                        .iter()
                        .map(|edge| {
                            ResolvedEdge {
                                from: edge.from.to_string(),
                                import: edge.label.clone(),
                                to: edge.to.to_string(),
                            }
                        })
                        .collect(),
                });
            },
            | Err(ResolveError::Index(error)) if error.kind == IndexErrorKind::Unavailable => {
                loaded += 1;
                if loaded > MAX_LOADED {
                    return Err(ApiError::new(
                        StatusCode::UNPROCESSABLE_ENTITY,
                        "E_RESOLVE",
                        format!("the requirements reach more than {MAX_LOADED} packages"),
                    ));
                }
                match source.records(&error.package).await? {
                    | Some(records) => index.records.insert_records(&error.package, records),
                    | None => {
                        index.absent.insert(error.package.to_ascii_lowercase());
                    },
                }
            },
            | Err(other) => {
                return Err(ApiError::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "E_RESOLVE",
                    other.to_string(),
                ));
            },
        }
    }
}

fn canonical(package: &str) -> ApiResult<String> {
    aevia_resolve::canonical_package(package).ok_or_else(|| {
        bad(format!(
            "`{package}` is not a package identity; write `owner/name`"
        ))
    })
}

/// Records straight from the catalogue.
struct Catalogue<'a> {
    pool: &'a sqlx::PgPool,
}

impl RecordSource for Catalogue<'_> {
    async fn records(
        &self,
        package: &str,
    ) -> aevia_registry_core::Result<Option<Vec<SparseRecord>>> {
        let Ok(id) = package.parse::<PackageId>() else {
            return Ok(None);
        };
        let Some((row, owner)) =
            repo::package_by_handle(self.pool, id.owner(), &id.name_key()).await?
        else {
            return Ok(None);
        };
        let id = PackageId::new(&owner.handle, &row.name)?;
        let versions = repo::versions(self.pool, row.id).await?;
        // Through the text the index endpoint serves, so the resolver reads
        // exactly what a client would.
        let text = crate::api::index::render(&id, &versions)?;
        aevia_resolve::sparse::parse_text(&text)
            .map(Some)
            .map_err(|error| {
                aevia_registry_core::Error::new("E_JSON", format!("a stored record is damaged: {error}"))
            })
    }
}

/// `POST /api/v1/resolve`
///
/// # Errors
///
/// As [`resolve_request`].
pub async fn resolve(
    State(state): State<AppState>,
    Json(request): Json<ResolveRequest>,
) -> ApiResult<Json<ResolveResponse>> {
    let source = Catalogue { pool: &state.pool };
    Ok(Json(resolve_request(&source, &request).await?))
}

/// One dependent in the reverse-dependency listing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DependentEntry {
    /// `owner/name`.
    pub package: String,
    /// Its newest non-yanked version that depends on the package.
    pub version: String,
    /// The requirement that version states.
    pub req: String,
}

/// The reply of `GET /api/v1/crates/{owner}/{name}/dependents`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DependentsResponse {
    /// `owner/name`.
    pub package: String,
    /// How many distinct packages depend on it through a non-yanked version.
    pub total: i64,
    /// Up to 100 of them, ordered by name.
    pub dependents: Vec<DependentEntry>,
}

/// `GET /api/v1/crates/{owner}/{name}/dependents`
///
/// # Errors
///
/// `404` when the package does not exist.
pub async fn dependents(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
) -> ApiResult<Json<DependentsResponse>> {
    let requested = PackageId::new(&owner, &name).map_err(ApiError::from_core)?;
    let Some((package, owner_row)) =
        repo::package_by_handle(&state.pool, requested.owner(), &requested.name_key()).await?
    else {
        return Err(ApiError::not_found(format!(
            "There is no package called {requested}."
        )));
    };
    let id = PackageId::new(&owner_row.handle, &package.name).map_err(ApiError::from_core)?;
    let rows = repo::reverse_dependencies(&state.pool, &id.key(), 100).await?;
    let total = repo::dependents_count(&state.pool, &id.key()).await?;
    Ok(Json(DependentsResponse {
        package: id.to_string(),
        total,
        dependents: rows
            .into_iter()
            .map(|row| {
                DependentEntry {
                    package: format!("{}/{}", row.owner_handle, row.name),
                    version: row.version,
                    req: row.req,
                }
            })
            .collect(),
    }))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    /// A catalogue held in memory as index text.
    struct Fake {
        texts: BTreeMap<String, String>,
        asked: std::sync::Mutex<Vec<String>>,
    }

    impl RecordSource for Fake {
        async fn records(
            &self,
            package: &str,
        ) -> aevia_registry_core::Result<Option<Vec<SparseRecord>>> {
            self.asked.lock().expect("lock").push(package.to_string());
            Ok(self
                .texts
                .get(package)
                .map(|text| aevia_resolve::sparse::parse_text(text).expect("fixture parses")))
        }
    }

    fn line(
        name: &str,
        version: &str,
        deps: &[(&str, &str)],
        yanked: bool,
    ) -> String {
        let deps: Vec<String> = deps
            .iter()
            .map(|(package, req)| format!("{{\"package\":\"{package}\",\"req\":\"{req}\"}}"))
            .collect();
        format!(
            "{{\"name\":\"{name}\",\"vers\":\"{version}\",\"deps\":[{}],\"cksum\":\"sha256:{}\",\"yanked\":{yanked}}}\n",
            deps.join(","),
            format!("{name}{version}").len()
        )
    }

    fn catalogue() -> Fake {
        let mut texts = BTreeMap::new();
        texts.insert(
            "o/plot".to_string(),
            [
                line("o/plot", "1.0.0", &[("o/units", "^1")], false),
                line("o/plot", "1.1.0", &[("o/units", "^1.2")], false),
                line("o/plot", "2.0.0", &[("o/units", "^2")], false),
            ]
            .concat(),
        );
        texts.insert(
            "o/units".to_string(),
            [
                line("o/units", "1.0.0", &[], false),
                line("o/units", "1.2.0", &[], false),
                line("o/units", "1.3.0", &[], true),
                line("o/units", "2.0.0", &[], false),
            ]
            .concat(),
        );
        Fake {
            texts,
            asked: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn request(dependencies: &[(&str, &str)]) -> ResolveRequest {
        ResolveRequest {
            dependencies: dependencies
                .iter()
                .map(|(package, req)| {
                    RequestedDependency {
                        package: (*package).to_string(),
                        req: (*req).to_string(),
                        import: None,
                    }
                })
                .collect(),
            locked: Vec::new(),
            unify: None,
        }
    }

    #[tokio::test]
    async fn a_request_resolves_through_loaded_rounds() {
        let source = catalogue();
        let response = resolve_request(&source, &request(&[("O/Plot", "^1")]))
            .await
            .expect("resolves");
        let chosen: Vec<String> = response
            .packages
            .iter()
            .map(|p| format!("{} {}", p.package, p.version))
            .collect();
        assert_eq!(chosen, ["o/plot 1.1.0", "o/units 1.2.0"], "yanked 1.3.0 is skipped");
        assert!(response.packages.iter().all(|p| p.checksum.starts_with("sha256:")));
        assert!(response.edges.iter().any(|e| e.from == "request" && e.import == "Plot"));
        // Each package was loaded once, however many rounds it took.
        let asked = source.asked.lock().expect("lock");
        assert_eq!(asked.len(), 2, "{asked:?}");
    }

    #[tokio::test]
    async fn a_lock_keeps_a_yanked_version_and_the_answer_says_so() {
        let source = catalogue();
        let mut asked = request(&[("o/units", "^1")]);
        asked.locked.push(LockedVersion {
            package: "o/units".to_string(),
            version: "1.3.0".to_string(),
        });
        let response = resolve_request(&source, &asked).await.expect("resolves");
        assert_eq!(response.packages.len(), 1);
        assert_eq!(response.packages[0].version, "1.3.0");
        assert!(response.packages[0].yanked);
    }

    #[tokio::test]
    async fn a_missing_package_is_a_422_with_the_resolvers_words() {
        let source = catalogue();
        let error = resolve_request(&source, &request(&[("o/ghost", "^1")]))
            .await
            .expect_err("unknown");
        assert_eq!(error.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(error.error.code, "E_RESOLVE");
        assert!(error.error.message.contains("not in the index"), "{}", error.error.message);
    }

    #[tokio::test]
    async fn conflicting_requirements_under_the_package_rule_explain_themselves() {
        let source = catalogue();
        let mut asked = request(&[("o/plot", "^1"), ("o/units", "^2")]);
        asked.unify = Some("package".to_string());
        let error = resolve_request(&source, &asked).await.expect_err("conflict");
        assert_eq!(error.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(error.error.message.contains("o/units"), "{}", error.error.message);
        // The default rule lets both majors coexist.
        let ok = resolve_request(&source, &request(&[("o/plot", "^1"), ("o/units", "^2")]))
            .await
            .expect("series");
        assert_eq!(
            ok.packages.iter().filter(|p| p.package == "o/units").count(),
            2
        );
    }

    #[tokio::test]
    async fn malformed_input_is_a_400() {
        let source = catalogue();
        for asked in [
            request(&[("nonsense", "^1")]),
            request(&[("o/plot", "not a requirement")]),
        ] {
            let error = resolve_request(&source, &asked).await.expect_err("bad");
            assert_eq!(error.status, StatusCode::BAD_REQUEST);
            assert_eq!(error.error.code, "E_DEPENDENCY");
        }
        let mut asked = request(&[("o/plot", "^1")]);
        asked.unify = Some("both".to_string());
        assert_eq!(
            resolve_request(&source, &asked).await.expect_err("bad").status,
            StatusCode::BAD_REQUEST
        );
    }
}
