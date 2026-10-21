use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use toml::Value;

use crate::GithubRepo;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ManifestReport {
    #[serde(default)]
    pub inspection_version: u32,
    pub revision: String,
    pub manifest_count: usize,
    pub dependencies: Vec<DeclaredDependency>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DeclaredDependency {
    pub name: String,
    pub alias: String,
    pub requirement: String,
    pub kind: String,
    pub target: Option<String>,
    pub optional: bool,
    pub source: String,
    pub github_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_note: Option<String>,
    pub manifest: String,
}

/// Read direct declarations in every committed manifest, including nested projects.
/// This intentionally reports declared requirements, not resolved lockfile versions.
pub fn parse_manifests(
    revision: String,
    files: &BTreeMap<String, String>,
) -> Result<ManifestReport> {
    let parsed: BTreeMap<_, Value> = files
        .iter()
        .filter(|(path, _)| path.ends_with("Cargo.toml"))
        .map(|(path, text)| {
            toml::from_str(text)
                .with_context(|| format!("invalid TOML in {path}"))
                .map(|value| (path.clone(), value))
        })
        .collect::<Result<_>>()?;
    let mut dependencies = Vec::new();
    for (path, value) in &parsed {
        // A virtual workspace's shared dependency table is not itself a usage.
        if value.get("package").is_none() {
            continue;
        }
        let workspace = find_workspace(path, value, &parsed)?;
        collect_sections(path, value, workspace, None, &mut dependencies)?;
        if let Some(targets) = value.get("target").and_then(Value::as_table) {
            for (target, section) in targets {
                collect_sections(path, section, workspace, Some(target), &mut dependencies)?;
            }
        }
    }
    dependencies.sort_by(|a, b| {
        (&a.name, &a.manifest, &a.kind, &a.target, &a.alias).cmp(&(
            &b.name,
            &b.manifest,
            &b.kind,
            &b.target,
            &b.alias,
        ))
    });
    Ok(ManifestReport {
        inspection_version: super::cmake::INSPECTION_VERSION,
        revision,
        manifest_count: parsed.len(),
        dependencies,
    })
}

fn find_workspace<'a>(
    path: &str,
    package: &Value,
    files: &'a BTreeMap<String, Value>,
) -> Result<Option<&'a Value>> {
    let directory = path
        .strip_suffix("Cargo.toml")
        .unwrap_or("")
        .trim_end_matches('/');
    if let Some(explicit) = package
        .get("package")
        .and_then(|p| p.get("workspace"))
        .and_then(Value::as_str)
    {
        let workspace = normalize(&format!("{directory}/{explicit}/Cargo.toml"));
        return files
            .get(&workspace)
            .map(Some)
            .with_context(|| format!("workspace manifest is unavailable: {workspace}"));
    }
    let mut current = Some(directory);
    while let Some(dir) = current {
        let candidate = if dir.is_empty() {
            "Cargo.toml".into()
        } else {
            format!("{dir}/Cargo.toml")
        };
        if let Some(value) = files
            .get(&candidate)
            .filter(|value| value.get("workspace").is_some())
        {
            return Ok(Some(value));
        }
        current = dir
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .or(if dir.is_empty() { None } else { Some("") });
    }
    Ok(None)
}

fn normalize(path: &str) -> String {
    let mut result = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                result.pop();
            }
            _ => result.push(part),
        }
    }
    result.join("/")
}

fn collect_sections(
    path: &str,
    value: &Value,
    workspace: Option<&Value>,
    target: Option<&str>,
    output: &mut Vec<DeclaredDependency>,
) -> Result<()> {
    for (section, kind) in [
        ("dependencies", "normal"),
        ("dev-dependencies", "dev"),
        ("build-dependencies", "build"),
    ] {
        let Some(dependencies) = value.get(section).and_then(Value::as_table) else {
            continue;
        };
        for (alias, definition) in dependencies {
            let inherited = definition
                .get("workspace")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let base = if inherited {
                workspace
                    .and_then(|w| w.get("workspace"))
                    .and_then(|w| w.get("dependencies"))
                    .and_then(|d| d.get(alias))
                    .with_context(|| {
                        format!("cannot inherit workspace dependency {alias:?} in {path}")
                    })?
            } else {
                definition
            };
            let field = |key| base.get(key).and_then(Value::as_str);
            let requirement = base
                .as_str()
                .or_else(|| field("version"))
                .unwrap_or("*")
                .to_owned();
            if !base.is_str() && !base.is_table() {
                bail!("invalid dependency {alias:?} in {path}");
            }
            let (source, github_url) = if let Some(git) = field("git") {
                (
                    format!("git: {git}"),
                    GithubRepo::parse(git).map(|repo| repo.url()),
                )
            } else if let Some(local) = field("path") {
                (format!("path: {local}"), None)
            } else {
                (field("registry").unwrap_or("crates.io").to_owned(), None)
            };
            output.push(DeclaredDependency {
                name: field("package").unwrap_or(alias).to_owned(),
                alias: alias.clone(),
                requirement,
                kind: kind.into(),
                target: target.map(str::to_owned),
                optional: definition
                    .get("optional")
                    .and_then(Value::as_bool)
                    .or_else(|| base.get("optional").and_then(Value::as_bool))
                    .unwrap_or(false),
                source,
                github_url,
                library_id: None,
                repository_note: None,
                manifest: path.into(),
            });
        }
    }
    Ok(())
}
