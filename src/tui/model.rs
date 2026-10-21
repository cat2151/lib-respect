use std::collections::BTreeMap;

use lib_respect::browser::{Account, Catalog, DependencyCache, OwnedRepository, UiPreferences};

use super::repo_list::{RepoList, Screen};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Focus {
    Projects,
    Dependencies,
}

#[derive(Default)]
pub struct DependencyView {
    pub data: Option<DependencyCache>,
    pub pending: bool,
    pub error: Option<String>,
    /// Completed attempt or reusable saved result, including repositories without a push date.
    pub checked_revision: Option<Option<String>>,
}

pub struct Model {
    pub screen: Screen,
    pub repositories: RepoList,
    pub account: Option<Account>,
    pub authenticated: bool,
    pub catalog: Option<Catalog>,
    pub cached_catalog: bool,
    pub catalog_loading: bool,
    pub selected: usize,
    pub preferred_repo: Option<u64>,
    pub dependency_selected: usize,
    pub focus: Focus,
    pub query: String,
    pub searching: bool,
    pub notice: String,
    pub views: BTreeMap<u64, DependencyView>,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            screen: Screen::Projects,
            repositories: RepoList::default(),
            account: None,
            authenticated: false,
            catalog: None,
            cached_catalog: true,
            catalog_loading: true,
            selected: 0,
            preferred_repo: None,
            dependency_selected: 0,
            focus: Focus::Projects,
            query: String::new(),
            searching: false,
            notice: "Checking gh authentication...".into(),
            views: BTreeMap::new(),
        }
    }
}

impl Model {
    pub fn restore_preferences(&mut self, preferences: &UiPreferences) {
        self.screen = preferences.screen;
        self.repositories.descending = preferences.stars_descending;
        self.repositories.filter = preferences.star_filter;
        self.repositories.include_own = preferences.include_own;
    }

    pub fn preferences(&self) -> UiPreferences {
        UiPreferences {
            screen: self.screen,
            stars_descending: self.repositories.descending,
            star_filter: self.repositories.filter,
            include_own: self.repositories.include_own,
        }
    }

    pub fn filtered(&self) -> Vec<&OwnedRepository> {
        let query = self.query.to_lowercase();
        self.catalog
            .as_ref()
            .map(|catalog| {
                catalog
                    .repositories
                    .iter()
                    .filter(|r| r.full_name.to_lowercase().contains(&query))
                    .collect()
            })
            .unwrap_or_default()
    }
    pub fn current(&self) -> Option<&OwnedRepository> {
        self.filtered().get(self.selected).copied()
    }
    pub fn view(&self) -> Option<&DependencyView> {
        self.views.get(&self.current()?.id)
    }
    pub fn dependency_count(&self) -> usize {
        self.view()
            .and_then(|v| v.data.as_ref())
            .map_or(0, |d| d.report.dependencies.len())
    }
    pub fn move_selection(&mut self, delta: isize) {
        let project_count = self.filtered_len();
        let (position, length) = match self.focus {
            Focus::Projects => (&mut self.selected, project_count),
            Focus::Dependencies => {
                let length = self.dependency_count();
                (&mut self.dependency_selected, length)
            }
        };
        *position = position
            .saturating_add_signed(delta)
            .min(length.saturating_sub(1));
    }
    fn filtered_len(&self) -> usize {
        self.filtered().len()
    }
    pub fn set_catalog(&mut self, catalog: Catalog, cached: bool) {
        let previous = self.current().map(|r| r.id).or(self.preferred_repo);
        if self
            .account
            .as_ref()
            .is_some_and(|a| a.id != catalog.account.id)
        {
            self.views.clear();
        }
        self.account = Some(catalog.account.clone());
        self.catalog = Some(catalog);
        self.cached_catalog = cached;
        self.selected = previous
            .and_then(|id| self.filtered().iter().position(|r| r.id == id))
            .unwrap_or(0);
    }
}
