use lib_respect::browser::{Account, Catalog, DependencyCache, OwnedRepository, parse_manifests};
use ratatui::{Terminal, backend::TestBackend};
use serde_json::json;

use super::*;

pub(super) fn catalog() -> Catalog {
    let repos: Vec<OwnedRepository> = ["alpha", "beta", "gamma"].iter().enumerate().map(|(index, name)| {
        serde_json::from_value(json!({"id":index as u64+1,"name":name,"full_name":format!("example/{name}"),"html_url":format!("https://github.com/example/{name}"),"description":null,"pushed_at":null,"owner":{"id":1,"login":"example"},"private":false,"archived":false,"fork":false})).unwrap()
    }).collect();
    Catalog {
        schema_version: 1,
        account: Account {
            id: 1,
            login: "example".into(),
        },
        fetched_at: 1,
        repositories: repos,
    }
}

fn cache(repo_id: u64, crate_name: &str) -> DependencyCache {
    DependencyCache {
        schema_version: 1,
        account_id: 1,
        repo_id,
        pushed_at: None,
        fetched_at: 1,
        report: parse_manifests(
            "sha".into(),
            &std::collections::BTreeMap::from([(
                "Cargo.toml".into(),
                format!("[package]\nname='app'\n[dependencies]\n{crate_name}='1'"),
            )]),
        )
        .unwrap(),
    }
}

pub(super) fn screen(model: &Model, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| render::draw(frame, model)).unwrap();
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

#[test]
fn selecting_projects_updates_right_pane_and_right_focus_scrolls_dependencies() {
    let mut model = Model::default();
    model.set_catalog(catalog(), true);
    model.views.insert(
        1,
        DependencyView {
            data: Some(cache(1, "alpha_dep")),
            ..Default::default()
        },
    );
    model.views.insert(
        2,
        DependencyView {
            data: Some(cache(2, "beta_dep")),
            ..Default::default()
        },
    );
    assert!(screen(&model, 120, 28).contains("alpha_dep"));
    model.move_selection(1);
    let rendered = screen(&model, 120, 28);
    assert!(rendered.contains("beta_dep"));
    assert!(!rendered.contains("alpha_dep"));
    model.focus = Focus::Dependencies;
    model.move_selection(1);
    assert_eq!(model.selected, 1);
    model.focus = Focus::Projects;
    model.move_selection(-1);
    assert_eq!(model.current().unwrap().name, "alpha");
    // Small terminals must render without panicking.
    screen(&model, 40, 10);
}

#[test]
fn search_and_refresh_preserve_identity_instead_of_row_number() {
    let mut model = Model::default();
    model.set_catalog(catalog(), true);
    model.selected = 1;
    let mut refreshed = catalog();
    refreshed.repositories.remove(0);
    model.set_catalog(refreshed, false);
    assert_eq!(model.current().unwrap().name, "beta");
    model.query = "gamma".into();
    model.selected = 0;
    assert_eq!(model.filtered().len(), 1);
    assert_eq!(model.current().unwrap().name, "gamma");
    model.query = "no match".into();
    assert!(model.current().is_none());
    screen(&model, 80, 24);
}

#[test]
fn hjkl_moves_focus_and_old_results_never_replace_the_selected_repo() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::at(directory.path().into());
    let mut model = Model::default();
    model.set_catalog(catalog(), true);
    model.views.insert(
        1,
        DependencyView {
            data: Some(cache(1, "alpha_dep")),
            ..Default::default()
        },
    );
    let mut app = App {
        model,
        workers: Workers::new(store.clone()),
        store,
        pending_since: None,
        pending_force: false,
        visits: Default::default(),
    };
    app.key(KeyCode::Char('j'));
    assert_eq!(app.model.current().unwrap().id, 2);
    app.receive(Message::Dependencies(
        1,
        1,
        Ok(cache(1, "late_alpha_dep")),
        None,
    ));
    assert!(!screen(&app.model, 120, 28).contains("late_alpha_dep"));
    app.key(KeyCode::Char('l'));
    assert_eq!(app.model.focus, Focus::Dependencies);
    app.key(KeyCode::Char('j'));
    assert_eq!(app.model.current().unwrap().id, 2);
    app.key(KeyCode::Char('h'));
    app.key(KeyCode::Char('k'));
    assert_eq!(app.model.current().unwrap().id, 1);
    assert!(screen(&app.model, 120, 28).contains("late_alpha_dep"));
}

#[test]
fn cached_projects_get_shared_urls_before_auth_and_schedule_enrichment_after_auth() {
    use lib_respect::browser::{Library, LibraryCache, LibraryIdentity};
    let directory = tempfile::tempdir().unwrap();
    let store = Store::at(directory.path().into());
    let mut model = Model::default();
    model.set_catalog(catalog(), true);
    let data = cache(1, "serde");
    store.save_dependencies(&data).unwrap();
    let identity = LibraryIdentity::for_dependency(
        "https://github.com/example/alpha",
        &data.report.dependencies[0],
    );
    let entry = Library {
        identity: identity.clone(),
        repository_url: Some("https://github.com/serde-rs/serde".into()),
        github_url: Some("https://github.com/serde-rs/serde".into()),
        provenance: "crates.io metadata".into(),
        reason: None,
        error: None,
        checked_at: 1,
        used_by: Default::default(),
        requirements: Default::default(),
    };
    store
        .save_libraries(&LibraryCache {
            schema_version: 1,
            libraries: std::collections::BTreeMap::from([(identity.id(), entry)]),
        })
        .unwrap();
    let mut app = App {
        model,
        workers: Workers::new(store.clone()),
        store,
        pending_since: None,
        pending_force: false,
        visits: Default::default(),
    };
    app.selection_changed(false);
    assert!(app.pending_since.is_none());
    assert!(screen(&app.model, 160, 28).contains("https://github.com/serde-rs/serde"));
    app.model.authenticated = true;
    app.selection_changed(false);
    assert!(app.pending_since.is_some());
    assert!(!app.pending_force);
}

#[test]
fn persisted_modes_restore_before_authentication_and_survive_catalog_updates() {
    use lib_respect::browser::{Screen, StarFilter, UiPreferences};
    let preferences = UiPreferences {
        screen: Screen::Repositories,
        stars_descending: false,
        star_filter: StarFilter::NotStarred,
        include_own: false,
    };
    let mut model = Model::default();
    model.restore_preferences(&preferences);
    assert_eq!(model.preferences(), preferences);
    assert!(!model.authenticated);
    assert!(screen(&model, 120, 30).contains("stars ascending"));
    assert!(screen(&model, 120, 30).contains("Own repos: excluded"));
    model.set_catalog(catalog(), false);
    assert_eq!(model.preferences(), preferences);
}

#[test]
fn background_inspects_unselected_projects_even_when_filtered_on_repository_screen() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::at(directory.path().into());
    let mut model = Model::default();
    model.set_catalog(catalog(), false);
    model.authenticated = true;
    model.query = "gamma".into();
    model.screen = Screen::Repositories;
    for id in 1..=3 {
        let mut data = cache(id, "unused");
        data.report.dependencies.clear();
        store.save_dependencies(&data).unwrap();
    }
    let mut app = App {
        model,
        workers: Workers::new(store.clone()),
        store,
        pending_since: None,
        pending_force: false,
        visits: Default::default(),
    };
    app.pending_since = Some(Instant::now());
    app.inspect_next_background();
    assert!(app.model.views.is_empty());
    app.pending_since = None;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        app.inspect_next_background();
        while let Ok(message) = app.workers.messages.try_recv() {
            app.receive(message);
        }
        if (1..=3).all(|id| {
            app.model.views.get(&id).is_some_and(|view| {
                view.checked_revision == Some(None) && !view.pending && view.data.is_some()
            })
        }) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "background scan did not complete"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(app.model.current().unwrap().id, 3);
    assert!(app.model.next_background_repository().is_none());
    app.model.catalog.as_mut().unwrap().repositories[0].pushed_at = Some("new revision".into());
    assert_eq!(app.model.next_background_repository().unwrap().id, 1);
    app.model.authenticated = false;
    assert!(app.model.next_background_repository().is_none());
}

#[test]
fn own_project_screen_shows_scan_progress_completion_and_failures() {
    let mut model = Model::default();
    assert!(screen(&model, 120, 28).contains("Dependency scan: waiting for authentication"));
    model.authenticated = true;
    model.set_catalog(catalog(), false);
    model.catalog_loading = false;
    model.query = "gamma".into();
    model.views.insert(
        1,
        DependencyView {
            checked_revision: Some(None),
            ..Default::default()
        },
    );
    model.views.insert(
        2,
        DependencyView {
            pending: true,
            ..Default::default()
        },
    );
    assert!(
        screen(&model, 120, 28).contains("Dependency scan: checking 1/3 | 0 failed | example/beta")
    );
    for id in 1..=3 {
        model.views.insert(
            id,
            DependencyView {
                checked_revision: Some(None),
                ..Default::default()
            },
        );
    }
    let rendered = screen(&model, 120, 28);
    assert!(rendered.contains("Dependency scan: complete 3/3 | 0 failed"));
    assert!(rendered.contains("✓ gamma"));
    model.views.get_mut(&2).unwrap().error = Some("lookup failed".into());
    assert!(
        screen(&model, 120, 28).contains("Dependency scan: complete with errors 3/3 | 1 failed")
    );
    model.catalog.as_mut().unwrap().repositories[0].pushed_at = Some("updated".into());
    assert!(screen(&model, 120, 28).contains("Dependency scan: checking 2/3 | 1 failed"));
    model.catalog.as_mut().unwrap().repositories[0].pushed_at = None;
    model.catalog_loading = true;
    assert!(screen(&model, 120, 28).contains("Dependency scan: waiting for project list"));
    screen(&model, 20, 5);
}

#[test]
fn completed_cache_restores_and_selection_does_not_restart_it() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::at(directory.path().into());
    for id in 1..=3 {
        let mut data = cache(id, "unresolved");
        if id != 3 {
            data.report.dependencies.clear();
        }
        store.save_dependencies(&data).unwrap();
    }
    let mut model = Model::default();
    model.set_catalog(catalog(), false);
    model.authenticated = true;
    let mut app = App {
        model,
        workers: Workers::new(store.clone()),
        store,
        pending_since: None,
        pending_force: false,
        visits: Default::default(),
    };
    app.restore_cached_inspections();
    assert_eq!(app.model.views[&1].checked_revision, Some(None));
    assert_eq!(app.model.views[&2].checked_revision, Some(None));
    assert_eq!(app.model.views[&3].checked_revision, None);
    app.selection_changed(false);
    assert!(app.pending_since.is_none());
    app.key(KeyCode::Char('j'));
    assert!(app.pending_since.is_none());
    app.key(KeyCode::Char('k'));
    assert!(app.pending_since.is_none());
    // Selecting a completed project leaves an in-flight background check alone.
    app.model.views.get_mut(&3).unwrap().pending = true;
    app.selection_changed(false);
    assert!(app.model.views[&3].pending);
    app.selection_changed(true);
    assert!(app.pending_since.is_some());
    app.model.views.get_mut(&1).unwrap().pending = false;
    app.model.catalog.as_mut().unwrap().repositories[0].pushed_at = Some("new push".into());
    app.restore_cached_inspections();
    assert_eq!(app.model.next_background_repository().unwrap().id, 1);
}
