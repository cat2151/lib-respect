use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::{Cancellation, DeclaredDependency, ManifestReport, Store, now};
use crate::GithubRepo;

/// A library, independent of dependency aliases, requirements and consuming projects.
/// Unknown registries and path declarations are scoped to the declaring project;
/// a registry alias or a relative path alone cannot identify a global library.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct LibraryIdentity {
    pub name: String,
    pub source: String,
    pub scope: Option<String>,
}

impl LibraryIdentity {
    pub fn for_dependency(project: &str, dep: &DeclaredDependency) -> Self {
        let name = if dep.source == "crates.io" {
            dep.name.to_ascii_lowercase().replace('_', "-")
        } else {
            dep.name.clone()
        };
        let (source, scope) = if let Some(git) = dep.source.strip_prefix("git: ") {
            (
                format!(
                    "git: {}",
                    GithubRepo::parse(git).map_or_else(|| git.into(), |r| r.url())
                ),
                None,
            )
        } else if dep.source == "crates.io" {
            (dep.source.clone(), None)
        } else {
            // JSON tuple encoding avoids ambiguous separator-based identities.
            (
                dep.source.clone(),
                Some(serde_json::to_string(&(project, &dep.manifest)).unwrap()),
            )
        };
        Self {
            name,
            source,
            scope,
        }
    }

    pub fn id(&self) -> String {
        serde_json::to_string(self).expect("library identities contain only strings")
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Library {
    pub identity: LibraryIdentity,
    pub repository_url: Option<String>,
    pub github_url: Option<String>,
    /// Records what the evidence means; registry metadata is not lockfile resolution.
    pub provenance: String,
    pub reason: Option<String>,
    /// A transient lookup failure is separate from missing/non-GitHub metadata.
    pub error: Option<String>,
    pub checked_at: u64,
    pub used_by: BTreeSet<String>,
    pub requirements: BTreeSet<String>,
}

impl Library {
    pub fn due(&self, time: u64) -> bool {
        let lifetime = if self.error.is_some() {
            60
        } else if self.github_url.is_some() {
            7 * 86_400
        } else {
            86_400
        };
        time.saturating_sub(self.checked_at) >= lifetime
    }

    pub fn note(&self) -> String {
        if let Some(error) = &self.error {
            format!(
                "Lookup failed: {error}{}",
                if self.github_url.is_some() {
                    " (previous URL retained)"
                } else {
                    ""
                }
            )
        } else {
            self.reason
                .clone()
                .unwrap_or_else(|| self.provenance.clone())
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LibraryCache {
    pub schema_version: u32,
    pub libraries: BTreeMap<String, Library>,
}

impl Default for LibraryCache {
    fn default() -> Self {
        Self {
            schema_version: 1,
            libraries: BTreeMap::new(),
        }
    }
}

/// Injectable metadata source. None means the crate declares no repository;
/// errors must never be represented as missing metadata.
pub trait LibraryLookup {
    fn repository(&self, name: &str, cancel: &Cancellation) -> Result<Option<String>>;
}

/// Overlay known mappings without network access, including when gh auth is offline.
pub fn apply_library_cache(project: &str, report: &mut ManifestReport, cache: &LibraryCache) {
    for dep in &mut report.dependencies {
        let id = LibraryIdentity::for_dependency(project, dep).id();
        if let Some(entry) = cache.libraries.get(&id) {
            dep.library_id = Some(id);
            dep.github_url = entry.github_url.clone();
            dep.repository_note = Some(entry.note());
        }
    }
}

/// Populate a project report using the shared index, persisting each completed lib.
/// Progress is delivered after each lib, so the TUI stays useful during network I/O.
pub fn enrich_libraries(
    project: &str,
    report: &mut ManifestReport,
    store: &Store,
    lookup: &impl LibraryLookup,
    cancel: &Cancellation,
    mut progress: impl FnMut(&ManifestReport),
) -> Result<()> {
    let mut cache = store.libraries()?;
    for index in 0..report.dependencies.len() {
        cancel.check()?;
        let dep = &report.dependencies[index];
        let identity = LibraryIdentity::for_dependency(project, dep);
        let id = identity.id();
        let time = now();
        let entry = cache
            .libraries
            .entry(id.clone())
            .or_insert_with(|| Library {
                identity,
                repository_url: None,
                github_url: None,
                provenance: String::new(),
                reason: None,
                error: None,
                checked_at: 0,
                used_by: BTreeSet::new(),
                requirements: BTreeSet::new(),
            });
        entry.used_by.insert(project.into());
        entry.requirements.insert(dep.requirement.clone());
        // CMake mappings come from the current declaration/local checkout, not
        // registry metadata; do not retain an old README guess for a week.
        if dep.kind == "cmake" || entry.due(time) {
            if dep.source == "crates.io" {
                let result = lookup.repository(&dep.name, cancel);
                cancel.check()?;
                match result {
                    Ok(repository) => {
                        set_repository(entry, repository);
                        entry.provenance =
                            "crates.io current crate metadata (not a resolved version)".into();
                    }
                    Err(error) => entry.error = Some(format!("{error:#}")),
                }
            } else if let Some(git) = dep.source.strip_prefix("git: ") {
                set_repository(entry, Some(git.into()));
                entry.provenance = dep
                    .repository_note
                    .clone()
                    .unwrap_or_else(|| "Git dependency source (including forks)".into());
            } else if dep.source.starts_with("cmake: ") {
                set_repository(entry, dep.github_url.clone());
                entry.provenance = dep
                    .repository_note
                    .clone()
                    .unwrap_or_else(|| "CMake static declaration".into());
                if dep.github_url.is_none() {
                    entry.reason = Some(entry.provenance.clone());
                }
            } else if dep.source.starts_with("path: ") {
                entry.reason = Some(
                    "Path dependency: repository cannot be inferred from this declaration".into(),
                );
            } else {
                entry.reason = Some(format!(
                    "Registry {:?}: registry URL/configuration is unavailable",
                    dep.source
                ));
            }
            entry.checked_at = time;
        }
        // A failed save is surfaced to the caller; do not silently use an unpersisted index.
        cancel.check()?;
        store.save_libraries(&cache)?;
        let entry = &cache.libraries[&id];
        let dep = &mut report.dependencies[index];
        dep.library_id = Some(id);
        dep.github_url = entry.github_url.clone();
        dep.repository_note = Some(entry.note());
        progress(report);
    }
    Ok(())
}

fn set_repository(entry: &mut Library, repository: Option<String>) {
    entry.github_url = repository
        .as_deref()
        .and_then(GithubRepo::parse)
        .map(|r| r.url());
    entry.reason = match &repository {
        None => Some("No repository declared in crate metadata".into()),
        Some(url) if entry.github_url.is_none() => {
            Some(format!("Repository is not a recognized GitHub URL: {url}"))
        }
        _ => None,
    };
    entry.repository_url = repository;
    entry.error = None;
}
