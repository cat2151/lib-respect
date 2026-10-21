use std::{
    sync::mpsc::{self, Sender},
    thread::{self, JoinHandle},
};

use lib_respect::{
    GithubRepo,
    browser::{Account, Cancellation, Store, now, repository_details},
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
    account: Account,
    cancel: Cancellation,
}

impl StatsWorker {
    pub fn new(store: Store, events: Sender<Message>) -> Self {
        let (sender, jobs) = mpsc::channel::<StatsJob>();
        let handle = thread::spawn(move || {
            while let Ok(mut job) = jobs.recv() {
                while let Ok(newer) = jobs.try_recv() {
                    job = newer;
                }
                let StatsJob {
                    generation,
                    urls,
                    account,
                    cancel,
                } = job;
                let result = (|| -> anyhow::Result<()> {
                    cancel.check()?;
                    let mut cache = store.repository_stats()?;
                    let mut viewer = store.viewer_stars(account.id)?;
                    for url in urls {
                        cancel.check()?;
                        let repo = GithubRepo::parse(&url)
                            .ok_or_else(|| anyhow::anyhow!("invalid repository URL: {url}"))?;
                        let entry = cache.repositories.entry(repo.url()).or_default();
                        let star = viewer.repositories.entry(repo.url()).or_default();
                        if entry.due(now()) || star.due(now()) {
                            let result = repository_details(&repo, &account, &cancel);
                            cancel.check()?;
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
                            let updated = entry.clone();
                            let updated_star = star.clone();
                            store.save_repository_stats(&cache)?;
                            store.save_viewer_stars(&viewer)?;
                            let _ = events.send(Message::RepoStats(
                                generation,
                                repo.url(),
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

    pub fn refresh(&mut self, account: Account, urls: Vec<String>) -> u64 {
        self.cancel.cancel();
        self.cancel = Cancellation::default();
        self.generation += 1;
        if let Some(sender) = &self.jobs {
            let _ = sender.send(StatsJob {
                generation: self.generation,
                urls,
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
