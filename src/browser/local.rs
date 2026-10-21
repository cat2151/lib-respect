use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::Result;

use super::{Cancellation, ManifestReport, OwnedRepository, process::run};
use crate::GithubRepo;

fn origin(path: &Path, cancel: &Cancellation) -> Result<Option<String>> {
    // Require an actual checkout boundary, not the enclosing project's origin.
    if !path.join(".git").exists() {
        return Ok(None);
    }
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(path)
        .args(["config", "--local", "--get", "remote.origin.url"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = run(&mut command, "local Git origin", cancel);
    cancel.check()?;
    Ok(output
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .map(|url| url.trim().to_owned())
        .filter(|url| !url.is_empty()))
}

pub(super) fn checkout(repo: &OwnedRepository, cancel: &Cancellation) -> Result<Option<PathBuf>> {
    let expected = repo.identity()?;
    let start = std::env::var_os("LIB_RESPECT_PROJECTS_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok());
    if let Some(start) = start {
        for parent in start.ancestors() {
            for candidate in [parent.to_path_buf(), parent.join(expected.name())] {
                if origin(&candidate, cancel)?
                    .as_deref()
                    .and_then(GithubRepo::parse)
                    .as_ref()
                    == Some(&expected)
                {
                    return Ok(Some(candidate.canonicalize()?));
                }
            }
        }
    }
    Ok(None)
}

/// Resolve only declarations whose local CMake text matches the inspected snapshot.
/// An actual checkout's origin takes precedence over a README's upstream URL.
pub fn resolve_local_repositories(
    root: &Path,
    files: &BTreeMap<String, String>,
    report: &mut ManifestReport,
    cancel: &Cancellation,
) -> Result<()> {
    for dep in &mut report.dependencies {
        let Some(source) = dep.source.strip_prefix("cmake: ") else {
            continue;
        };
        cancel.check()?;
        let Some(text) = files.get(&dep.manifest) else {
            continue;
        };
        if std::fs::read_to_string(root.join(&dep.manifest))
            .ok()
            .map(|value| value.replace("\r\n", "\n"))
            != Some(text.replace("\r\n", "\n"))
        {
            continue;
        }
        // Verbatim Windows paths disable normal slash/.. handling. CMake paths
        // use forward slashes, so substitute a conventional absolute root.
        let root_text = root.to_string_lossy();
        let root_text = if let Some(unc) = root_text.strip_prefix(r"\\?\UNC\") {
            format!(r"\\{unc}")
        } else {
            root_text
                .strip_prefix(r"\\?\")
                .unwrap_or(&root_text)
                .to_owned()
        };
        let source = source.replace("${LIB_RESPECT_ROOT}", &root_text.replace('\\', "/"));
        if source.contains("${") || source.contains("$<") {
            continue;
        }
        let Ok(target) = root.join(source).canonicalize() else {
            continue;
        };
        if target == root {
            continue;
        }
        if let Some(url) = origin(&target, cancel)? {
            dep.source = format!("git: {url}");
            dep.github_url = GithubRepo::parse(&url).map(|repo| repo.url());
            dep.repository_note = Some("CMake add_subdirectory; verified local checkout remote.origin.url (not a resolved build)".into());
        }
    }
    Ok(())
}
