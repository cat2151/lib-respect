use std::{
    sync::mpsc::{self, Receiver, Sender},
    thread::{self, JoinHandle},
};

use lib_respect::browser::{
    Account, Cancellation, Catalog, CratesIo, DependencyCache, INSPECTION_VERSION, OwnedRepository,
    RepositoryStats, Store, ViewerStar, discover_cached, enrich_libraries, identify_account,
    inspect_repository_cached, now,
};

use super::stats_worker::StatsWorker;

pub enum Message {
    Account(Account),
    Catalog(Catalog, bool, Option<String>),
    CatalogError(String),
    Dependencies(u64, u64, Result<DependencyCache, String>, Option<String>),
    DependencyProgress(u64, u64, DependencyCache),
    RepoStats(u64, String, RepositoryStats, ViewerStar),
    RepoStatsDone(u64, Option<String>),
}

pub struct Inspection {
    pub account_id: u64,
    pub repo: OwnedRepository,
    pub cancel: Cancellation,
    pub force: bool,
}

pub struct Workers {
    pub messages: Receiver<Message>,
    catalog: Option<Sender<Cancellation>>,
    dependencies: Option<Sender<Inspection>>,
    handles: Vec<JoinHandle<()>>,
    pub catalog_cancel: Cancellation,
    pub dependency_cancel: Cancellation,
    pub stats: StatsWorker,
}

impl Workers {
    pub fn new(store: Store) -> Self {
        let (sender, messages) = mpsc::channel();
        let stats = StatsWorker::new(store.clone(), sender.clone());
        let (catalog_sender, catalog_jobs) = mpsc::channel::<Cancellation>();
        let (dependency_sender, dependency_jobs) = mpsc::channel::<Inspection>();
        let catalog_store = store.clone();
        let catalog_events = sender.clone();
        let catalog = thread::spawn(move || {
            while let Ok(cancel) = catalog_jobs.recv() {
                let result = (|| -> anyhow::Result<()> {
                    let account = identify_account(&cancel)?;
                    let _ = catalog_events.send(Message::Account(account.clone()));
                    if let Some(cache) = catalog_store.catalog(&account)? {
                        let _ = catalog_events.send(Message::Catalog(cache, true, None));
                    }
                    let catalog = discover_cached(&account, &cancel, &catalog_store)?;
                    cancel.check()?;
                    let warning = catalog_store
                        .save_catalog(&catalog)
                        .err()
                        .map(|e| format!("cache save: {e:#}"));
                    let _ = catalog_events.send(Message::Catalog(catalog, false, warning));
                    Ok(())
                })();
                if !cancel.is_cancelled()
                    && let Err(error) = result
                {
                    let _ = catalog_events.send(Message::CatalogError(format!("{error:#}")));
                }
            }
        });
        let dependencies = thread::spawn(move || {
            let registry = CratesIo::default();
            while let Ok(mut job) = dependency_jobs.recv() {
                // A held j/k key should fetch the latest selection, not a backlog.
                while let Ok(newer) = dependency_jobs.try_recv() {
                    job = newer;
                }
                if job.cancel.is_cancelled() {
                    continue;
                }
                let result = (|| -> anyhow::Result<DependencyCache> {
                    let cached = store.dependencies(job.account_id, &job.repo)?;
                    let mut data = match cached.filter(|data| {
                        !job.force
                            && data.report.inspection_version == INSPECTION_VERSION
                            && data.pushed_at == job.repo.pushed_at
                    }) {
                        Some(data) => data,
                        None => DependencyCache {
                            schema_version: 1,
                            account_id: job.account_id,
                            repo_id: job.repo.id,
                            pushed_at: job.repo.pushed_at.clone(),
                            fetched_at: now(),
                            report: inspect_repository_cached(
                                &job.repo,
                                job.account_id,
                                &store,
                                &job.cancel,
                            )?,
                        },
                    };
                    job.cancel.check()?;
                    let _ = sender.send(Message::DependencyProgress(
                        job.account_id,
                        job.repo.id,
                        data.clone(),
                    ));
                    let template = data.clone();
                    enrich_libraries(
                        &job.repo.html_url,
                        &mut data.report,
                        &store,
                        &registry,
                        &job.cancel,
                        |report| {
                            if !job.cancel.is_cancelled() {
                                let mut updated = template.clone();
                                updated.report = report.clone();
                                let _ = sender.send(Message::DependencyProgress(
                                    job.account_id,
                                    job.repo.id,
                                    updated,
                                ));
                            }
                        },
                    )?;
                    Ok(data)
                })();
                if job.cancel.is_cancelled() {
                    continue;
                }
                let warning = result
                    .as_ref()
                    .ok()
                    .and_then(|data| store.save_dependencies(data).err())
                    .map(|e| format!("cache save: {e:#}"));
                let _ = sender.send(Message::Dependencies(
                    job.account_id,
                    job.repo.id,
                    result.map_err(|e| format!("{e:#}")),
                    warning,
                ));
            }
        });
        Self {
            messages,
            catalog: Some(catalog_sender),
            dependencies: Some(dependency_sender),
            handles: vec![catalog, dependencies],
            catalog_cancel: Cancellation::default(),
            dependency_cancel: Cancellation::default(),
            stats,
        }
    }

    pub fn refresh_catalog(&mut self) {
        self.catalog_cancel.cancel();
        self.catalog_cancel = Cancellation::default();
        if let Some(sender) = &self.catalog {
            let _ = sender.send(self.catalog_cancel.clone());
        }
    }

    pub fn cancel_inspection(&mut self) {
        self.dependency_cancel.cancel();
    }

    pub fn inspect(&mut self, account_id: u64, repo: OwnedRepository, force: bool) {
        self.dependency_cancel = Cancellation::default();
        if let Some(sender) = &self.dependencies {
            let _ = sender.send(Inspection {
                account_id,
                repo,
                cancel: self.dependency_cancel.clone(),
                force,
            });
        }
    }
}

impl Drop for Workers {
    fn drop(&mut self) {
        self.catalog_cancel.cancel();
        self.dependency_cancel.cancel();
        self.stats.cancel();
        self.catalog.take();
        self.dependencies.take();
        for handle in self.handles.drain(..) {
            let _ = handle.join();
        }
    }
}
