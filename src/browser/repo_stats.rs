use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use super::{Cancellation, process::graphql};
use crate::GithubRepo;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct RepositoryStats {
    /// None is unknown, never zero stars.
    pub stars: Option<u64>,
    pub fetched_at: Option<u64>,
    pub checked_at: u64,
    pub error: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// Separates an empty description from metadata not yet fetched by older builds.
    #[serde(default)]
    pub details_fetched_at: Option<u64>,
}

impl RepositoryStats {
    pub fn due(&self, time: u64) -> bool {
        let ttl = if self.error.is_some() { 60 } else { 3600 };
        self.checked_at == 0
            || (self.details_fetched_at.is_none() && self.error.is_none())
            || time.saturating_sub(self.checked_at) >= ttl
    }

    pub fn record(&mut self, result: Result<u64>, time: u64) {
        self.checked_at = time;
        match result {
            Ok(stars) => {
                self.stars = Some(stars);
                self.fetched_at = Some(time);
                self.error = None;
            }
            Err(error) => self.error = Some(format!("{error:#}")),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RepositoryStatsCache {
    pub schema_version: u32,
    pub repositories: BTreeMap<String, RepositoryStats>,
}

impl Default for RepositoryStatsCache {
    fn default() -> Self {
        Self {
            schema_version: 1,
            repositories: BTreeMap::new(),
        }
    }
}

/// Read public repository popularity through the user's existing gh authentication.
pub fn repository_stars(repo: &GithubRepo, cancel: &Cancellation) -> Result<u64> {
    let bytes = graphql(
        "query($owner:String!,$name:String!){repository(owner:$owner,name:$name){stargazerCount}}",
        repo,
        cancel,
    )?;
    parse_stars(&bytes)
}

fn parse_stars(bytes: &[u8]) -> Result<u64> {
    let response: serde_json::Value =
        serde_json::from_slice(bytes).context("invalid GitHub response")?;
    if let Some(errors) = response.get("errors") {
        ensure!(
            errors.as_array().is_some_and(|errors| errors.is_empty()),
            "GitHub GraphQL errors: {errors}"
        );
    }
    response
        .pointer("/data/repository/stargazerCount")
        .and_then(serde_json::Value::as_u64)
        .context("GitHub response has no valid stargazerCount")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_count_is_not_zero_and_failed_refresh_keeps_previous_count() {
        assert_eq!(
            parse_stars(br#"{"data":{"repository":{"stargazerCount":0}}}"#).unwrap(),
            0
        );
        for bytes in [
            br#"{}"#.as_slice(),
            br#"{"data":{"repository":{"stargazerCount":null}}}"#,
            br#"{"data":{"repository":{"stargazerCount":-1}}}"#,
            br#"{"data":{"repository":{"stargazerCount":0}},"errors":[{"message":"denied"}]}"#,
        ] {
            assert!(parse_stars(bytes).is_err());
        }
        let mut entry = RepositoryStats::default();
        entry.record(Err(anyhow::anyhow!("network")), 100);
        assert_eq!(entry.stars, None);
        entry.record(Ok(20), 200);
        entry.record(Err(anyhow::anyhow!("network")), 300);
        assert_eq!(entry.stars, Some(20));
        assert_eq!(entry.fetched_at, Some(200));
        assert!(!entry.due(359));
        assert!(entry.due(360));
    }
}
