use std::{collections::BTreeMap, fs};

use lib_respect::browser::{
    Account, Cancellation, Catalog, Config, DependencyCache, OwnedRepository, Store,
    identify_account, parse_manifests,
};
use serde_json::json;

fn repo() -> OwnedRepository {
    serde_json::from_value(json!({"id":10,"name":"app","full_name":"example/app","html_url":"https://github.com/example/app","description":null,"pushed_at":"2026-01-01T00:00:00Z","owner":{"id":1,"login":"example"},"private":false,"archived":false,"fork":false})).unwrap()
}

#[test]
fn remote_manifest_view_resolves_workspace_rename_target_and_optional_declarations() {
    let files = BTreeMap::from([
        (
            "Cargo.toml".into(),
            r#"
[workspace]
members = ["crates/app"]
[workspace.dependencies]
renamed = { package = "serde", version = "1", features = ["derive"] }
"#
            .into(),
        ),
        (
            "crates/app/Cargo.toml".into(),
            r#"
[package]
name = "app"
version = "0.1.0"
[dependencies]
renamed = { workspace = true, optional = true }
gitdep = { git = "https://github.com/Owner/Fork.git", branch = "main" }
[dev-dependencies]
testlib = "2"
[build-dependencies]
buildlib = { path = "../buildlib" }
[target.'cfg(windows)'.dependencies]
win = { version = "0.1" }
"#
            .into(),
        ),
        (
            "nested/Cargo.toml".into(),
            "[package]\nname='nested'\nversion='0.1.0'\n[dependencies]\nother='3'".into(),
        ),
    ]);
    let report = parse_manifests("tree-sha".into(), &files).unwrap();
    assert_eq!(report.manifest_count, 3);
    assert_eq!(report.dependencies.len(), 6);
    let serde = report
        .dependencies
        .iter()
        .find(|dep| dep.name == "serde")
        .unwrap();
    assert_eq!(serde.alias, "renamed");
    assert_eq!(serde.requirement, "1");
    assert!(serde.optional);
    assert_eq!(serde.manifest, "crates/app/Cargo.toml");
    let git = report
        .dependencies
        .iter()
        .find(|dep| dep.name == "gitdep")
        .unwrap();
    assert_eq!(
        git.github_url.as_deref(),
        Some("https://github.com/owner/fork")
    );
    assert!(
        report
            .dependencies
            .iter()
            .any(|dep| dep.kind == "build" && dep.source == "path: ../buildlib")
    );
    assert!(report.dependencies.iter().any(|dep| dep.kind == "dev"));
    assert!(
        report
            .dependencies
            .iter()
            .any(|dep| dep.target.as_deref() == Some("cfg(windows)"))
    );
}

#[test]
fn missing_workspace_and_invalid_toml_fail_instead_of_reporting_no_dependencies() {
    let files = BTreeMap::from([(
        "Cargo.toml".into(),
        "[package]\nname='app'\n[dependencies]\nserde.workspace=true".into(),
    )]);
    assert!(
        parse_manifests("sha".into(), &files)
            .unwrap_err()
            .to_string()
            .contains("cannot inherit")
    );
    let files = BTreeMap::from([("Cargo.toml".into(), "[invalid".into())]);
    assert!(parse_manifests("sha".into(), &files).is_err());
    assert_eq!(
        parse_manifests("sha".into(), &BTreeMap::new())
            .unwrap()
            .manifest_count,
        0
    );
}

#[test]
fn eligibility_requires_owner_public_non_archived_and_non_fork() {
    let account = Account {
        id: 1,
        login: "example".into(),
    };
    let original = repo();
    assert!(original.eligible(&account));
    for field in ["private", "archived", "fork"] {
        let mut value = serde_json::to_value(&original).unwrap();
        value[field] = json!(true);
        let altered: OwnedRepository = serde_json::from_value(value).unwrap();
        assert!(!altered.eligible(&account));
    }
    assert!(!original.eligible(&Account {
        id: 2,
        login: "someone-else".into()
    }));
}

#[test]
fn local_store_restores_configuration_catalog_and_dependencies_without_cwd() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::at(directory.path().join("local-app-data"));
    let account = Account {
        id: 1,
        login: "example".into(),
    };
    assert!(store.config().unwrap().account.is_none());
    store
        .save_config(&Config {
            schema_version: 1,
            account: Some(account.clone()),
            selected_repo: Some(10),
            ..Default::default()
        })
        .unwrap();
    let catalog = Catalog {
        schema_version: 1,
        account: account.clone(),
        fetched_at: 123,
        repositories: vec![repo()],
    };
    store.save_catalog(&catalog).unwrap();
    // Replacing an existing snapshot must also work on Windows.
    store.save_catalog(&catalog).unwrap();
    let cache = DependencyCache {
        schema_version: 1,
        account_id: 1,
        repo_id: 10,
        pushed_at: repo().pushed_at,
        fetched_at: 456,
        report: parse_manifests("sha".into(), &BTreeMap::new()).unwrap(),
    };
    store.save_dependencies(&cache).unwrap();
    let reopened = Store::at(store.root().into());
    assert_eq!(reopened.config().unwrap().selected_repo, Some(10));
    assert_eq!(
        reopened
            .catalog(&account)
            .unwrap()
            .unwrap()
            .repositories
            .len(),
        1
    );
    assert_eq!(
        reopened
            .dependencies(1, &repo())
            .unwrap()
            .unwrap()
            .report
            .revision,
        "sha"
    );
    assert!(
        reopened
            .catalog(&Account {
                id: 2,
                login: "another".into()
            })
            .unwrap()
            .is_none()
    );
}

#[test]
fn unknown_or_corrupt_saved_data_is_preserved() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::at(directory.path().into());
    for text in ["{\"schema_version\":999}", "broken json"] {
        fs::write(directory.path().join("config.json"), text).unwrap();
        assert!(store.config().is_err());
        assert!(store.save_config(&Config::default()).is_err());
        assert_eq!(
            fs::read_to_string(directory.path().join("config.json")).unwrap(),
            text
        );
    }
}

#[test]
fn cancelled_request_does_not_start_a_github_process() {
    let cancel = Cancellation::default();
    cancel.cancel();
    assert!(
        identify_account(&cancel)
            .unwrap_err()
            .to_string()
            .contains("cancelled")
    );
}

#[test]
fn repository_star_cache_restores_zero_unknown_and_failures_and_preserves_corruption() {
    use lib_respect::browser::{RepositoryStats, RepositoryStatsCache};
    let directory = tempfile::tempdir().unwrap();
    let store = Store::at(directory.path().into());
    let data = RepositoryStatsCache {
        schema_version: 1,
        repositories: BTreeMap::from([
            (
                "https://github.com/example/zero".into(),
                RepositoryStats {
                    stars: Some(0),
                    fetched_at: Some(10),
                    checked_at: 10,
                    error: None,
                    ..Default::default()
                },
            ),
            (
                "https://github.com/example/unknown".into(),
                RepositoryStats {
                    stars: None,
                    fetched_at: None,
                    checked_at: 10,
                    error: Some("offline".into()),
                    ..Default::default()
                },
            ),
        ]),
    };
    store.save_repository_stats(&data).unwrap();
    store.save_repository_stats(&data).unwrap();
    let reopened = Store::at(directory.path().into());
    let cache = reopened.repository_stats().unwrap();
    assert_eq!(
        cache.repositories["https://github.com/example/zero"].stars,
        Some(0)
    );
    assert_eq!(
        cache.repositories["https://github.com/example/unknown"].stars,
        None
    );
    assert!(
        cache.repositories["https://github.com/example/unknown"]
            .error
            .is_some()
    );
    let path = directory
        .path()
        .join("cache/github.com/repository-stats.json");
    fs::write(&path, "corrupt").unwrap();
    assert!(reopened.save_repository_stats(&data).is_err());
    assert_eq!(fs::read_to_string(path).unwrap(), "corrupt");
}

#[test]
fn personal_stars_are_account_scoped_and_old_public_counts_are_enriched_without_discarding() {
    use lib_respect::browser::{RepositoryStats, ViewerStar};
    let directory = tempfile::tempdir().unwrap();
    let store = Store::at(directory.path().into());
    let mut first = store.viewer_stars(1).unwrap();
    let mut star = ViewerStar::default();
    star.record(Ok(false), 100);
    first
        .repositories
        .insert("https://github.com/example/lib".into(), star);
    store.save_viewer_stars(&first).unwrap();
    assert!(store.viewer_stars(2).unwrap().repositories.is_empty());
    let reopened = Store::at(directory.path().into());
    assert_eq!(
        reopened.viewer_stars(1).unwrap().repositories["https://github.com/example/lib"].starred,
        Some(false)
    );
    let mut legacy: RepositoryStats =
        serde_json::from_value(json!({"stars":123,"fetched_at":100,"checked_at":100,"error":null}))
            .unwrap();
    assert_eq!(legacy.stars, Some(123));
    assert!(legacy.details_fetched_at.is_none());
    assert!(legacy.due(101));
    legacy.description = Some("Library description".into());
    legacy.details_fetched_at = Some(101);
    assert!(!legacy.due(102));
    let mut mismatched = first;
    mismatched.account_id = 2;
    let path = directory
        .path()
        .join("cache/github.com/1/viewer-stars.json");
    fs::write(&path, serde_json::to_vec(&mismatched).unwrap()).unwrap();
    assert!(store.viewer_stars(1).is_err());
}
