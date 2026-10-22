use std::collections::{BTreeMap, BTreeSet};

use lib_respect::browser::{
    Library, LibraryCache, LibraryIdentity, RepositoryStats, RepositoryStatsCache, ViewerStar,
    ViewerStarsCache, now,
};

use super::{
    repo_list::{RepoList, RepoRow},
    tests::{catalog, screen},
    *,
};

fn library(name: &str, url: &str, project: &str) -> Library {
    Library {
        identity: LibraryIdentity {
            name: name.into(),
            source: "crates.io".into(),
            scope: None,
        },
        repository_url: Some(url.into()),
        github_url: Some(url.into()),
        provenance: "metadata".into(),
        reason: None,
        error: None,
        checked_at: now(),
        used_by: BTreeSet::from([project.into()]),
        requirements: BTreeSet::from(["1".into()]),
    }
}

fn libraries() -> LibraryCache {
    let entries = [
        library(
            "serde",
            "https://github.com/Serde-rs/serde.git",
            "https://github.com/example/alpha",
        ),
        library(
            "serde_derive",
            "https://github.com/serde-rs/serde",
            "https://github.com/example/beta",
        ),
        library(
            "clap",
            "https://github.com/clap-rs/clap",
            "https://github.com/example/alpha",
        ),
        library(
            "other-account",
            "https://github.com/other/lib",
            "https://github.com/other/app",
        ),
        library(
            "invalid",
            "https://github.com.evil.example/owner/lib",
            "https://github.com/example/alpha",
        ),
    ];
    LibraryCache {
        schema_version: 1,
        libraries: entries
            .into_iter()
            .map(|entry| (entry.identity.id(), entry))
            .collect(),
    }
}

fn stats(count: u64) -> RepositoryStats {
    RepositoryStats {
        stars: Some(count),
        fetched_at: Some(now()),
        checked_at: now(),
        error: None,
        description: Some("A useful library".into()),
        details_fetched_at: Some(now()),
    }
}

#[test]
fn repository_list_deduplicates_urls_and_excludes_other_accounts() {
    let mut list = RepoList::default();
    list.reload(
        &libraries(),
        Some(&catalog()),
        &RepositoryStatsCache::default(),
        &ViewerStarsCache::new(1),
    );
    assert_eq!(list.rows.len(), 2);
    let serde = list
        .rows
        .iter()
        .find(|r| r.url == "https://github.com/serde-rs/serde")
        .unwrap();
    assert_eq!(serde.libraries.len(), 2);
    assert_eq!(serde.projects.len(), 2);
    list.reload(
        &libraries(),
        None,
        &RepositoryStatsCache::default(),
        &ViewerStarsCache::new(1),
    );
    assert!(list.rows.is_empty());
}

#[test]
fn sort_toggles_both_directions_with_unknown_last_and_keeps_selected_repo() {
    let mut list = RepoList::default();
    list.reload(
        &libraries(),
        Some(&catalog()),
        &RepositoryStatsCache::default(),
        &ViewerStarsCache::new(1),
    );
    list.rows.push(RepoRow {
        url: "https://github.com/example/unknown".into(),
        libraries: BTreeSet::new(),
        projects: BTreeSet::new(),
        stats: RepositoryStats::default(),
        viewer: ViewerStar::default(),
    });
    list.update(
        "https://github.com/clap-rs/clap",
        stats(0),
        ViewerStar::default(),
    );
    list.update(
        "https://github.com/serde-rs/serde",
        stats(30),
        ViewerStar::default(),
    );
    assert_eq!(
        list.rows.iter().map(|r| r.stats.stars).collect::<Vec<_>>(),
        [Some(30), Some(0), None]
    );
    let selected = list.selected_url();
    list.toggle_sort();
    assert_eq!(
        list.rows.iter().map(|r| r.stats.stars).collect::<Vec<_>>(),
        [Some(0), Some(30), None]
    );
    assert_eq!(list.selected_url(), selected);
    list.toggle_sort();
    assert_eq!(list.rows[0].stats.stars, Some(30));
    list.update(
        "https://github.com/clap-rs/clap",
        stats(30),
        ViewerStar::default(),
    );
    assert_eq!(list.rows[0].url, "https://github.com/clap-rs/clap");
    assert_eq!(list.rows.last().unwrap().stats.stars, None);
}

#[test]
fn tab_switches_screen_sort_and_open_target_follow_repository_selection() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::at(dir.path().into());
    store.save_libraries(&libraries()).unwrap();
    let saved = RepositoryStatsCache {
        schema_version: 1,
        repositories: BTreeMap::from([
            ("https://github.com/serde-rs/serde".into(), stats(100)),
            ("https://github.com/clap-rs/clap".into(), stats(200)),
        ]),
    };
    store.save_repository_stats(&saved).unwrap();
    store
        .save_viewer_stars(&ViewerStarsCache {
            schema_version: 1,
            account_id: 1,
            repositories: saved
                .repositories
                .keys()
                .map(|url| {
                    (
                        url.clone(),
                        ViewerStar {
                            starred: Some(true),
                            fetched_at: Some(now()),
                            checked_at: now(),
                            error: None,
                        },
                    )
                })
                .collect(),
        })
        .unwrap();
    let mut model = Model::default();
    model.set_catalog(catalog(), true);
    model.selected = 1;
    model.focus = Focus::Dependencies;
    let mut app = App {
        model,
        workers: Workers::new(store.clone()),
        store,
        pending_since: None,
        pending_force: false,
        visits: Default::default(),
    };
    app.key(KeyCode::Tab);
    assert_eq!(app.model.screen, Screen::Repositories);
    assert_eq!(app.model.repositories.rows[0].stats.stars, Some(200));
    assert!(screen(&app.model, 120, 30).contains("stars descending"));
    app.key(KeyCode::Char('s'));
    assert_eq!(app.model.repositories.rows[0].stats.stars, Some(100));
    assert!(screen(&app.model, 120, 30).contains("stars ascending"));
    app.model.repositories.selected = 0;
    app.key(KeyCode::Char('j'));
    let mut opened = String::new();
    app.open_selected_repository(|url| {
        opened = url.into();
        Ok(())
    });
    assert_eq!(opened, "https://github.com/clap-rs/clap");
    assert!(app.visits.due().is_empty());
    app.visits.returned();
    assert_eq!(app.visits.due(), BTreeSet::from([opened.clone()]));
    app.visits.scheduled();
    app.visits.completed(&opened);
    app.open_selected_repository(|_| Err(std::io::Error::other("no browser")));
    assert!(app.model.repositories.notice.contains("no browser"));
    app.visits.returned();
    assert!(app.visits.due().is_empty());
    // An old refresh cannot overwrite newer star counts or its loading status.
    let generation = app.model.repositories.generation;
    app.receive(Message::RepoStats(
        generation - 1,
        opened,
        stats(999),
        ViewerStar::default(),
    ));
    app.receive(Message::RepoStatsDone(generation - 1, None));
    assert!(app.model.repositories.loading);
    assert_eq!(
        app.model.repositories.current().unwrap().stats.stars,
        Some(200)
    );
    screen(&app.model, 30, 8);
    app.key(KeyCode::Tab);
    assert_eq!(app.model.screen, Screen::Projects);
    assert_eq!(app.model.selected, 1);
    assert_eq!(app.model.focus, Focus::Dependencies);
}

#[test]
fn empty_list_has_no_browser_target_and_renders() {
    let mut model = Model {
        screen: Screen::Repositories,
        ..Default::default()
    };
    model.repositories.toggle_sort();
    model.repositories.move_selection(1);
    assert!(model.repositories.selected_url().is_none());
    assert!(screen(&model, 120, 25).contains("No repositories match this filter"));
    screen(&model, 20, 5);
}

#[test]
fn filter_cycles_three_states_and_never_classifies_unknown_as_not_starred() {
    use super::repo_list::StarFilter;
    let mut list = RepoList::default();
    list.reload(
        &libraries(),
        Some(&catalog()),
        &RepositoryStatsCache::default(),
        &ViewerStarsCache::new(1),
    );
    list.rows.push(RepoRow {
        url: "https://github.com/example/unknown".into(),
        libraries: BTreeSet::new(),
        projects: BTreeSet::new(),
        stats: RepositoryStats::default(),
        viewer: ViewerStar::default(),
    });
    let mut starred = ViewerStar::default();
    starred.record(Ok(true), now());
    let mut not_starred = ViewerStar::default();
    not_starred.record(Ok(false), now());
    list.update(
        "https://github.com/serde-rs/serde",
        stats(30),
        starred.clone(),
    );
    list.update("https://github.com/clap-rs/clap", stats(20), not_starred);
    assert_eq!(list.visible().len(), 3);
    list.cycle_filter();
    assert_eq!(list.filter, StarFilter::NotStarred);
    assert_eq!(list.visible().len(), 1);
    assert_eq!(
        list.selected_url().as_deref(),
        Some("https://github.com/clap-rs/clap")
    );
    list.toggle_sort();
    assert_eq!(list.visible().len(), 1);
    list.cycle_filter();
    assert_eq!(list.filter, StarFilter::Starred);
    assert_eq!(list.visible().len(), 1);
    assert_eq!(
        list.current().unwrap().url,
        "https://github.com/serde-rs/serde"
    );
    list.cycle_filter();
    assert_eq!(list.filter, StarFilter::Both);
    assert_eq!(list.visible().len(), 3);
    list.cycle_filter();
    // An asynchronous recheck can remove the selected row from the current filter.
    list.update("https://github.com/clap-rs/clap", stats(20), starred);
    assert!(list.visible().is_empty());
    assert!(list.selected_url().is_none());
    list.move_selection(1);
    assert_eq!(list.selected, 0);
}

#[test]
fn description_and_consuming_project_names_render_and_wrong_account_stars_are_ignored() {
    let mut viewer = ViewerStarsCache::new(99);
    let mut star = ViewerStar::default();
    star.record(Ok(true), now());
    viewer
        .repositories
        .insert("https://github.com/serde-rs/serde".into(), star.clone());
    let mut model = Model {
        screen: Screen::Repositories,
        ..Default::default()
    };
    model.repositories.reload(
        &libraries(),
        Some(&catalog()),
        &RepositoryStatsCache::default(),
        &viewer,
    );
    assert!(
        model
            .repositories
            .rows
            .iter()
            .all(|row| row.viewer.starred.is_none())
    );
    let mut metadata = stats(100);
    metadata.description = Some("Serialize data\nwith Rust".into());
    model
        .repositories
        .update("https://github.com/serde-rs/serde", metadata, star);
    let rendered = screen(&model, 160, 40);
    assert!(rendered.contains("Serialize data with Rust"));
    assert!(rendered.contains("example/alpha, example/beta"));
    assert!(rendered.contains(
        "serde-rs/serde  Libraries: serde, serde_derive  Serialize data with Rust  100 stars"
    ));
    assert!(rendered.contains("★  Used by: example/alpha, example/beta"));
    assert!(!rendered.contains("[starred]"));
    model.repositories.cycle_filter();
    screen(&model, 30, 8);
}
