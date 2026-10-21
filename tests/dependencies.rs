use lib_respect::{CollectOptions, RepositoryOrigin, collect_dependencies, report_from_metadata};
use serde_json::json;

mod support;

#[test]
fn cargo_workspace_keeps_one_hop_aliases_kinds_and_unknown_repositories() {
    let fixture = support::workspace();
    let report = collect_dependencies(&CollectOptions {
        manifest_path: Some(fixture.path().into()),
        offline: true,
        ..Default::default()
    })
    .unwrap();
    assert_eq!(
        report
            .packages
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["app", "other"]
    );
    let app = &report.packages[0];
    assert!(
        !app.dependencies
            .iter()
            .any(|d| matches!(d.name.as_str(), "transitive" | "optional"))
    );
    let shared = app
        .dependencies
        .iter()
        .find(|d| d.name == "shared")
        .unwrap();
    assert_eq!(shared.dependency_name, "renamed");
    assert!(shared.source.is_none());
    assert_eq!(
        shared.github.as_ref().unwrap().url,
        "https://github.com/example/shared"
    );
    assert!(matches!(
        shared.github.as_ref().unwrap().identified_by,
        RepositoryOrigin::PackageRepository
    ));
    assert_eq!(
        shared
            .kinds
            .iter()
            .map(|k| k.kind.as_str())
            .collect::<Vec<_>>(),
        ["dev", "normal"]
    );
    assert!(
        app.dependencies
            .iter()
            .find(|d| d.name == "no-repo")
            .unwrap()
            .github
            .is_none()
    );
    let build = app
        .dependencies
        .iter()
        .find(|d| d.name == "build-helper")
        .unwrap();
    assert_eq!(build.kinds[0].kind, "build");
    let unix = app
        .dependencies
        .iter()
        .find(|d| d.name == "unix-only")
        .unwrap();
    assert_eq!(unix.kinds[0].target.as_deref(), Some("cfg(unix)"));
}

#[test]
fn feature_platform_and_package_selection_follow_cargo() {
    let fixture = support::workspace();
    let mut options = CollectOptions {
        manifest_path: Some(fixture.path().join("app/Cargo.toml")),
        packages: vec!["app".into()],
        all_features: true,
        filter_platform: Some("x86_64-pc-windows-msvc".into()),
        offline: true,
        ..Default::default()
    };
    let report = collect_dependencies(&options).unwrap();
    assert_eq!(report.packages.len(), 1);
    let dependencies = &report.packages[0].dependencies;
    assert!(dependencies.iter().any(|d| d.name == "optional"));
    assert!(!dependencies.iter().any(|d| d.name == "unix-only"));
    options.packages = vec!["missing".into()];
    assert!(
        collect_dependencies(&options)
            .unwrap_err()
            .to_string()
            .contains("no package named")
    );
}

fn metadata() -> serde_json::Value {
    let package = |id: &str, name: &str, source: Option<&str>, repository: Option<&str>| {
        json!({
            "id": id, "name": name, "version": "1.0.0", "source": source,
            "repository": repository, "manifest_path": format!("X:/workspace/{name}/Cargo.toml")
        })
    };
    json!({
        "version": 1,
        "workspace_root": "X:/workspace",
        "workspace_members": ["opaque-app-id"],
        "packages": [
            package("opaque-app-id", "app", None, None),
            package("registry-id", "registry-crate", Some("registry+https://github.com/rust-lang/crates.io-index"), None),
            package("fork-id", "fork", Some("git+https://github.com/ForkOwner/Actual.git?branch=main#abcdef"), Some("https://github.com/upstream/original")),
            package("non-github-id", "elsewhere", Some("git+https://gitlab.com/owner/actual#abcdef"), Some("https://github.com/upstream/original")),
            package("published-id", "published", Some("registry+https://github.com/rust-lang/crates.io-index"), Some("https://github.com/Owner/Published/tree/main/crate")),
            package("unrelated-id", "transitive", None, Some("https://github.com/owner/transitive"))
        ],
        "resolve": { "nodes": [
            { "id": "opaque-app-id", "deps": [
                { "name": "registry_crate", "pkg": "registry-id", "dep_kinds": [{"kind":null,"target":null}] },
                { "name": "fork", "pkg": "fork-id", "dep_kinds": [{"kind":null,"target":null}] },
                { "name": "elsewhere", "pkg": "non-github-id", "dep_kinds": [{"kind":null,"target":null}] },
                { "name": "published", "pkg": "published-id", "dep_kinds": [{"kind":null,"target":null}] }
            ] },
            { "id": "fork-id", "deps": [{ "name":"transitive", "pkg":"unrelated-id", "dep_kinds":[{"kind":null,"target":null}] }] }
        ] }
    })
}

#[test]
fn distinguishes_git_checkout_from_published_repository_and_registry_index() {
    let report = report_from_metadata(&metadata().to_string(), &[]).unwrap();
    let dependencies = &report.packages[0].dependencies;
    assert_eq!(dependencies.len(), 4);
    let get = |name| dependencies.iter().find(|d| d.name == name).unwrap();
    assert!(get("registry-crate").github.is_none());
    assert!(get("elsewhere").github.is_none());
    assert_eq!(
        get("fork").github.as_ref().unwrap().url,
        "https://github.com/forkowner/actual"
    );
    assert!(matches!(
        get("fork").github.as_ref().unwrap().identified_by,
        RepositoryOrigin::GitSource
    ));
    assert_eq!(
        get("published").github.as_ref().unwrap().url,
        "https://github.com/owner/published"
    );
}

#[test]
fn missing_graph_and_unsupported_format_are_errors() {
    let mut input = metadata();
    input["resolve"] = serde_json::Value::Null;
    assert!(report_from_metadata(&input.to_string(), &[]).is_err());
    input = metadata();
    input["version"] = json!(2);
    assert!(
        report_from_metadata(&input.to_string(), &[])
            .unwrap_err()
            .to_string()
            .contains("unsupported")
    );
}
