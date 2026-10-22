use crossterm::event::KeyCode;

use super::App;

impl App {
    pub(super) fn refresh_repositories(&mut self) {
        let Some(account) = self.model.account.clone() else {
            return;
        };
        let result = (|| -> anyhow::Result<_> {
            Ok((
                self.current_library_uses()?,
                self.store.repository_stats()?,
                self.store.viewer_stars(account.id)?,
            ))
        })();
        match result {
            Ok((libraries, stats, viewer)) => {
                self.model.repositories.reload(
                    &libraries,
                    self.model.catalog.as_ref(),
                    &stats,
                    &viewer,
                );
                let urls = self
                    .model
                    .repositories
                    .rows
                    .iter()
                    .map(|row| row.url.clone())
                    .collect();
                self.model.repositories.loading = !self.model.repositories.rows.is_empty();
                self.model.repositories.notice.clear();
                let force = self.visits.scheduled();
                self.model.repositories.generation =
                    self.workers.stats.refresh(account, urls, force);
            }
            Err(error) => {
                self.model.repositories.loading = false;
                self.model.repositories.notice = format!("Cannot load repository list: {error:#}");
            }
        }
    }

    fn current_library_uses(&self) -> anyhow::Result<lib_respect::browser::LibraryCache> {
        use lib_respect::{GithubRepo, browser::LibraryIdentity};
        let mut libraries = self.store.libraries()?;
        if let Some(catalog) = &self.model.catalog {
            for repo in &catalog.repositories {
                if let Some(data) = self.store.dependencies(catalog.account.id, repo)? {
                    let project = repo.identity()?.url();
                    // A new manifest snapshot supersedes historical usage records.
                    for library in libraries.libraries.values_mut() {
                        library.used_by.retain(|url| {
                            GithubRepo::parse(url).is_none_or(|r| r.url() != project)
                        });
                    }
                    for dep in &data.report.dependencies {
                        let id = LibraryIdentity::for_dependency(&repo.html_url, dep).id();
                        if let Some(library) = libraries.libraries.get_mut(&id) {
                            library.used_by.insert(project.clone());
                        }
                    }
                }
            }
        }
        Ok(libraries)
    }

    pub(super) fn repository_key(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::Char('q') => return true,
            KeyCode::Char('j') | KeyCode::Down => self.model.repositories.move_selection(1),
            KeyCode::Char('k') | KeyCode::Up => self.model.repositories.move_selection(-1),
            KeyCode::Home => self.model.repositories.move_selection(isize::MIN),
            KeyCode::End => self.model.repositories.move_selection(isize::MAX),
            KeyCode::PageUp => self.model.repositories.move_selection(-10),
            KeyCode::PageDown => self.model.repositories.move_selection(10),
            KeyCode::Char('s') => self.model.repositories.toggle_sort(),
            KeyCode::Char('f') => self.model.repositories.cycle_filter(),
            KeyCode::Char('o') => self.model.repositories.toggle_own(),
            KeyCode::Enter => {
                self.open_selected_repository(|url| open::that_detached(url));
            }
            _ => {}
        }
        false
    }

    /// Passing the opener allows testing Enter's exact target without launching apps.
    pub(super) fn open_selected_repository(
        &mut self,
        open: impl FnOnce(&str) -> std::io::Result<()>,
    ) {
        if let Some(url) = self.model.repositories.selected_url() {
            self.model.repositories.notice = match open(&url) {
                Ok(()) => {
                    self.visits.opened(url.clone());
                    format!("Opened {url}; star status will be checked again")
                }
                Err(error) => format!("Cannot open browser: {error}"),
            };
        }
    }
}
