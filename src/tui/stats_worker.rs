use std::{
    collections::BTreeSet,
    sync::mpsc::{self, Sender},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use lib_respect::{
    GithubRepo,
    browser::{Account, Cancellation, RepositoryDetails, Store, now, repository_details_batch},
};

use super::workers::Message;

pub struct StatsWorker {
    jobs: Option<Sender<StatsJob>>,
    cancel: Cancellation,
    handle: Option<JoinHandle<()>>,
    generation: u64,
}

struct StatsJob {
    generation: u64,
    urls: Vec<String>,
    force: BTreeSet<String>,
    account: Account,
    cancel: Cancellation,
}

impl StatsWorker {
    pub fn new(store: Store, events: Sender<Message>) -> Self {
        Self::with_lookup(store, events, repository_details_batch)
    }

    fn with_lookup(
        store: Store,
        events: Sender<Message>,
        lookup: impl Fn(
            &[GithubRepo],
            &Account,
            &Cancellation,
        ) -> anyhow::Result<Vec<Result<RepositoryDetails, String>>>
        + Send
        + 'static,
    ) -> Self {
        let (sender, jobs) = mpsc::channel::<StatsJob>();
        let handle = thread::spawn(move || {
            let mut backoff: Option<(u64, Instant, Duration)> = None;
            while let Ok(mut job) = jobs.recv() {
                while let Ok(newer) = jobs.try_recv() {
                    job = newer;
                }
                let StatsJob {
                    generation,
                    urls,
                    force,
                    account,
                    cancel,
                } = job;
                let result = (|| -> anyhow::Result<()> {
                    cancel.check()?;
                    let mut cache = store.repository_stats()?;
                    let mut viewer = store.viewer_stars(account.id)?;
                    let mut due = Vec::new();
                    for url in urls {
                        let repo = GithubRepo::parse(&url)
                            .ok_or_else(|| anyhow::anyhow!("invalid repository URL: {url}"))?;
                        if force.contains(&repo.url())
                            || cache
                                .repositories
                                .get(&repo.url())
                                .is_none_or(|e| e.due(now()))
                            || viewer
                                .repositories
                                .get(&repo.url())
                                .is_none_or(|e| e.due(now()))
                        {
                            due.push(repo);
                        }
                    }
                    for repos in due.chunks(25) {
                        cancel.check()?;
                        if let Some((id, until, _)) = backoff
                            && id == account.id
                            && Instant::now() < until
                        {
                            anyhow::bail!("GitHub requests paused after a failure; retry later");
                        }
                        let results = match lookup(repos, &account, &cancel) {
                            Ok(results) => {
                                backoff = None;
                                results
                            }
                            Err(error) => {
                                cancel.check()?;
                                let delay = backoff
                                    .filter(|(id, _, _)| *id == account.id)
                                    .map_or(Duration::from_secs(60), |(_, _, delay)| {
                                        (delay * 2).min(Duration::from_secs(3600))
                                    });
                                backoff = Some((account.id, Instant::now() + delay, delay));
                                return Err(error);
                            }
                        };
                        anyhow::ensure!(
                            results.len() == repos.len(),
                            "incomplete repository batch"
                        );
                        let mut updates = Vec::new();
                        for (repo, result) in repos.iter().zip(results) {
                            cancel.check()?;
                            let entry = cache.repositories.entry(repo.url()).or_default();
                            let star = viewer.repositories.entry(repo.url()).or_default();
                            let time = now();
                            match result {
                                Ok(details) => {
                                    entry.record(Ok(details.stars), time);
                                    entry.description = details.description;
                                    entry.details_fetched_at = Some(time);
                                    star.record(Ok(details.viewer_has_starred), time);
                                }
                                Err(error) => {
                                    let error = format!("{error:#}");
                                    entry.record(Err(anyhow::anyhow!(error.clone())), time);
                                    star.record(Err(anyhow::anyhow!(error)), time);
                                }
                            }
                            updates.push((repo.url(), entry.clone(), star.clone()));
                        }
                        cancel.check()?;
                        store.save_repository_stats(&cache)?;
                        store.save_viewer_stars(&viewer)?;
                        for (url, updated, updated_star) in updates {
                            let _ = events.send(Message::RepoStats(
                                generation,
                                url,
                                updated,
                                updated_star,
                            ));
                        }
                    }
                    Ok(())
                })();
                if !cancel.is_cancelled() {
                    let _ = events.send(Message::RepoStatsDone(
                        generation,
                        result.err().map(|e| format!("{e:#}")),
                    ));
                }
            }
        });
        Self {
            jobs: Some(sender),
            cancel: Cancellation::default(),
            handle: Some(handle),
            generation: 0,
        }
    }

    pub fn refresh(
        &mut self,
        account: Account,
        mut urls: Vec<String>,
        force: BTreeSet<String>,
    ) -> u64 {
        // Visited repositories remain targets even when a filter hides them.
        urls.retain(|url| !force.contains(url));
        urls.splice(0..0, force.iter().cloned());
        self.cancel.cancel();
        self.cancel = Cancellation::default();
        self.generation += 1;
        if let Some(sender) = &self.jobs {
            let _ = sender.send(StatsJob {
                generation: self.generation,
                urls,
                force,
                account,
                cancel: self.cancel.clone(),
            });
        }
        self.generation
    }

    pub fn cancel(&self) {
        self.cancel.cancel();
    }
}

impl Drop for StatsWorker {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.jobs.take();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
#[path = "stats_worker_tests.rs"]
mod tests;
