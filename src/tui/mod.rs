mod background;
mod model;
mod render;
mod repo_list;
mod repo_render;
mod repo_screen;
mod scan_status;
mod stats_worker;
mod theme;
mod visits;
mod workers;

use std::{
    io::{self, IsTerminal},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use crossterm::event::{
    self, DisableFocusChange, EnableFocusChange, Event, KeyCode, KeyEventKind, KeyModifiers,
};
use lib_respect::browser::{Config, Store, apply_library_cache};

use model::{DependencyView, Focus, Model};
use repo_list::Screen;
use workers::{Message, Workers};

pub fn run() -> Result<()> {
    ensure!(
        io::stdin().is_terminal() && io::stdout().is_terminal(),
        "the TUI requires a terminal; use deps/stars for JSON output"
    );
    let store = Store::local()?;
    let config = store.config()?;
    let mut app = App::new(store, config)?;
    let mut terminal = ratatui::init();
    let result = crossterm::execute!(io::stdout(), EnableFocusChange)
        .map_err(anyhow::Error::from)
        .and_then(|()| app.run(&mut terminal));
    let _ = crossterm::execute!(io::stdout(), DisableFocusChange);
    ratatui::restore();
    result
}

struct App {
    model: Model,
    store: Store,
    workers: Workers,
    pending_since: Option<Instant>,
    pending_force: bool,
    visits: visits::Visits,
}

impl App {
    fn new(store: Store, config: Config) -> Result<Self> {
        let mut model = Model {
            account: config.account.clone(),
            preferred_repo: config.selected_repo,
            ..Default::default()
        };
        model.restore_preferences(&config.ui);
        if let Some(account) = &config.account
            && let Some(catalog) = store.catalog(account)?
        {
            model.set_catalog(catalog, true);
        }
        let mut app = Self {
            model,
            workers: Workers::new(store.clone()),
            store,
            pending_since: None,
            pending_force: false,
            visits: Default::default(),
        };
        app.restore_cached_inspections();
        app.selection_changed(false);
        if app.model.screen == Screen::Repositories {
            app.refresh_repositories();
        }
        app.workers.refresh_catalog();
        Ok(app)
    }

    fn run(&mut self, terminal: &mut ratatui::DefaultTerminal) -> Result<()> {
        loop {
            while let Ok(message) = self.workers.messages.try_recv() {
                self.receive(message);
            }
            if self
                .pending_since
                .is_some_and(|time| time.elapsed() >= Duration::from_millis(150))
            {
                self.pending_since = None;
                if let (Some(account), Some(repo)) = (&self.model.account, self.model.current()) {
                    self.workers
                        .inspect(account.id, repo.clone(), self.pending_force);
                }
            }
            self.inspect_next_background();
            if !self.model.repositories.loading && !self.visits.due().is_empty() {
                self.refresh_repositories();
            }
            terminal.draw(|frame| render::draw(frame, &self.model))?;
            if event::poll(Duration::from_millis(30))? {
                let key = match event::read()? {
                    Event::FocusGained => {
                        self.visits.returned();
                        continue;
                    }
                    Event::Key(key) => key,
                    _ => continue,
                };
                if key.kind == KeyEventKind::Release {
                    continue;
                }
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    break;
                }
                self.visits.key_pressed();
                let before = self.model.preferences();
                let quit = self.key(key.code);
                if self.model.preferences() != before {
                    self.save_config()?;
                }
                if quit {
                    break;
                }
            }
        }
        self.save_config()
    }

    fn save_config(&self) -> Result<()> {
        self.store
            .save_config(&Config {
                schema_version: 1,
                account: self.model.account.clone(),
                selected_repo: self.model.current().map(|r| r.id),
                ui: self.model.preferences(),
            })
            .context("cannot save TUI configuration")
    }

    fn key(&mut self, key: KeyCode) -> bool {
        if key == KeyCode::Tab || key == KeyCode::BackTab {
            self.model.searching = false;
            self.model.screen = if self.model.screen == Screen::Projects {
                self.refresh_repositories();
                Screen::Repositories
            } else {
                Screen::Projects
            };
            return false;
        }
        if self.model.screen == Screen::Repositories {
            return self.repository_key(key);
        }
        let before = self.model.current().map(|repo| repo.id);
        if self.model.searching {
            match key {
                KeyCode::Esc => {
                    self.model.searching = false;
                    self.model.query.clear();
                    self.model.selected = 0;
                }
                KeyCode::Enter => self.model.searching = false,
                KeyCode::Backspace => {
                    self.model.query.pop();
                    self.model.selected = 0;
                }
                KeyCode::Char(ch) => {
                    self.model.query.push(ch);
                    self.model.selected = 0;
                }
                _ => {}
            }
        } else {
            match key {
                KeyCode::Char('q') => return true,
                KeyCode::Char('j') | KeyCode::Down => self.model.move_selection(1),
                KeyCode::Char('k') | KeyCode::Up => self.model.move_selection(-1),
                KeyCode::Home => self.model.move_selection(isize::MIN),
                KeyCode::End => self.model.move_selection(isize::MAX),
                KeyCode::PageUp => self.model.move_selection(-10),
                KeyCode::PageDown => self.model.move_selection(10),
                KeyCode::Char('h') | KeyCode::Left | KeyCode::Esc => {
                    self.model.focus = Focus::Projects
                }
                KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => {
                    self.model.focus = Focus::Dependencies
                }
                KeyCode::Char('/') => {
                    self.model.searching = true;
                    self.model.focus = Focus::Projects;
                }
                KeyCode::Char('r') => self.selection_changed(true),
                KeyCode::Char('R') => {
                    self.workers.refresh_catalog();
                    self.model.catalog_loading = true;
                    self.model.notice = "Refreshing owned public projects...".into();
                }
                _ => {}
            }
        }
        if before != self.model.current().map(|repo| repo.id) {
            self.selection_changed(false);
        }
        false
    }

    fn selection_changed(&mut self, force: bool) {
        self.model.dependency_selected = 0;
        if !force
            && self.model.current().is_some_and(|repo| {
                self.model.views.get(&repo.id).is_some_and(|view| {
                    !view.pending && view.checked_revision.as_ref() == Some(&repo.pushed_at)
                })
            })
        {
            return;
        }
        self.pending_force = force;
        self.workers.cancel_inspection();
        for view in self.model.views.values_mut() {
            view.pending = false;
        }
        self.pending_since = None;
        self.model.dependency_selected = 0;
        let Some(repo) = self.model.current().cloned() else {
            return;
        };
        let Some(account) = &self.model.account else {
            return;
        };
        if !self.model.views.contains_key(&repo.id) {
            let (data, error) = match self.store.dependencies(account.id, &repo) {
                Ok(data) => (data, None),
                Err(error) => (None, Some(format!("{error:#}"))),
            };
            self.model.views.insert(
                repo.id,
                DependencyView {
                    data,
                    error,
                    pending: false,
                    checked_revision: None,
                },
            );
        }
        let view = self
            .model
            .views
            .get_mut(&repo.id)
            .expect("view was inserted");
        match self.store.libraries() {
            Ok(cache) => {
                if let Some(data) = &mut view.data {
                    apply_library_cache(&repo.html_url, &mut data.report, &cache);
                }
            }
            Err(error) => self.model.notice = format!("Cannot load library cache: {error:#}"),
        }
        // Inspect even an unchanged project: its shared library metadata may be
        // absent/expired. The worker reuses manifests and only refreshes due libs.
        if self.model.authenticated {
            view.pending = true;
            view.error = None;
            self.pending_since = Some(Instant::now());
        } else {
            view.pending = false;
        }
    }

    fn receive(&mut self, message: Message) {
        match message {
            Message::RepoStats(generation, url, stats, viewer) => {
                if generation == self.model.repositories.generation {
                    self.visits.completed(&url);
                    self.model.repositories.update(&url, stats, viewer);
                }
            }
            Message::RepoStatsDone(generation, error) => {
                if generation == self.model.repositories.generation {
                    self.visits.finished();
                    self.model.repositories.loading = false;
                    self.model.repositories.notice = error.unwrap_or_else(|| {
                        "Repository details updated (cache valid for 1 hour)".into()
                    });
                }
            }
            Message::DependencyProgress(account, repo, data) => {
                if self.model.account.as_ref().is_some_and(|a| a.id == account) {
                    let view = self.model.views.entry(repo).or_default();
                    view.data = Some(data);
                }
            }
            Message::Account(account) => {
                if self
                    .model
                    .account
                    .as_ref()
                    .is_some_and(|old| old.id != account.id)
                {
                    self.workers.cancel_inspection();
                    self.workers.stats.cancel();
                    self.pending_since = None;
                    self.model.catalog = None;
                    self.model.views.clear();
                    self.visits = Default::default();
                    self.model.selected = 0;
                }
                self.model.account = Some(account);
                self.model.authenticated = true;
                self.model.notice = "Authenticated. Refreshing project list...".into();
                self.restore_cached_inspections();
                self.selection_changed(false);
                if self.model.screen == Screen::Repositories {
                    self.refresh_repositories();
                }
            }
            Message::Catalog(catalog, cached, warning) => {
                self.model.set_catalog(catalog, cached);
                self.model.catalog_loading = cached;
                self.model.notice = warning.unwrap_or_else(|| {
                    format!(
                        "{} projects | {}",
                        self.model.filtered().len(),
                        if cached {
                            "Showing cache while refreshing"
                        } else {
                            "Repository list updated"
                        }
                    )
                });
                self.restore_cached_inspections();
                self.selection_changed(false);
                if self.model.screen == Screen::Repositories {
                    self.refresh_repositories();
                }
            }
            Message::CatalogError(error) => {
                self.model.catalog_loading = false;
                self.model.notice =
                    format!("Project refresh failed; previous results retained. {error}");
            }
            Message::Dependencies(account, repo, result, warning) => {
                if self.model.account.as_ref().is_none_or(|a| a.id != account) {
                    return;
                }
                let view = self.model.views.entry(repo).or_default();
                view.pending = false;
                view.checked_revision = match &result {
                    Ok(data) => Some(data.pushed_at.clone()),
                    Err(_) => self
                        .model
                        .catalog
                        .as_ref()
                        .and_then(|catalog| catalog.repositories.iter().find(|r| r.id == repo))
                        .map(|r| r.pushed_at.clone()),
                };
                match result {
                    Ok(data) => {
                        view.data = Some(data);
                        view.error = None;
                    }
                    Err(error) => {
                        view.error = Some(error.clone());
                        if self.model.current().is_some_and(|r| r.id == repo) {
                            self.model.notice = error;
                        }
                    }
                }
                if let Some(warning) = warning {
                    self.model.notice = warning;
                }
                self.refresh_repositories();
            }
        }
    }
}

#[cfg(test)]
mod cmake_tests;
#[cfg(test)]
mod repo_list_tests;
#[cfg(test)]
mod tests;
