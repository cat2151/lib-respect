use std::cell::RefCell;

use anyhow::{Result, bail};
use lib_respect::{DependencyReport, GithubRepo, StarLookup, collect_stars};
use serde_json::json;

#[derive(Default)]
struct FakeGithub {
    calls: RefCell<Vec<String>>,
    fail_auth: bool,
}

impl StarLookup for FakeGithub {
    fn viewer(&self) -> Result<String> {
        if self.fail_auth {
            bail!("authentication failed")
        }
        Ok("example-user".into())
    }

    fn has_starred(&self, repository: &GithubRepo) -> Result<bool> {
        self.calls.borrow_mut().push(repository.url());
        match repository.name() {
            "starred" => Ok(true),
            "unstarred" => Ok(false),
            _ => bail!("repository unavailable"),
        }
    }
}

fn report() -> DependencyReport {
    let dependency = |name: &str, url: Option<&str>| {
        json!({
            "package_id": format!("opaque-{name}"), "name":name, "version":"1.0.0",
            "dependency_name": name, "kinds":[{"kind":"normal","target":null}],
            "source":null, "repository": url,
            "github":url.map(|url| json!({"url":url,"identified_by":"package_repository"}))
        })
    };
    serde_json::from_value(json!({
        "schema_version":1, "workspace_root":"X:/workspace",
        "packages":[{"id":"opaque-app", "name":"app", "version":"0.1.0", "manifest_path":"X:/workspace/Cargo.toml", "dependencies":[
            dependency("one", Some("https://github.com/Owner/Starred.git")),
            dependency("two", Some("https://github.com/owner/starred/tree/main/subcrate")),
            dependency("three", Some("https://github.com/owner/unstarred")),
            dependency("four", Some("https://github.com/owner/missing")),
            dependency("unknown", None)
        ]}]
    })).unwrap()
}

#[test]
fn groups_repositories_and_preserves_true_false_and_unknown_distinctly() {
    let mut second = report();
    second.workspace_root = "X:/another-workspace".into();
    let lookup = FakeGithub::default();
    let result = collect_stars(&[report(), report(), second], &lookup).unwrap();
    assert_eq!(result.viewer, "example-user");
    assert_eq!(lookup.calls.borrow().len(), 3);
    assert_eq!(result.repositories.len(), 3);
    let get = |name| result.repositories.iter().find(|r| r.name == name).unwrap();
    assert_eq!(get("starred").starred, Some(true));
    assert_eq!(get("starred").used_by.len(), 4);
    assert_eq!(get("unstarred").starred, Some(false));
    assert!(get("unstarred").error.is_none());
    assert_eq!(get("missing").starred, None);
    assert!(
        get("missing")
            .error
            .as_ref()
            .unwrap()
            .contains("unavailable")
    );
    assert_eq!(result.unmapped_dependencies.len(), 2);
    assert!(result.has_errors());
}

#[test]
fn auth_failure_prevents_repository_checks() {
    let lookup = FakeGithub {
        fail_auth: true,
        ..Default::default()
    };
    assert!(
        collect_stars(&[report()], &lookup)
            .unwrap_err()
            .to_string()
            .contains("authenticated")
    );
    assert!(lookup.calls.borrow().is_empty());
}

#[test]
fn rejects_unsupported_input_schema_and_invalid_repository_links() {
    let lookup = FakeGithub::default();
    let mut input = report();
    input.schema_version = 2;
    assert!(collect_stars(&[input], &lookup).is_err());
    let mut input = report();
    input.packages[0].dependencies[0]
        .github
        .as_mut()
        .unwrap()
        .url = "https://example.com/owner/repo".into();
    assert!(collect_stars(&[input], &lookup).is_err());
    assert!(lookup.calls.borrow().is_empty());
}
