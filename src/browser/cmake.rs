use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;

use super::{
    DeclaredDependency, ManifestReport,
    cmake_syntax::{commands, expand},
    parse_manifests,
};
use crate::GithubRepo;

pub const INSPECTION_VERSION: u32 = 2;

pub(super) fn is_cmake(path: &str) -> bool {
    path.rsplit('/').next() == Some("CMakeLists.txt") || path.ends_with(".cmake")
}

pub(super) fn is_readme(path: &str) -> bool {
    path.rsplit('/').next().is_some_and(|name| {
        let name = name.to_ascii_lowercase();
        name == "readme" || name.starts_with("readme.")
    })
}

/// Static declarations, including inactive branches; no configuration or builds.
pub fn parse_project_files(
    revision: String,
    files: &BTreeMap<String, String>,
) -> Result<ManifestReport> {
    let mut report = parse_manifests(revision, files)?;
    report.inspection_version = INSPECTION_VERSION;
    for (path, text) in files.iter().filter(|(path, _)| is_cmake(path)) {
        report.manifest_count += 1;
        let directory = path.rsplit_once('/').map_or(".", |(dir, _)| dir);
        let mut variables = BTreeMap::from([(
            "CMAKE_CURRENT_LIST_DIR".into(),
            format!("${{LIB_RESPECT_ROOT}}/{directory}"),
        )]);
        for (command, args) in commands(text) {
            if command == "set" && args.len() >= 2 {
                variables.insert(args[0].clone(), expand(&args[1], &variables));
                continue;
            }
            let Some(alias) = args.first() else { continue };
            let (name, source, requirement, github_url, note) = match command.as_str() {
                "fetchcontent_declare" | "externalproject_add" => {
                    let field = |key: &str| {
                        args.windows(2)
                            .find(|pair| pair[0].eq_ignore_ascii_case(key))
                            .map(|pair| expand(&pair[1], &variables))
                    };
                    let Some(git) = field("GIT_REPOSITORY") else {
                        continue;
                    };
                    (
                        alias.clone(),
                        format!("git: {git}"),
                        field("GIT_TAG").unwrap_or("*".into()),
                        GithubRepo::parse(&git).map(|r| r.url()),
                        format!("CMake {command} GIT_REPOSITORY (static declaration)"),
                    )
                }
                "add_subdirectory" => {
                    let resolved = expand(alias, &variables);
                    // Internal source directories are implementation, not external dependencies.
                    let relative = resolved
                        .strip_prefix("${LIB_RESPECT_ROOT}/")
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("{directory}/{resolved}"));
                    let internal = normalize(&format!("{relative}/CMakeLists.txt"));
                    if files.contains_key(&internal) {
                        continue;
                    }
                    let name = if let Some(var) =
                        alias.strip_prefix("${").and_then(|v| v.strip_suffix('}'))
                    {
                        var.strip_suffix("_SOURCE_DIR")
                            .or_else(|| var.strip_suffix("_DIR"))
                            .unwrap_or(var)
                            .to_ascii_lowercase()
                    } else {
                        resolved
                            .trim_end_matches('/')
                            .rsplit('/')
                            .next()
                            .unwrap_or(alias)
                            .into()
                    };
                    let links = readme_links(&name, files);
                    let url = (links.len() == 1).then(|| links.iter().next().unwrap().clone());
                    let note = if url.is_some() {
                        "CMake add_subdirectory; GitHub URL inferred from matching README link"
                    } else {
                        "CMake add_subdirectory; external repository not identified"
                    };
                    let location = if resolved.starts_with("${LIB_RESPECT_ROOT}")
                        || std::path::Path::new(&resolved).is_absolute()
                    {
                        resolved
                    } else {
                        format!("{directory}/{resolved}")
                    };
                    (
                        name,
                        format!("cmake: {location}"),
                        "*".into(),
                        url,
                        note.into(),
                    )
                }
                _ => continue,
            };
            report.dependencies.push(DeclaredDependency {
                name,
                alias: alias.clone(),
                requirement,
                kind: "cmake".into(),
                target: None,
                optional: false,
                source,
                github_url,
                library_id: None,
                repository_note: Some(note),
                manifest: path.clone(),
            });
        }
    }
    report.dependencies.sort_by(|a, b| {
        (&a.name, &a.manifest, &a.kind, &a.alias).cmp(&(&b.name, &b.manifest, &b.kind, &b.alias))
    });
    Ok(report)
}

fn normalize(path: &str) -> String {
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "." | "" => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    parts.join("/")
}

fn readme_links(name: &str, files: &BTreeMap<String, String>) -> BTreeSet<String> {
    let mut links = BTreeSet::new();
    for (_, text) in files.iter().filter(|(path, _)| is_readme(path)) {
        for part in text.split(|c: char| c.is_whitespace() || "()<>\"'`[]".contains(c)) {
            if let Some(repo) = GithubRepo::parse(part)
                && repo.name().eq_ignore_ascii_case(name)
            {
                links.insert(repo.url());
            }
        }
    }
    links
}
