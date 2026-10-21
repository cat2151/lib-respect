use std::process::Command;

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::{GithubRepo, StarLookup};

/// Uses the existing `gh` authentication for github.com; performs only queries.
#[derive(Clone, Copy, Debug, Default)]
pub struct GhClient;

impl StarLookup for GhClient {
    fn viewer(&self) -> Result<String> {
        let data = graphql("query { viewer { login } }", &[])?;
        data.pointer("/viewer/login")
            .and_then(Value::as_str)
            .filter(|login| !login.is_empty())
            .map(str::to_owned)
            .context("GitHub response is missing viewer.login")
    }

    fn has_starred(&self, repository: &GithubRepo) -> Result<bool> {
        let data = graphql(
            "query($owner: String!, $name: String!) { repository(owner: $owner, name: $name) { viewerHasStarred } }",
            &[("owner", repository.owner()), ("name", repository.name())],
        )?;
        data.pointer("/repository/viewerHasStarred")
            .and_then(Value::as_bool)
            .context("GitHub repository is missing or viewerHasStarred is unavailable")
    }
}

fn graphql(query: &str, variables: &[(&str, &str)]) -> Result<Value> {
    let mut command = Command::new("gh");
    command
        .args(["api", "graphql", "--hostname", "github.com", "--raw-field"])
        .arg(format!("query={query}"))
        .env("GH_PROMPT_DISABLED", "1");
    for (key, value) in variables {
        command.arg("--raw-field").arg(format!("{key}={value}"));
    }
    let output = command
        .output()
        .context("cannot run gh; install GitHub CLI and run gh auth login")?;
    if !output.status.success() {
        bail!(
            "gh api graphql failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    parse_response(&output.stdout)
}

fn parse_response(bytes: &[u8]) -> Result<Value> {
    let mut response: Value = serde_json::from_slice(bytes).context("gh returned invalid JSON")?;
    if let Some(errors) = response.get("errors") {
        match errors.as_array() {
            Some(errors) if errors.is_empty() => {}
            _ => bail!("GitHub GraphQL errors: {errors}"),
        }
    }
    let data = response
        .get_mut("data")
        .context("GitHub response is missing data")?
        .take();
    if data.is_null() {
        bail!("GitHub returned null data");
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_graphql_errors_even_with_partial_data() {
        let response = br#"{"data":{"repository":{"viewerHasStarred":false}},"errors":[{"message":"access denied"}]}"#;
        assert!(
            parse_response(response)
                .unwrap_err()
                .to_string()
                .contains("access denied")
        );
        for response in [b"not json".as_slice(), br#"{}"#, br#"{"data":null}"#] {
            assert!(parse_response(response).is_err());
        }
    }

    #[test]
    fn accepts_valid_false_without_treating_it_as_missing() {
        let data =
            parse_response(br#"{"data":{"repository":{"viewerHasStarred":false}}}"#).unwrap();
        assert_eq!(data["repository"]["viewerHasStarred"], false);
    }
}
