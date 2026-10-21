use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Screen {
    #[default]
    Projects,
    Repositories,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StarFilter {
    NotStarred,
    Starred,
    #[default]
    Both,
}

impl StarFilter {
    pub fn label(self) -> &'static str {
        match self {
            Self::NotStarred => "Not starred",
            Self::Starred => "Starred",
            Self::Both => "Both",
        }
    }
    pub fn matches(self, value: Option<bool>) -> bool {
        match self {
            Self::NotStarred => value == Some(false),
            Self::Starred => value == Some(true),
            Self::Both => true,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct UiPreferences {
    pub screen: Screen,
    pub stars_descending: bool,
    pub star_filter: StarFilter,
    pub include_own: bool,
}

impl Default for UiPreferences {
    fn default() -> Self {
        Self {
            screen: Screen::Projects,
            stars_descending: true,
            star_filter: StarFilter::Both,
            include_own: true,
        }
    }
}
