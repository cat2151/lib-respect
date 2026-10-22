use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use super::{Account, Cancellation, process::graphql};
use crate::GithubRepo;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ViewerStar {
    pub starred: Option<bool>,
    pub fetched_at: Option<u64>,
    pub checked_at: u64,
    pub error: Option<String>,
}

impl ViewerStar {
    pub fn due(&self, time: u64) -> bool {
        let ttl = if self.error.is_some() { 60 } else { 3600 };
        self.checked_at == 0 || time.saturating_sub(self.checked_at) >= ttl
    }

    pub fn record(&mut self, result: Result<bool>, time: u64) {
        self.checked_at = time;
        match result {
            Ok(starred) => {
                self.starred = Some(starred);
                self.fetched_at = Some(time);
                self.error = None;
            }
            Err(error) => self.error = Some(format!("{error:#}")),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ViewerStarsCache {
    pub schema_version: u32,
    pub account_id: u64,
    pub repositories: BTreeMap<String, ViewerStar>,
}

impl ViewerStarsCache {
    pub fn new(account_id: u64) -> Self {
        Self {
            schema_version: 1,
            account_id,
            repositories: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryDetails {
    #[serde(rename = "stargazerCount")]
    pub stars: u64,
    pub description: Option<String>,
    pub viewer_has_starred: bool,
}

/// Query popularity, description and personal star status together. Validate the
/// actual authenticated account on every response before caching personal data.
pub fn repository_details(
    repo: &GithubRepo,
    account: &Account,
    cancel: &Cancellation,
) -> Result<RepositoryDetails> {
    let query = "query($owner:String!,$name:String!){viewer{databaseId} repository(owner:$owner,name:$name){stargazerCount description viewerHasStarred}}";
    let bytes = graphql(query, repo, cancel)?;
    parse_details(&bytes, account)
}

/// Fetch a bounded group of repositories in one request, retaining per-repo errors.
pub fn repository_details_batch(
    repos: &[GithubRepo],
    account: &Account,
    cancel: &Cancellation,
) -> Result<Vec<Result<RepositoryDetails, String>>> {
    ensure!(
        repos.len() <= crate::graphql_batch::BATCH_SIZE,
        "repository batch is too large"
    );
    let batch = crate::graphql_batch::fetch(repos, true, cancel)?;
    ensure!(
        batch.viewer_id == account.id,
        "GitHub account changed; restart to identify the current account"
    );
    Ok(batch
        .repositories
        .into_iter()
        .map(|value| {
            value.and_then(|value| {
                serde_json::from_value(value)
                    .map_err(|error| format!("invalid repository details: {error}"))
            })
        })
        .collect())
}

fn parse_details(bytes: &[u8], account: &Account) -> Result<RepositoryDetails> {
    #[derive(Deserialize)]
    struct Response {
        data: Option<Data>,
        errors: Option<serde_json::Value>,
    }
    #[derive(Deserialize)]
    struct Data {
        viewer: Viewer,
        repository: Option<RepositoryDetails>,
    }
    #[derive(Deserialize)]
    struct Viewer {
        #[serde(rename = "databaseId")]
        id: u64,
    }
    let response: Response =
        serde_json::from_slice(bytes).context("invalid GitHub repository details")?;
    if let Some(errors) = response.errors {
        ensure!(
            errors.as_array().is_some_and(|a| a.is_empty()),
            "GitHub GraphQL errors: {errors}"
        );
    }
    let data = response
        .data
        .context("GitHub repository details are missing")?;
    ensure!(
        data.viewer.id == account.id,
        "GitHub account changed; restart to identify the current account"
    );
    data.repository
        .context("GitHub repository is missing or inaccessible")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn false_is_valid_but_missing_partial_errors_and_different_accounts_are_not() {
        let account = Account {
            id: 1,
            login: "example".into(),
        };
        let valid = br#"{"data":{"viewer":{"databaseId":1},"repository":{"stargazerCount":0,"description":null,"viewerHasStarred":false}}}"#;
        let details = parse_details(valid, &account).unwrap();
        assert!(!details.viewer_has_starred);
        assert_eq!(details.description, None);
        let mut value: serde_json::Value = serde_json::from_slice(valid).unwrap();
        value["errors"] = serde_json::json!([{"message":"denied"}]);
        assert!(parse_details(&serde_json::to_vec(&value).unwrap(), &account).is_err());
        value.as_object_mut().unwrap().remove("errors");
        value["data"]["viewer"]["databaseId"] = serde_json::json!(2);
        assert!(parse_details(&serde_json::to_vec(&value).unwrap(), &account).is_err());
        value["data"]["viewer"]["databaseId"] = serde_json::json!(1);
        value["data"]["repository"]
            .as_object_mut()
            .unwrap()
            .remove("viewerHasStarred");
        assert!(parse_details(&serde_json::to_vec(&value).unwrap(), &account).is_err());
        value["data"]["repository"] = serde_json::Value::Null;
        assert!(parse_details(&serde_json::to_vec(&value).unwrap(), &account).is_err());
    }

    #[test]
    fn refresh_failure_keeps_previous_state_and_unknown_is_not_false() {
        let mut star = ViewerStar::default();
        star.record(Err(anyhow::anyhow!("offline")), 100);
        assert_eq!(star.starred, None);
        star.record(Ok(true), 200);
        star.record(Err(anyhow::anyhow!("offline")), 300);
        assert_eq!(star.starred, Some(true));
        assert_eq!(star.fetched_at, Some(200));
    }
}
