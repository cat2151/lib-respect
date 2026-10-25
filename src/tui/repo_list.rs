use std::collections::{BTreeMap, BTreeSet};

pub use lib_respect::browser::{Screen, StarFilter};
use lib_respect::{
    GithubRepo,
    browser::{
        Catalog, LibraryCache, RepositoryStats, RepositoryStatsCache, ViewerStar, ViewerStarsCache,
    },
};

#[derive(Clone, Debug)]
pub struct RepoRow {
    pub url: String,
    pub libraries: BTreeSet<String>,
    pub projects: BTreeSet<String>,
    pub stats: RepositoryStats,
    pub viewer: ViewerStar,
}

pub struct RepoList {
    pub rows: Vec<RepoRow>,
    pub selected: usize,
    pub descending: bool,
    pub loading: bool,
    pub generation: u64,
    pub notice: String,
    pub filter: StarFilter,
    pub viewer_login: Option<String>,
    pub include_own: bool,
}

impl Default for RepoList {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            selected: 0,
            descending: true,
            loading: false,
            generation: 0,
            notice: String::new(),
            filter: StarFilter::Both,
            viewer_login: None,
            include_own: true,
        }
    }
}

impl RepoList {
    pub fn reload(
        &mut self,
        libraries: &LibraryCache,
        catalog: Option<&Catalog>,
        stats: &RepositoryStatsCache,
        viewer: &ViewerStarsCache,
    ) {
        let previous = self.current().map(|row| row.url.clone());
        self.viewer_login = catalog.map(|c| c.account.login.clone());
        let eligible: BTreeSet<_> = catalog
            .into_iter()
            .flat_map(|c| c.repositories.iter())
            .filter_map(|repo| GithubRepo::parse(&repo.html_url).map(|r| r.url()))
            .collect();
        let mut rows: BTreeMap<String, RepoRow> = BTreeMap::new();
        for library in libraries.libraries.values() {
            let Some(repo) = library.github_url.as_deref().and_then(GithubRepo::parse) else {
                continue;
            };
            let projects: BTreeSet<_> = library
                .used_by
                .iter()
                .filter_map(|url| GithubRepo::parse(url).map(|r| r.url()))
                .filter(|url| eligible.contains(url))
                .collect();
            if projects.is_empty() {
                continue;
            }
            let url = repo.url();
            let row = rows.entry(url.clone()).or_insert_with(|| RepoRow {
                stats: stats.repositories.get(&url).cloned().unwrap_or_default(),
                viewer: if catalog.is_some_and(|c| c.account.id == viewer.account_id) {
                    viewer.repositories.get(&url).cloned().unwrap_or_default()
                } else {
                    ViewerStar::default()
                },
                url,
                libraries: BTreeSet::new(),
                projects: BTreeSet::new(),
            });
            row.libraries.insert(library.identity.name.clone());
            row.projects.extend(projects);
        }
        self.rows = rows.into_values().collect();
        self.sort(previous);
    }

    pub fn current(&self) -> Option<&RepoRow> {
        self.visible().get(self.selected).copied()
    }

    pub fn visible(&self) -> Vec<&RepoRow> {
        self.rows
            .iter()
            .filter(|row| self.filter.matches(row.viewer.starred, row.stats.archived))
            .filter(|row| self.include_own || !self.is_own(row))
            .collect()
    }

    fn is_own(&self, row: &RepoRow) -> bool {
        self.viewer_login.as_deref().is_some_and(|login| {
            GithubRepo::parse(&row.url).is_some_and(|repo| repo.owner().eq_ignore_ascii_case(login))
        })
    }

    pub fn toggle_own(&mut self) {
        let previous = self.current().map(|row| row.url.clone());
        self.include_own = !self.include_own;
        self.restore_selection(previous);
    }

    pub fn cycle_filter(&mut self) {
        let previous = self.current().map(|row| row.url.clone());
        self.filter = match self.filter {
            StarFilter::Both => StarFilter::NotStarred,
            StarFilter::NotStarred => StarFilter::Starred,
            StarFilter::Starred => StarFilter::Archived,
            StarFilter::Archived => StarFilter::Both,
        };
        self.restore_selection(previous);
    }

    pub fn move_selection(&mut self, delta: isize) {
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(self.visible().len().saturating_sub(1));
    }

    pub fn toggle_sort(&mut self) {
        let previous = self.current().map(|row| row.url.clone());
        self.descending = !self.descending;
        self.sort(previous);
    }

    pub fn update(&mut self, url: &str, stats: RepositoryStats, viewer: ViewerStar) {
        let previous = self.current().map(|row| row.url.clone());
        if let Some(row) = self.rows.iter_mut().find(|row| row.url == url) {
            row.stats = stats;
            row.viewer = viewer;
            self.sort(previous);
        }
    }

    fn sort(&mut self, previous: Option<String>) {
        self.rows.sort_by(|a, b| {
            match (a.stats.stars, b.stats.stars) {
                (Some(a), Some(b)) => {
                    if self.descending {
                        b.cmp(&a)
                    } else {
                        a.cmp(&b)
                    }
                }
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            }
            .then_with(|| a.url.cmp(&b.url))
        });
        self.restore_selection(previous);
    }

    fn restore_selection(&mut self, previous: Option<String>) {
        self.selected = previous
            .and_then(|url| self.visible().iter().position(|row| row.url == url))
            .unwrap_or(0);
    }

    pub fn selected_url(&self) -> Option<String> {
        // Validate again at the browser boundary; never open arbitrary cache content.
        self.current()
            .and_then(|row| GithubRepo::parse(&row.url))
            .map(|repo| repo.url())
    }
}
