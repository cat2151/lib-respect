use super::*;
use lib_respect::browser::{RepositoryStats, RepositoryStatsCache, ViewerStar, ViewerStarsCache};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

#[test]
fn batches_due_repositories_and_rechecks_only_visited_fresh_entries() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::at(temp.path().into());
    let urls: Vec<String> = (0..26)
        .map(|i| format!("https://github.com/example/repo-{i}"))
        .collect();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let calls = requests.clone();
    let (events, messages) = mpsc::channel();
    let mut worker = StatsWorker::with_lookup(store.clone(), events, move |repos, _, _| {
        calls
            .lock()
            .unwrap()
            .push(repos.iter().map(|repo| repo.url()).collect::<Vec<_>>());
        Ok(repos
            .iter()
            .map(|_| {
                Ok(RepositoryDetails {
                    stars: 42,
                    description: None,
                    viewer_has_starred: true,
                    archived: true,
                })
            })
            .collect())
    });
    let account = Account {
        id: 1,
        login: "example".into(),
    };
    let wait = |generation| loop {
        if let Message::RepoStatsDone(g, error) =
            messages.recv_timeout(Duration::from_secs(5)).unwrap()
        {
            assert_eq!(g, generation);
            assert!(error.is_none(), "{error:?}");
            break;
        }
    };
    let generation = worker.refresh(account.clone(), urls.clone(), BTreeSet::new());
    wait(generation);
    assert_eq!(
        requests
            .lock()
            .unwrap()
            .iter()
            .map(Vec::len)
            .collect::<Vec<_>>(),
        [25, 1]
    );
    let generation = worker.refresh(account.clone(), urls.clone(), BTreeSet::new());
    wait(generation);
    assert_eq!(requests.lock().unwrap().len(), 2);
    // A visited target is checked even when absent from the current list.
    let generation = worker.refresh(account, Vec::new(), BTreeSet::from([urls[0].clone()]));
    wait(generation);
    assert_eq!(requests.lock().unwrap()[2], [urls[0].clone()]);
    assert_eq!(
        store.viewer_stars(1).unwrap().repositories[&urls[0]].starred,
        Some(true)
    );
    assert_eq!(
        Store::at(temp.path().into())
            .repository_stats()
            .unwrap()
            .repositories[&urls[0]]
            .stars,
        Some(42)
    );
    assert_eq!(
        store.repository_stats().unwrap().repositories[&urls[0]].archived,
        Some(true)
    );
}

#[test]
fn failed_recheck_retains_starred_state_and_other_batch_results() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::at(temp.path().into());
    let url = "https://github.com/example/previous".to_owned();
    let mut star = ViewerStar::default();
    star.record(Ok(true), now());
    let mut viewer = ViewerStarsCache::new(1);
    viewer.repositories.insert(url.clone(), star);
    store.save_viewer_stars(&viewer).unwrap();
    let mut cache = RepositoryStatsCache::default();
    let mut stats = RepositoryStats::default();
    stats.record(Ok(10), now());
    cache.repositories.insert(url.clone(), stats);
    store.save_repository_stats(&cache).unwrap();
    let (events, messages) = mpsc::channel();
    let mut worker = StatsWorker::with_lookup(store.clone(), events, |_, _, _| {
        Ok(vec![
            Err("offline".into()),
            Ok(RepositoryDetails {
                stars: 20,
                description: None,
                viewer_has_starred: false,
                archived: false,
            }),
        ])
    });
    worker.refresh(
        Account {
            id: 1,
            login: "example".into(),
        },
        vec!["https://github.com/example/second".into()],
        BTreeSet::from([url.clone()]),
    );
    loop {
        if let Message::RepoStatsDone(_, _) = messages.recv_timeout(Duration::from_secs(5)).unwrap()
        {
            break;
        }
    }
    let viewer = store.viewer_stars(1).unwrap();
    assert_eq!(viewer.repositories[&url].starred, Some(true));
    assert!(
        viewer.repositories[&url]
            .error
            .as_deref()
            .unwrap()
            .contains("offline")
    );
    assert_eq!(
        viewer.repositories["https://github.com/example/second"].starred,
        Some(false)
    );
    assert_eq!(
        store.repository_stats().unwrap().repositories[&url].stars,
        Some(10)
    );
}

#[test]
fn global_failure_stops_following_batches_and_immediate_retries() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::at(temp.path().into());
    let requests = Arc::new(Mutex::new(0));
    let calls = requests.clone();
    let (events, messages) = mpsc::channel();
    let mut worker = StatsWorker::with_lookup(store, events, move |_, _, _| {
        *calls.lock().unwrap() += 1;
        anyhow::bail!("rate limited")
    });
    for _ in 0..2 {
        worker.refresh(
            Account {
                id: 1,
                login: "example".into(),
            },
            (0..26)
                .map(|i| format!("https://github.com/example/repo-{i}"))
                .collect(),
            BTreeSet::new(),
        );
        if let Message::RepoStatsDone(_, error) =
            messages.recv_timeout(Duration::from_secs(5)).unwrap()
        {
            assert!(error.is_some());
        } else {
            panic!("unexpected repository result on global failure");
        }
    }
    assert_eq!(*requests.lock().unwrap(), 1);
}
