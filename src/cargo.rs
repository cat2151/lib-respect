use std::{collections::BTreeMap, path::PathBuf, process::Command};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::{
    DependencyKind, DependencyReport, DirectDependency, GithubLink, GithubRepo, ProjectPackage,
    RepositoryOrigin, SCHEMA_VERSION,
};

#[derive(Clone, Debug, Default)]
pub struct CollectOptions {
    /// A project directory or a Cargo.toml path; None means the current directory.
    pub manifest_path: Option<PathBuf>,
    /// Workspace package names. Empty means all workspace members.
    pub packages: Vec<String>,
    pub all_features: bool,
    pub no_default_features: bool,
    pub features: Vec<String>,
    pub filter_platform: Option<String>,
    pub locked: bool,
    pub offline: bool,
}

/// Run Cargo metadata and retain exactly one dependency hop per selected member.
/// Cargo itself still resolves (and may download) transitive dependencies.
pub fn collect_dependencies(options: &CollectOptions) -> Result<DependencyReport> {
    let path = options
        .manifest_path
        .clone()
        .unwrap_or(std::env::current_dir().context("cannot determine current directory")?);
    let path = if path.is_dir() {
        path.join("Cargo.toml")
    } else {
        path
    };
    let manifest = std::path::absolute(&path).context("cannot resolve manifest path")?;
    if !manifest.is_file() {
        bail!("Cargo manifest does not exist: {}", manifest.display());
    }
    let mut command = Command::new("cargo");
    command
        .current_dir(
            manifest
                .parent()
                .context("manifest has no parent directory")?,
        )
        .args(["metadata", "--format-version", "1", "--manifest-path"])
        .arg(&manifest);
    for (enabled, flag) in [
        (options.all_features, "--all-features"),
        (options.no_default_features, "--no-default-features"),
        (options.locked, "--locked"),
        (options.offline, "--offline"),
    ] {
        if enabled {
            command.arg(flag);
        }
    }
    if !options.features.is_empty() {
        command.arg("--features").arg(options.features.join(","));
    }
    if let Some(platform) = &options.filter_platform {
        command.arg("--filter-platform").arg(platform);
    }
    let output = command
        .output()
        .context("cannot run cargo metadata; is cargo on PATH?")?;
    if !output.status.success() {
        bail!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let json =
        std::str::from_utf8(&output.stdout).context("cargo metadata returned invalid UTF-8")?;
    report_from_metadata(json, &options.packages)
}

/// Convert Cargo's format-version 1 JSON without invoking Cargo or the network.
/// Package IDs are treated as opaque identifiers, never parsed as URLs.
pub fn report_from_metadata(json: &str, selected: &[String]) -> Result<DependencyReport> {
    let metadata: Metadata = serde_json::from_str(json).context("invalid cargo metadata JSON")?;
    if metadata.version != 1 {
        bail!("unsupported Cargo metadata format: {}", metadata.version);
    }
    let resolve = metadata
        .resolve
        .context("Cargo resolve graph is missing (do not use --no-deps)")?;
    let packages: BTreeMap<_, _> = metadata
        .packages
        .iter()
        .map(|p| (p.id.as_str(), p))
        .collect();
    let nodes: BTreeMap<_, _> = resolve.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let members = metadata
        .workspace_members
        .iter()
        .map(|id| {
            packages
                .get(id.as_str())
                .copied()
                .with_context(|| format!("missing workspace member: {id}"))
        })
        .collect::<Result<Vec<_>>>()?;
    for name in selected {
        if !members.iter().any(|p| &p.name == name) {
            bail!("workspace has no package named {name:?}");
        }
    }
    let mut projects = Vec::new();
    for member in members {
        if !selected.is_empty() && !selected.contains(&member.name) {
            continue;
        }
        let node = nodes
            .get(member.id.as_str())
            .with_context(|| format!("missing resolve node: {}", member.id))?;
        let mut dependencies = Vec::new();
        for edge in &node.deps {
            let package = packages
                .get(edge.pkg.as_str())
                .with_context(|| format!("missing dependency: {}", edge.pkg))?;
            let mut kinds = edge
                .dep_kinds
                .iter()
                .map(|kind| DependencyKind {
                    kind: kind.kind.clone().unwrap_or_else(|| "normal".into()),
                    target: kind.target.clone(),
                })
                .collect::<Vec<_>>();
            kinds.sort();
            kinds.dedup();
            dependencies.push(DirectDependency {
                package_id: package.id.clone(),
                name: package.name.clone(),
                version: package.version.clone(),
                dependency_name: edge.name.clone(),
                kinds,
                source: package.source.clone(),
                repository: package.repository.clone(),
                github: github_link(package),
            });
        }
        dependencies.sort_by(|a, b| {
            (&a.name, &a.version, &a.dependency_name, &a.package_id).cmp(&(
                &b.name,
                &b.version,
                &b.dependency_name,
                &b.package_id,
            ))
        });
        projects.push(ProjectPackage {
            id: member.id.clone(),
            name: member.name.clone(),
            version: member.version.clone(),
            manifest_path: member.manifest_path.clone(),
            dependencies,
        });
    }
    projects.sort_by(|a, b| (&a.name, &a.id).cmp(&(&b.name, &b.id)));
    Ok(DependencyReport {
        schema_version: SCHEMA_VERSION,
        workspace_root: metadata.workspace_root,
        packages: projects,
    })
}

fn github_link(package: &Package) -> Option<GithubLink> {
    // Git dependencies identify the checkout actually used, including forks.
    // Never mistake the crates.io registry index URL for a crate repository.
    let (value, identified_by) = match package
        .source
        .as_deref()
        .and_then(|s| s.strip_prefix("git+"))
    {
        Some(source) => (source, RepositoryOrigin::GitSource),
        None => (
            package.repository.as_deref()?,
            RepositoryOrigin::PackageRepository,
        ),
    };
    Some(GithubLink {
        url: GithubRepo::parse(value)?.url(),
        identified_by,
    })
}

#[derive(Deserialize)]
struct Metadata {
    version: u32,
    packages: Vec<Package>,
    workspace_members: Vec<String>,
    workspace_root: String,
    resolve: Option<Resolve>,
}

#[derive(Deserialize)]
struct Package {
    id: String,
    name: String,
    version: String,
    manifest_path: String,
    repository: Option<String>,
    source: Option<String>,
}

#[derive(Deserialize)]
struct Resolve {
    nodes: Vec<Node>,
}

#[derive(Deserialize)]
struct Node {
    id: String,
    deps: Vec<Edge>,
}

#[derive(Deserialize)]
struct Edge {
    name: String,
    pkg: String,
    dep_kinds: Vec<Kind>,
}

#[derive(Deserialize)]
struct Kind {
    kind: Option<String>,
    target: Option<String>,
}
