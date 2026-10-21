use std::time::Duration;

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use super::{Cancellation, LibraryLookup};

pub struct CratesIo {
    agent: ureq::Agent,
}

impl Default for CratesIo {
    fn default() -> Self {
        Self {
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(5)))
                .build()
                .into(),
        }
    }
}

impl LibraryLookup for CratesIo {
    fn repository(&self, name: &str, cancel: &Cancellation) -> Result<Option<String>> {
        cancel.check()?;
        ensure!(
            !name.is_empty()
                && name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c)),
            "invalid crates.io crate name: {name:?}"
        );
        let url = format!("https://crates.io/api/v1/crates/{name}?include=default_version");
        let body = self
            .agent
            .get(&url)
            .header(
                "User-Agent",
                concat!(
                    "lib-respect/",
                    env!("CARGO_PKG_VERSION"),
                    " (Rust dependency repository discovery)"
                ),
            )
            .call()
            .with_context(|| format!("GET {url}"))?
            .body_mut()
            .read_to_string()?;
        cancel.check()?;
        parse_repository(&body, name)
    }
}

fn parse_repository(body: &str, expected: &str) -> Result<Option<String>> {
    #[derive(Deserialize)]
    struct Response {
        #[serde(rename = "crate")]
        krate: Metadata,
    }
    #[derive(Deserialize)]
    struct Metadata {
        name: String,
        repository: Option<String>,
    }
    let response: Response = serde_json::from_str(body).context("invalid crates.io metadata")?;
    let canonical = |name: &str| name.to_ascii_lowercase().replace('_', "-");
    ensure!(
        canonical(&response.krate.name) == canonical(expected),
        "crates.io returned a different crate"
    );
    Ok(response.krate.repository.filter(|s| !s.trim().is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_or_wrong_crate_is_not_treated_as_missing_repository() {
        assert!(parse_repository("{}", "serde").is_err());
        assert!(
            parse_repository(r#"{"crate":{"name":"other","repository":null}}"#, "serde").is_err()
        );
        assert_eq!(
            parse_repository(r#"{"crate":{"name":"serde","repository":null}}"#, "serde").unwrap(),
            None
        );
    }
}
