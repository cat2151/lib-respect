use super::{App, model::Model};
use lib_respect::browser::OwnedRepository;

impl Model {
    pub(super) fn next_background_repository(&self) -> Option<OwnedRepository> {
        if !self.authenticated || self.views.values().any(|view| view.pending) {
            return None;
        }
        let catalog = self.catalog.as_ref()?;
        let account = self.account.as_ref()?;
        if catalog.account.id != account.id {
            return None;
        }
        catalog
            .repositories
            .iter()
            .find(|repo| {
                repo.eligible(account)
                    && self
                        .views
                        .get(&repo.id)
                        .is_none_or(|view| view.checked_revision.as_ref() != Some(&repo.pushed_at))
            })
            .cloned()
    }
}

impl App {
    pub(super) fn restore_cached_inspections(&mut self) {
        use lib_respect::browser::{INSPECTION_VERSION, LibraryIdentity, apply_library_cache, now};
        let Some(catalog) = self.model.catalog.as_ref() else {
            return;
        };
        let result = (|| -> anyhow::Result<()> {
            let libraries = self.store.libraries()?;
            for repo in &catalog.repositories {
                let view = self.model.views.entry(repo.id).or_default();
                if view.pending || view.checked_revision.as_ref() == Some(&repo.pushed_at) {
                    continue;
                }
                let Some(mut data) = self.store.dependencies(catalog.account.id, repo)? else {
                    continue;
                };
                let complete = data.report.inspection_version == INSPECTION_VERSION
                    && data.pushed_at == repo.pushed_at
                    && data.report.dependencies.iter().all(|dep| {
                        let id = LibraryIdentity::for_dependency(&repo.html_url, dep).id();
                        dep.library_id.as_ref() == Some(&id)
                            && libraries
                                .libraries
                                .get(&id)
                                .is_some_and(|lib| !lib.due(now()))
                    });
                apply_library_cache(&repo.html_url, &mut data.report, &libraries);
                view.data = Some(data);
                if complete {
                    view.checked_revision = Some(repo.pushed_at.clone());
                    view.error = None;
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.model.notice = format!("Cannot restore inspection cache: {error:#}");
        }
    }

    pub(super) fn inspect_next_background(&mut self) {
        // Foreground debounce and inspections take priority over the scan.
        if self.pending_since.is_some() {
            return;
        }
        let Some(repo) = self.model.next_background_repository() else {
            return;
        };
        let account = self.model.account.as_ref().expect("authenticated account");
        self.model.views.entry(repo.id).or_default().pending = true;
        self.workers.inspect(account.id, repo, false);
    }
}
