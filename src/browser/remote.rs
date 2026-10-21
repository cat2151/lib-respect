use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use super::{
    Cancellation, ManifestReport, OwnedRepository,
    cmake::{is_cmake, is_readme},
    local, parse_project_files,
    process::api,
};

#[derive(Deserialize)]
struct Tree {
    sha: String,
    truncated: bool,
    tree: Vec<Entry>,
}
#[derive(Deserialize)]
struct Entry {
    path: String,
    sha: String,
    r#type: String,
    size: Option<u64>,
}

/// Read committed Cargo/CMake files and supplement local references without builds.
pub fn inspect_repository(repo: &OwnedRepository, cancel: &Cancellation) -> Result<ManifestReport> {
    let identity = repo.identity()?;
    let endpoint = format!("repos/{}/{}/git", identity.owner(), identity.name());
    let tree: Tree = serde_json::from_slice(&api(
        &format!("{endpoint}/trees/HEAD?recursive=1"),
        false,
        false,
        cancel,
    )?)
    .context("cannot read repository tree")?;
    ensure!(
        !tree.truncated,
        "GitHub tree is truncated; dependency list would be incomplete"
    );
    let has_cmake = tree
        .tree
        .iter()
        .any(|entry| entry.r#type == "blob" && is_cmake(&entry.path));
    let manifests: Vec<_> = tree
        .tree
        .into_iter()
        .filter(|entry| {
            entry.r#type == "blob"
                && (entry.path == "Cargo.toml"
                    || entry.path.ends_with("/Cargo.toml")
                    || is_cmake(&entry.path)
                    || (has_cmake && is_readme(&entry.path)))
        })
        .collect();
    ensure!(
        manifests.len() <= 256,
        "repository contains more than 256 inspection files; cannot inspect it completely"
    );
    let mut files = BTreeMap::new();
    for manifest in manifests {
        cancel.check()?;
        ensure!(
            manifest.size.is_some_and(|size| size <= 1_048_576),
            "manifest is too large: {}",
            manifest.path
        );
        let bytes = api(
            &format!("{endpoint}/blobs/{}", manifest.sha),
            true,
            false,
            cancel,
        )?;
        files.insert(
            manifest.path,
            String::from_utf8(bytes).context("manifest is not UTF-8")?,
        );
    }
    let mut report = parse_project_files(tree.sha, &files)?;
    if report
        .dependencies
        .iter()
        .any(|dep| dep.source.starts_with("cmake: "))
        && let Some(root) = local::checkout(repo, cancel)?
    {
        local::resolve_local_repositories(&root, &files, &mut report, cancel)?;
    }
    Ok(report)
}
