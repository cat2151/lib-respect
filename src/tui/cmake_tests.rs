use super::*;
use lib_respect::browser::{
    Cancellation, DependencyCache, LibraryLookup, enrich_libraries, parse_project_files,
};

#[test]
fn cmake_results_render_after_restart_and_legacy_scans_are_scheduled_again() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::at(temp.path().into());
    let catalog = tests::catalog();
    let mut report = parse_project_files(
        "sha".into(),
        &std::collections::BTreeMap::from([(
            "CMakeLists.txt".into(),
            "FetchContent_Declare(uapmd GIT_REPOSITORY https://github.com/atsushieno/uapmd)".into(),
        )]),
    )
    .unwrap();
    struct NoRegistry;
    impl LibraryLookup for NoRegistry {
        fn repository(&self, _: &str, _: &Cancellation) -> anyhow::Result<Option<String>> {
            panic!("CMake lookup called crates.io")
        }
    }
    enrich_libraries(
        &catalog.repositories[0].html_url,
        &mut report,
        &store,
        &NoRegistry,
        &Cancellation::default(),
        |_| {},
    )
    .unwrap();
    store
        .save_dependencies(&DependencyCache {
            schema_version: 1,
            account_id: 1,
            repo_id: 1,
            pushed_at: None,
            fetched_at: 1,
            report,
        })
        .unwrap();
    for _ in 0..2 {
        let store = Store::at(temp.path().into());
        let mut model = Model::default();
        model.set_catalog(catalog.clone(), true);
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
        let rendered = tests::screen(&app.model, 180, 30);
        assert!(rendered.contains("atsushieno/uapmd"));
        assert!(rendered.contains("fetchcontent_declare"));
        assert!(!rendered.contains("No Cargo.toml"));
    }
    let repo = &catalog.repositories[0];
    let mut data = store.dependencies(1, repo).unwrap().unwrap();
    data.report.inspection_version = 0;
    store.save_dependencies(&data).unwrap();
    let mut model = Model::default();
    model.set_catalog(catalog, true);
    let mut app = App {
        model,
        workers: Workers::new(store.clone()),
        store,
        pending_since: None,
        pending_force: false,
        visits: Default::default(),
    };
    app.restore_cached_inspections();
    assert_eq!(app.model.views[&1].checked_revision, None);
    assert!(app.model.views[&1].data.is_some());
}
