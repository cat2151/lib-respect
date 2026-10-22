use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use super::{
    Cancellation, ManifestReport, OwnedRepository, Store,
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
    inspect_with(repo, cancel, |endpoint, _sha| {
        String::from_utf8(api(endpoint, true, false, cancel)?).context("manifest is not UTF-8")
    })
}

/// Git objects are immutable. Cache only successful UTF-8 blobs, scoped to the
/// account, so repeated revisions do not re-download unchanged inspection files.
pub fn inspect_repository_cached(
    repo: &OwnedRepository,
    account_id: u64,
    store: &Store,
    cancel: &Cancellation,
) -> Result<ManifestReport> {
    inspect_with(repo, cancel, |endpoint, sha| {
        cached_blob(store, account_id, sha, || {
            let text = String::from_utf8(api(endpoint, true, false, cancel)?)
                .context("manifest is not UTF-8")?;
            cancel.check()?;
            Ok(text)
        })
    })
}

fn cached_blob(
    store: &Store,
    account_id: u64,
    sha: &str,
    fetch: impl FnOnce() -> Result<String>,
) -> Result<String> {
    #[derive(Deserialize, Serialize)]
    struct Blob {
        schema_version: u32,
        text: String,
    }
    ensure!(
        !sha.is_empty() && sha.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid Git blob SHA"
    );
    let path = format!("cache/github.com/{account_id}/git-blobs/{sha}.json");
    if let Some(blob) = store.read::<Blob>(&path)? {
        return Ok(blob.text);
    }
    let text = fetch()?;
    store.write(
        &path,
        &Blob {
            schema_version: 1,
            text: text.clone(),
        },
    )?;
    Ok(text)
}

fn inspect_with(
    repo: &OwnedRepository,
    cancel: &Cancellation,
    mut blob: impl FnMut(&str, &str) -> Result<String>,
) -> Result<ManifestReport> {
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
        files.insert(
            manifest.path,
            blob(&format!("{endpoint}/blobs/{}", manifest.sha), &manifest.sha)?,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blobs_survive_restart_and_errors_are_not_cached_or_shared_between_accounts() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::at(temp.path().into());
        let text = cached_blob(&store, 1, "abcd", || Ok("[package]".into())).unwrap();
        let restarted = Store::at(temp.path().into());
        assert_eq!(
            cached_blob(&restarted, 1, "abcd", || panic!(
                "immutable blob fetched twice"
            ))
            .unwrap(),
            text
        );
        assert!(cached_blob(&store, 1, "dcba", || anyhow::bail!("offline")).is_err());
        assert_eq!(
            cached_blob(&store, 1, "dcba", || Ok("retry succeeded".into())).unwrap(),
            "retry succeeded"
        );
        assert_eq!(
            cached_blob(&store, 2, "abcd", || Ok("different account".into())).unwrap(),
            "different account"
        );
        assert!(cached_blob(&store, 1, "../invalid", || panic!("invalid SHA fetched")).is_err());
    }
}
