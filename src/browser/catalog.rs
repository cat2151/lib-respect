use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use super::{Cancellation, now, process::api};
use crate::GithubRepo;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Account {
    pub id: u64,
    pub login: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct OwnedRepository {
    pub id: u64,
    pub name: String,
    pub full_name: String,
    pub html_url: String,
    pub description: Option<String>,
    pub pushed_at: Option<String>,
    pub owner: Account,
    pub private: bool,
    pub archived: bool,
    pub fork: bool,
}

impl OwnedRepository {
    pub fn identity(&self) -> Result<GithubRepo> {
        GithubRepo::parse(&self.html_url).context("invalid repository URL from GitHub")
    }
    pub fn eligible(&self, account: &Account) -> bool {
        self.owner.id == account.id && !self.private && !self.archived && !self.fork
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Catalog {
    pub schema_version: u32,
    pub account: Account,
    pub fetched_at: u64,
    pub repositories: Vec<OwnedRepository>,
}

pub fn identify_account(cancel: &Cancellation) -> Result<Account> {
    let account: Account = serde_json::from_slice(&api("user", false, false, cancel)?)?;
    ensure!(
        account.id > 0 && !account.login.is_empty(),
        "invalid authenticated GitHub account"
    );
    Ok(account)
}

pub fn discover(account: &Account, cancel: &Cancellation) -> Result<Catalog> {
    let pages: Vec<Vec<OwnedRepository>> = serde_json::from_slice(&api(
        "user/repos?visibility=public&affiliation=owner&per_page=100&sort=full_name",
        false,
        true,
        cancel,
    )?)
    .context("invalid repository list from GitHub")?;
    let mut repositories: Vec<_> = pages
        .into_iter()
        .flatten()
        .filter(|repo| repo.eligible(account))
        .collect();
    for repo in &repositories {
        repo.identity()?;
    }
    repositories.sort_by_key(|repo| repo.full_name.to_ascii_lowercase());
    repositories.dedup_by_key(|repo| repo.id);
    Ok(Catalog {
        schema_version: 1,
        account: account.clone(),
        fetched_at: now(),
        repositories,
    })
}
