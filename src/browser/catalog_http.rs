use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use super::{Account, Cancellation, Catalog, OwnedRepository, Store, http, now};

#[derive(Deserialize, Serialize)]
struct Cache {
    schema_version: u32,
    account_id: u64,
    pages: BTreeMap<usize, Page>,
}

#[derive(Clone, Deserialize, Serialize)]
struct Page {
    etag: Option<String>,
    next: bool,
    repositories: Vec<OwnedRepository>,
}

/// Validate each page independently; a 304 on page 1 says nothing about page 2.
/// This is an additional HTTP cache; existing catalog/config formats are unchanged.
pub fn discover_cached(account: &Account, cancel: &Cancellation, store: &Store) -> Result<Catalog> {
    discover_with(account, cancel, store, |endpoint, etag| {
        http::get(endpoint, etag, cancel)
    })
}

fn discover_with(
    account: &Account,
    cancel: &Cancellation,
    store: &Store,
    mut get: impl FnMut(&str, Option<&str>) -> Result<http::Response>,
) -> Result<Catalog> {
    let path = format!("cache/github.com/{}/catalog-http.json", account.id);
    let old: Cache = store.read(&path)?.unwrap_or(Cache {
        schema_version: 1,
        account_id: account.id,
        pages: BTreeMap::new(),
    });
    ensure!(
        old.account_id == account.id,
        "HTTP cache account does not match"
    );
    let mut cache = Cache {
        schema_version: 1,
        account_id: account.id,
        pages: BTreeMap::new(),
    };
    let mut repositories = Vec::new();
    for page in 1.. {
        cancel.check()?;
        let previous = old.pages.get(&page);
        let endpoint = format!(
            "user/repos?visibility=public&affiliation=owner&per_page=100&sort=full_name&page={page}"
        );
        let response = get(&endpoint, previous.and_then(|p| p.etag.as_deref()))?;
        let current = if response.status == 304 {
            previous
                .context("GitHub returned 304 without a cached page")?
                .clone()
        } else {
            Page {
                etag: response.etag,
                next: response.next,
                repositories: serde_json::from_slice(&response.body)
                    .context("invalid repository list from GitHub")?,
            }
        };
        // A full final page can acquire a successor without its body changing.
        // Check the next page even if a 304 omitted a new Link header.
        let next = current.next || current.repositories.len() == 100;
        repositories.extend(
            current
                .repositories
                .iter()
                .filter(|repo| repo.eligible(account))
                .cloned(),
        );
        cache.pages.insert(page, current);
        if !next {
            break;
        }
    }
    for repo in &repositories {
        repo.identity()?;
    }
    repositories.sort_by_key(|repo| repo.full_name.to_ascii_lowercase());
    repositories.dedup_by_key(|repo| repo.id);
    cancel.check()?;
    store.write(&path, &cache)?;
    Ok(Catalog {
        schema_version: 1,
        account: account.clone(),
        fetched_at: now(),
        repositories,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account() -> Account {
        Account {
            id: 1,
            login: "example".into(),
        }
    }

    fn repo(id: u64) -> OwnedRepository {
        serde_json::from_value(serde_json::json!({
            "id": id, "name": format!("repo-{id}"), "full_name": format!("example/repo-{id}"),
            "html_url": format!("https://github.com/example/repo-{id}"), "description": null,
            "pushed_at": null, "owner": { "id": 1, "login": "example" },
            "private": false, "archived": false, "fork": false
        }))
        .unwrap()
    }

    fn response(repositories: &[OwnedRepository], next: bool) -> http::Response {
        http::Response {
            status: 200,
            etag: Some("W/\"example\"".into()),
            next,
            body: serde_json::to_vec(repositories).unwrap(),
        }
    }

    #[test]
    fn validates_every_page_and_keeps_successful_cache_when_later_page_fails() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::at(temp.path().into());
        let cancel = Cancellation::default();
        let catalog = discover_with(&account(), &cancel, &store, |endpoint, etag| {
            assert!(etag.is_none());
            Ok(if endpoint.ends_with("page=1") {
                response(&[repo(1)], true)
            } else {
                response(&[repo(2)], false)
            })
        })
        .unwrap();
        assert_eq!(catalog.repositories.len(), 2);
        let mut calls = 0;
        let catalog = discover_with(&account(), &cancel, &store, |endpoint, etag| {
            calls += 1;
            assert_eq!(etag, Some("W/\"example\""));
            Ok(if endpoint.ends_with("page=1") {
                http::Response {
                    status: 304,
                    etag: None,
                    next: false,
                    body: Vec::new(),
                }
            } else {
                response(&[repo(3)], false)
            })
        })
        .unwrap();
        assert_eq!(calls, 2);
        assert_eq!(
            catalog
                .repositories
                .iter()
                .map(|repo| repo.id)
                .collect::<Vec<_>>(),
            [1, 3]
        );
        let path = store.root().join("cache/github.com/1/catalog-http.json");
        let previous = std::fs::read(&path).unwrap();
        assert!(
            discover_with(&account(), &cancel, &store, |endpoint, _| {
                if endpoint.ends_with("page=1") {
                    Ok(response(&[repo(9)], true))
                } else {
                    anyhow::bail!("offline")
                }
            })
            .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), previous);
    }

    #[test]
    fn a_full_page_is_revalidated_with_its_new_successor_and_accounts_are_separate() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::at(temp.path().into());
        let cancel = Cancellation::default();
        let full: Vec<_> = (1..=100).map(repo).collect();
        discover_with(&account(), &cancel, &store, |endpoint, _| {
            Ok(if endpoint.ends_with("page=1") {
                response(&full, false)
            } else {
                response(&[], false)
            })
        })
        .unwrap();
        let catalog = discover_with(&account(), &cancel, &store, |endpoint, _| {
            Ok(if endpoint.ends_with("page=1") {
                http::Response {
                    status: 304,
                    etag: None,
                    next: false,
                    body: Vec::new(),
                }
            } else {
                response(&[repo(101)], false)
            })
        })
        .unwrap();
        assert_eq!(catalog.repositories.len(), 101);
        discover_with(
            &Account {
                id: 2,
                login: "other".into(),
            },
            &cancel,
            &store,
            |_, etag| {
                assert!(etag.is_none());
                Ok(response(&[], false))
            },
        )
        .unwrap();
    }
}
