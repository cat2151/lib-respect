use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DependencyReport {
    pub schema_version: u32,
    pub workspace_root: String,
    pub packages: Vec<ProjectPackage>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProjectPackage {
    pub id: String,
    pub name: String,
    pub version: String,
    pub manifest_path: String,
    pub dependencies: Vec<DirectDependency>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DirectDependency {
    pub package_id: String,
    pub name: String,
    pub version: String,
    /// The name used by the dependent crate, including any dependency rename.
    pub dependency_name: String,
    pub kinds: Vec<DependencyKind>,
    /// Cargo's source identifier; null for a path dependency.
    pub source: Option<String>,
    /// The repository field declared by the dependency package.
    pub repository: Option<String>,
    pub github: Option<GithubLink>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, Ord, PartialEq, PartialOrd)]
pub struct DependencyKind {
    pub kind: String,
    pub target: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GithubLink {
    pub url: String,
    pub identified_by: RepositoryOrigin,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryOrigin {
    GitSource,
    PackageRepository,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, Ord, PartialEq, PartialOrd)]
pub struct DependencyUse {
    pub workspace_root: String,
    pub project_package_id: String,
    pub project_package: String,
    pub dependency_package_id: String,
    pub dependency_name: String,
    pub crate_name: String,
    pub version: String,
    pub kinds: Vec<DependencyKind>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct StarReport {
    pub schema_version: u32,
    /// The authenticated GitHub account used for these checks.
    pub viewer: String,
    pub repositories: Vec<RepositoryStars>,
    pub unmapped_dependencies: Vec<DependencyUse>,
}

impl StarReport {
    pub fn has_errors(&self) -> bool {
        self.repositories.iter().any(|repo| repo.error.is_some())
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RepositoryStars {
    pub url: String,
    pub owner: String,
    pub name: String,
    /// null means the check failed; it never means "not starred".
    pub starred: Option<bool>,
    pub error: Option<String>,
    pub used_by: Vec<DependencyUse>,
}
