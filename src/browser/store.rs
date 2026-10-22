use std::{
    fs,
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use super::{
    Account, Catalog, LibraryCache, ManifestReport, OwnedRepository, RepositoryStatsCache,
    UiPreferences, ViewerStarsCache,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Config {
    pub schema_version: u32,
    pub account: Option<Account>,
    pub selected_repo: Option<u64>,
    #[serde(default)]
    pub ui: UiPreferences,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: 1,
            account: None,
            selected_repo: None,
            ui: UiPreferences::default(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DependencyCache {
    pub schema_version: u32,
    pub account_id: u64,
    pub repo_id: u64,
    pub pushed_at: Option<String>,
    pub fetched_at: u64,
    pub report: ManifestReport,
}

#[derive(Clone, Debug)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn local() -> Result<Self> {
        Ok(Self::at(
            dirs::config_local_dir()
                .context("cannot locate the local application data directory")?
                .join("lib-respect"),
        ))
    }
    pub fn at(root: PathBuf) -> Self {
        Self { root }
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn libraries(&self) -> Result<LibraryCache> {
        Ok(self.read("cache/libraries.json")?.unwrap_or_default())
    }
    pub fn repository_stats(&self) -> Result<RepositoryStatsCache> {
        Ok(self
            .read("cache/github.com/repository-stats.json")?
            .unwrap_or_default())
    }
    pub fn save_repository_stats(&self, data: &RepositoryStatsCache) -> Result<()> {
        self.write("cache/github.com/repository-stats.json", data)
    }
    pub fn viewer_stars(&self, account_id: u64) -> Result<ViewerStarsCache> {
        let data: ViewerStarsCache = self
            .read(&format!("cache/github.com/{account_id}/viewer-stars.json"))?
            .unwrap_or_else(|| ViewerStarsCache::new(account_id));
        ensure!(
            data.account_id == account_id,
            "star cache account does not match"
        );
        Ok(data)
    }
    pub fn save_viewer_stars(&self, data: &ViewerStarsCache) -> Result<()> {
        self.write(
            &format!("cache/github.com/{}/viewer-stars.json", data.account_id),
            data,
        )
    }
    pub fn save_libraries(&self, data: &LibraryCache) -> Result<()> {
        self.write("cache/libraries.json", data)
    }
    pub fn config(&self) -> Result<Config> {
        Ok(self.read("config.json")?.unwrap_or_default())
    }
    pub fn save_config(&self, config: &Config) -> Result<()> {
        // Preserve settings from an unsupported/newer UI mode rather than overwrite them.
        let _ = self.config()?;
        self.write("config.json", config)
    }
    pub fn catalog(&self, account: &Account) -> Result<Option<Catalog>> {
        let data: Option<Catalog> =
            self.read(&format!("cache/github.com/{}/repos.json", account.id))?;
        if let Some(catalog) = &data {
            ensure!(
                catalog.account.id == account.id,
                "cached catalog account does not match"
            );
            ensure!(
                catalog.repositories.iter().all(|r| r.eligible(account)),
                "cached catalog contains ineligible repositories"
            );
        }
        Ok(data)
    }
    pub fn save_catalog(&self, catalog: &Catalog) -> Result<()> {
        self.write(
            &format!("cache/github.com/{}/repos.json", catalog.account.id),
            catalog,
        )
    }
    pub fn dependencies(
        &self,
        account_id: u64,
        repo: &OwnedRepository,
    ) -> Result<Option<DependencyCache>> {
        let data: Option<DependencyCache> = self.read(&format!(
            "cache/github.com/{account_id}/dependencies/{}.json",
            repo.id
        ))?;
        if let Some(data) = &data {
            ensure!(
                data.account_id == account_id && data.repo_id == repo.id,
                "dependency cache identity does not match"
            );
        }
        Ok(data)
    }
    pub fn save_dependencies(&self, data: &DependencyCache) -> Result<()> {
        self.write(
            &format!(
                "cache/github.com/{}/dependencies/{}.json",
                data.account_id, data.repo_id
            ),
            data,
        )
    }
    pub(super) fn read<T: DeserializeOwned>(&self, relative: &str) -> Result<Option<T>> {
        let path = self.root.join(relative);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(error).with_context(|| format!("cannot read {}", path.display()));
            }
        };
        let value: serde_json::Value = serde_json::from_str(&text)
            .with_context(|| format!("invalid JSON in {}", path.display()))?;
        ensure!(
            value["schema_version"] == 1,
            "unsupported cache schema in {}",
            path.display()
        );
        Ok(Some(serde_json::from_value(value).with_context(|| {
            format!("invalid cache in {}", path.display())
        })?))
    }
    pub(super) fn write(&self, relative: &str, value: &impl Serialize) -> Result<()> {
        let path = self.root.join(relative);
        // Refuse to overwrite unknown or corrupt existing data.
        let _: Option<serde_json::Value> = self.read(relative)?;
        let parent = path.parent().context("cache path has no parent")?;
        fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        serde_json::to_writer_pretty(&mut temporary, value)?;
        temporary.write_all(b"\n")?;
        temporary.as_file().sync_all()?;
        temporary
            .persist(&path)
            .with_context(|| format!("cannot save {}", path.display()))?;
        Ok(())
    }
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
