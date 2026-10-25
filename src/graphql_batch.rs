use anyhow::{Context, Result, ensure};
use serde_json::Value;

use crate::{
    GithubRepo,
    browser::{Cancellation, process::graphql_query},
};

// Bounded queries keep process arguments and server resource usage small.
pub(crate) const BATCH_SIZE: usize = 25;

pub(crate) struct Batch {
    pub viewer_id: u64,
    pub viewer_login: String,
    pub repositories: Vec<Result<Value, String>>,
}

pub(crate) fn fetch(repos: &[GithubRepo], details: bool, cancel: &Cancellation) -> Result<Batch> {
    let bytes = graphql_query(&query(repos, details), &[], cancel)?;
    parse(&bytes, repos.len())
}

fn query(repos: &[GithubRepo], details: bool) -> String {
    let fields = if details {
        "viewerHasStarred stargazerCount description isArchived"
    } else {
        "viewerHasStarred"
    };
    let mut query = "query { viewer { databaseId login }".to_owned();
    for (index, repo) in repos.iter().enumerate() {
        let owner = serde_json::to_string(repo.owner()).unwrap();
        let name = serde_json::to_string(repo.name()).unwrap();
        query.push_str(&format!(
            " r{index}: repository(owner:{owner},name:{name}) {{ {fields} }}"
        ));
    }
    query.push('}');
    query
}

fn parse(bytes: &[u8], count: usize) -> Result<Batch> {
    let response: Value =
        serde_json::from_slice(bytes).context("invalid GitHub GraphQL response")?;
    let errors = match response.get("errors") {
        None => Vec::new(),
        Some(value) => value
            .as_array()
            .context("invalid GitHub GraphQL errors")?
            .clone(),
    };
    // Authentication, rate limits and errors without a repository path must
    // fail the whole operation; never turn these into an unstarred repository.
    for error in &errors {
        ensure!(
            error
                .pointer("/path/0")
                .and_then(Value::as_str)
                .is_some_and(|path| (0..count).any(|i| path == format!("r{i}"))),
            "GitHub GraphQL error: {error}"
        );
    }
    let data = response
        .get("data")
        .context("GitHub response is missing data")?;
    let viewer_id = data
        .pointer("/viewer/databaseId")
        .and_then(Value::as_u64)
        .filter(|id| *id > 0)
        .context("GitHub response is missing viewer.databaseId")?;
    let viewer_login = data
        .pointer("/viewer/login")
        .and_then(Value::as_str)
        .filter(|login| !login.is_empty())
        .context("GitHub response is missing viewer.login")?
        .to_owned();
    let repositories = (0..count)
        .map(|i| {
            let alias = format!("r{i}");
            if let Some(error) = errors
                .iter()
                .find(|e| e.pointer("/path/0").and_then(Value::as_str) == Some(&alias))
            {
                return Err(format!("GitHub GraphQL error: {error}"));
            }
            data.get(&alias)
                .filter(|value| !value.is_null())
                .cloned()
                .ok_or_else(|| "GitHub repository is missing or inaccessible".into())
        })
        .collect();
    Ok(Batch {
        viewer_id,
        viewer_login,
        repositories,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_errors_are_isolated_and_global_errors_fail_the_batch() {
        let bytes = br#"{"data":{"viewer":{"databaseId":1,"login":"example"},"r0":{"viewerHasStarred":false},"r1":null},"errors":[{"message":"missing","path":["r1"]}]}"#;
        let batch = parse(bytes, 2).unwrap();
        assert_eq!(
            batch.repositories[0].as_ref().unwrap()["viewerHasStarred"],
            false
        );
        assert!(
            batch.repositories[1]
                .as_ref()
                .unwrap_err()
                .contains("missing")
        );
        let mut response: Value = serde_json::from_slice(bytes).unwrap();
        response["errors"][0]
            .as_object_mut()
            .unwrap()
            .remove("path");
        assert!(parse(&serde_json::to_vec(&response).unwrap(), 2).is_err());
    }

    #[test]
    fn multiple_repositories_share_one_query_and_star_only_omits_metadata() {
        let repos = [
            GithubRepo::parse("https://github.com/example/one").unwrap(),
            GithubRepo::parse("https://github.com/example/two").unwrap(),
        ];
        let query = query(&repos, false);
        assert!(query.contains("r0: repository("));
        assert!(query.contains("r1: repository("));
        assert!(!query.contains("stargazerCount"));
    }
}
