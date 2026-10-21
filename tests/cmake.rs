use std::{collections::BTreeMap, fs, process::Command};

use lib_respect::browser::{
    Cancellation, LibraryLookup, Store, enrich_libraries, parse_project_files,
    resolve_local_repositories,
};

fn files(cmake: &str, readme: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("shared/host.cmake".into(), cmake.into()),
        ("README.ja.md".into(), readme.into()),
        ("shim/CMakeLists.txt".into(), "project(shim)".into()),
        (
            "Cargo.toml".into(),
            "[package]\nname='app'\n[dependencies]\nserde='1'".into(),
        ),
    ])
}

const LOCAL: &str = r#"
set(PLAYER_ROOT "${CMAKE_CURRENT_LIST_DIR}/..")
set(UAPMD_DIR "${PLAYER_ROOT}/../uapmd" CACHE PATH "uapmd checkout")
add_subdirectory("${UAPMD_DIR}" uapmd EXCLUDE_FROM_ALL)
add_subdirectory("${PLAYER_ROOT}/shim" shim)
"#;

#[test]
fn combines_cargo_cmake_and_readme_evidence_without_internal_directories_or_comments() {
    let cmake = format!(
        r#"
{LOCAL}
message("(")
# FetchContent_Declare(fake GIT_REPOSITORY https://github.com/example/fake)
#[=[
ExternalProject_Add(fake2 GIT_REPOSITORY https://github.com/example/fake2)
]=]
set(REPO https://github.com/actual/fork.git)
FetchContent_Declare(example GIT_REPOSITORY "${{REPO}}" GIT_TAG v1)
ExternalProject_Add(other GIT_REPOSITORY [=[https://github.com/vendor/other]=])
"#
    );
    let report = parse_project_files(
        "rev".into(),
        &files(&cmake, "Uses [UAPMD](https://github.com/atsushieno/uapmd)."),
    )
    .unwrap();
    assert_eq!(report.manifest_count, 3);
    assert_eq!(report.dependencies.len(), 4);
    let uapmd = report
        .dependencies
        .iter()
        .find(|d| d.name == "uapmd")
        .unwrap();
    assert_eq!(
        uapmd.github_url.as_deref(),
        Some("https://github.com/atsushieno/uapmd")
    );
    assert!(uapmd.repository_note.as_ref().unwrap().contains("inferred"));
    let example = report
        .dependencies
        .iter()
        .find(|d| d.name == "example")
        .unwrap();
    assert_eq!(example.requirement, "v1");
    assert_eq!(
        example.github_url.as_deref(),
        Some("https://github.com/actual/fork")
    );
    assert!(
        report
            .dependencies
            .iter()
            .any(|d| d.name == "serde" && d.source == "crates.io")
    );
}

#[test]
fn readme_alone_does_not_create_dependencies_and_ambiguous_urls_remain_unknown() {
    let readme = "[uapmd](https://github.com/upstream/uapmd) [fork](https://github.com/fork/uapmd)";
    let report = parse_project_files("rev".into(), &files(LOCAL, readme)).unwrap();
    assert!(
        report
            .dependencies
            .iter()
            .find(|d| d.name == "uapmd")
            .unwrap()
            .github_url
            .is_none()
    );
    let report = parse_project_files("rev".into(), &files("", readme)).unwrap();
    assert_eq!(report.dependencies.len(), 1);
}

fn git(root: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn new_cmake_evidence_replaces_cached_readme_guess_immediately() {
    struct NoRegistry;
    impl LibraryLookup for NoRegistry {
        fn repository(&self, _: &str, _: &Cancellation) -> anyhow::Result<Option<String>> {
            panic!("CMake must not query crates.io")
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let store = Store::at(temp.path().into());
    for owner in ["upstream", "changed-fork"] {
        let mut report = parse_project_files(
            "rev".into(),
            &files(LOCAL, &format!("https://github.com/{owner}/uapmd")),
        )
        .unwrap();
        report.dependencies.retain(|dep| dep.kind == "cmake");
        enrich_libraries(
            "https://github.com/example/app",
            &mut report,
            &store,
            &NoRegistry,
            &Cancellation::default(),
            |_| {},
        )
        .unwrap();
        assert_eq!(
            report.dependencies[0].github_url,
            Some(format!("https://github.com/{owner}/uapmd"))
        );
    }
}

#[test]
fn actual_git_origin_overrides_readme_and_survives_library_enrichment_and_restore() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("app");
    let uapmd = temp.path().join("uapmd");
    fs::create_dir_all(root.join("shared")).unwrap();
    fs::create_dir_all(&uapmd).unwrap();
    fs::write(root.join("shared/host.cmake"), LOCAL.replace('\n', "\r\n")).unwrap();
    git(&uapmd, &["init"]);
    git(
        &uapmd,
        &[
            "config",
            "remote.origin.url",
            "git@github.com:my-fork/uapmd.git",
        ],
    );
    let input = files(LOCAL, "[UAPMD](https://github.com/atsushieno/uapmd)");
    let mut report = parse_project_files("rev".into(), &input).unwrap();
    report.dependencies.retain(|d| d.kind == "cmake");
    resolve_local_repositories(
        &root.canonicalize().unwrap(),
        &input,
        &mut report,
        &Cancellation::default(),
    )
    .unwrap();
    let dep = &report.dependencies[0];
    assert_eq!(
        dep.github_url.as_deref(),
        Some("https://github.com/my-fork/uapmd")
    );
    assert!(
        dep.repository_note
            .as_ref()
            .unwrap()
            .contains("remote.origin.url")
    );
    struct NoLookup;
    impl LibraryLookup for NoLookup {
        fn repository(&self, _: &str, _: &Cancellation) -> anyhow::Result<Option<String>> {
            panic!("CMake must not query crates.io")
        }
    }
    let store = Store::at(temp.path().join("store"));
    enrich_libraries(
        "https://github.com/example/app",
        &mut report,
        &store,
        &NoLookup,
        &Cancellation::default(),
        |_| {},
    )
    .unwrap();
    let restored = Store::at(store.root().to_owned()).libraries().unwrap();
    let library = restored.libraries.values().next().unwrap();
    assert_eq!(
        library.github_url.as_deref(),
        Some("https://github.com/my-fork/uapmd")
    );
    assert!(library.provenance.contains("remote.origin.url"));
    fs::write(root.join("shared/host.cmake"), "# edited locally").unwrap();
    let mut report = parse_project_files("rev".into(), &input).unwrap();
    resolve_local_repositories(&root, &input, &mut report, &Cancellation::default()).unwrap();
    assert_eq!(
        report
            .dependencies
            .iter()
            .find(|d| d.name == "uapmd")
            .unwrap()
            .github_url
            .as_deref(),
        Some("https://github.com/atsushieno/uapmd")
    );
    fs::write(root.join("shared/host.cmake"), LOCAL).unwrap();
    git(
        &uapmd,
        &[
            "config",
            "remote.origin.url",
            "https://gitlab.com/fork/uapmd.git",
        ],
    );
    resolve_local_repositories(&root, &input, &mut report, &Cancellation::default()).unwrap();
    let dep = report
        .dependencies
        .iter()
        .find(|d| d.name == "uapmd")
        .unwrap();
    assert!(dep.github_url.is_none());
    assert!(dep.source.contains("gitlab.com"));
}
