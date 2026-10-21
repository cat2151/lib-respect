use std::{cell::Cell, collections::BTreeMap};

use anyhow::Result;
use lib_respect::browser::{
    Cancellation, LibraryIdentity, LibraryLookup, ManifestReport, Store, enrich_libraries,
    parse_manifests,
};

struct Lookup {
    calls: Cell<usize>,
    fail: bool,
    repository: Option<&'static str>,
}

impl LibraryLookup for Lookup {
    fn repository(&self, _: &str, _: &Cancellation) -> Result<Option<String>> {
        self.calls.set(self.calls.get() + 1);
        if self.fail {
            anyhow::bail!("offline test");
        }
        Ok(self.repository.map(str::to_owned))
    }
}

fn report(dependencies: &str) -> ManifestReport {
    parse_manifests(
        "revision".into(),
        &BTreeMap::from([(
            "Cargo.toml".into(),
            format!("[package]\nname='app'\n[dependencies]\n{dependencies}"),
        )]),
    )
    .unwrap()
}

#[test]
fn shared_library_survives_restart_and_deduplicates_projects_aliases_and_requirements() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::at(dir.path().into());
    let lookup = Lookup {
        calls: Cell::new(0),
        fail: false,
        repository: Some("https://github.com/Serde-rs/serde.git"),
    };
    let mut first = report("renamed={package='serde',version='1'}");
    let mut steps = 0;
    enrich_libraries(
        "https://github.com/example/first",
        &mut first,
        &store,
        &lookup,
        &Cancellation::default(),
        |_| steps += 1,
    )
    .unwrap();
    assert_eq!(steps, 1);
    let reopened = Store::at(dir.path().into());
    let mut second = report("serde='=1.0.200'");
    enrich_libraries(
        "https://github.com/example/second",
        &mut second,
        &reopened,
        &lookup,
        &Cancellation::default(),
        |_| {},
    )
    .unwrap();
    assert_eq!(lookup.calls.get(), 1);
    assert_eq!(
        first.dependencies[0].library_id,
        second.dependencies[0].library_id
    );
    assert_eq!(
        second.dependencies[0].github_url.as_deref(),
        Some("https://github.com/serde-rs/serde")
    );
    let cache = reopened.libraries().unwrap();
    assert_eq!(cache.libraries.len(), 1);
    let entry = cache.libraries.values().next().unwrap();
    assert_eq!(entry.used_by.len(), 2);
    assert_eq!(entry.requirements.len(), 2);
    assert!(entry.provenance.contains("not a resolved version"));
}

#[test]
fn fork_path_and_custom_registry_never_reuse_a_same_named_crates_io_mapping() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::at(dir.path().into());
    let lookup = Lookup {
        calls: Cell::new(0),
        fail: false,
        repository: Some("https://github.com/serde-rs/serde"),
    };
    let mut deps = report(
        "published={package='serde',version='1'}\nforked={package='serde',git='https://github.com/example/fork.git'}\nlocal={package='serde',path='../serde'}\ncustom={package='serde',version='1',registry='private'}",
    );
    enrich_libraries(
        "https://github.com/example/app",
        &mut deps,
        &store,
        &lookup,
        &Cancellation::default(),
        |_| {},
    )
    .unwrap();
    assert_eq!(lookup.calls.get(), 1);
    assert_eq!(store.libraries().unwrap().libraries.len(), 4);
    for dep in &deps.dependencies {
        match dep.alias.as_str() {
            "published" => assert_eq!(
                dep.github_url.as_deref(),
                Some("https://github.com/serde-rs/serde")
            ),
            "forked" => assert_eq!(
                dep.github_url.as_deref(),
                Some("https://github.com/example/fork")
            ),
            _ => assert!(dep.github_url.is_none()),
        }
    }
    let path = deps
        .dependencies
        .iter()
        .find(|dep| dep.alias == "local")
        .unwrap();
    assert_ne!(
        LibraryIdentity::for_dependency("first", path),
        LibraryIdentity::for_dependency("second", path)
    );
}

#[test]
fn failed_refresh_keeps_previous_url_and_has_a_separate_error() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::at(dir.path().into());
    let mut deps = report("serde='1'");
    let lookup = Lookup {
        calls: Cell::new(0),
        fail: false,
        repository: Some("https://github.com/serde-rs/serde"),
    };
    enrich_libraries(
        "app",
        &mut deps,
        &store,
        &lookup,
        &Cancellation::default(),
        |_| {},
    )
    .unwrap();
    let mut cache = store.libraries().unwrap();
    cache.libraries.values_mut().next().unwrap().checked_at = 0;
    store.save_libraries(&cache).unwrap();
    let failed = Lookup {
        fail: true,
        ..lookup
    };
    enrich_libraries(
        "app",
        &mut deps,
        &store,
        &failed,
        &Cancellation::default(),
        |_| {},
    )
    .unwrap();
    let cache = store.libraries().unwrap();
    let entry = cache.libraries.values().next().unwrap();
    assert!(entry.github_url.is_some());
    assert!(entry.error.as_ref().unwrap().contains("offline test"));
    assert!(entry.reason.is_none());
    assert!(
        deps.dependencies[0]
            .repository_note
            .as_ref()
            .unwrap()
            .contains("previous URL retained")
    );
    let calls = failed.calls.get();
    enrich_libraries(
        "app",
        &mut deps,
        &store,
        &failed,
        &Cancellation::default(),
        |_| {},
    )
    .unwrap();
    assert_eq!(failed.calls.get(), calls);
}

#[test]
fn missing_repository_non_github_and_cancellation_have_distinct_outcomes() {
    for (repository, expected) in [
        (None, "No repository declared"),
        (
            Some("https://gitlab.com/example/lib"),
            "not a recognized GitHub",
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::at(dir.path().into());
        let mut deps = report("serde='1'");
        let lookup = Lookup {
            calls: Cell::new(0),
            fail: false,
            repository,
        };
        enrich_libraries(
            "app",
            &mut deps,
            &store,
            &lookup,
            &Cancellation::default(),
            |_| {},
        )
        .unwrap();
        let cache = store.libraries().unwrap();
        let entry = cache.libraries.values().next().unwrap();
        assert!(entry.error.is_none());
        assert!(entry.reason.as_ref().unwrap().contains(expected));
        let before = std::fs::read(store.root().join("cache/libraries.json")).unwrap();
        let cancel = Cancellation::default();
        cancel.cancel();
        assert!(enrich_libraries("other", &mut deps, &store, &lookup, &cancel, |_| {}).is_err());
        assert_eq!(
            std::fs::read(store.root().join("cache/libraries.json")).unwrap(),
            before
        );
    }
}
