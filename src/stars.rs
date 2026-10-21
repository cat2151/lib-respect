use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};

use crate::{
    DependencyReport, DependencyUse, GithubRepo, RepositoryStars, SCHEMA_VERSION, StarReport,
};

/// Read-only access to the authenticated viewer and repository star status.
pub trait StarLookup {
    fn viewer(&self) -> Result<String>;
    fn has_starred(&self, repository: &GithubRepo) -> Result<bool>;
}

/// Merge reports and check each normalized GitHub repository exactly once.
/// Authentication errors fail the operation; per-repository errors remain in JSON.
pub fn collect_stars(reports: &[DependencyReport], lookup: &impl StarLookup) -> Result<StarReport> {
    let mut repositories: BTreeMap<GithubRepo, BTreeSet<DependencyUse>> = BTreeMap::new();
    let mut unmapped = BTreeSet::new();
    for report in reports {
        if report.schema_version != SCHEMA_VERSION {
            bail!(
                "unsupported dependency report schema_version: {}",
                report.schema_version
            );
        }
        for project in &report.packages {
            for dependency in &project.dependencies {
                let usage = DependencyUse {
                    workspace_root: report.workspace_root.clone(),
                    project_package_id: project.id.clone(),
                    project_package: project.name.clone(),
                    dependency_package_id: dependency.package_id.clone(),
                    dependency_name: dependency.dependency_name.clone(),
                    crate_name: dependency.name.clone(),
                    version: dependency.version.clone(),
                    kinds: dependency.kinds.clone(),
                };
                if let Some(link) = &dependency.github {
                    let repo = GithubRepo::parse(&link.url).with_context(|| {
                        format!("invalid GitHub repository URL in report: {:?}", link.url)
                    })?;
                    repositories.entry(repo).or_default().insert(usage);
                } else {
                    unmapped.insert(usage);
                }
            }
        }
    }
    let viewer = lookup
        .viewer()
        .context("cannot identify authenticated GitHub user; run gh auth login")?;
    let repositories = repositories
        .into_iter()
        .map(|(repo, used_by)| {
            let (starred, error) = match lookup.has_starred(&repo) {
                Ok(value) => (Some(value), None),
                Err(error) => (None, Some(format!("{error:#}"))),
            };
            RepositoryStars {
                url: repo.url(),
                owner: repo.owner().to_owned(),
                name: repo.name().to_owned(),
                starred,
                error,
                used_by: used_by.into_iter().collect(),
            }
        })
        .collect();
    Ok(StarReport {
        schema_version: SCHEMA_VERSION,
        viewer,
        repositories,
        unmapped_dependencies: unmapped.into_iter().collect(),
    })
}
