//! GitHub project discovery and static Cargo/CMake dependency browsing.

mod catalog;
mod cmake;
mod cmake_syntax;
mod libraries;
mod local;
mod manifest;
mod preferences;
mod process;
mod registry;
mod remote;
mod repo_stats;
mod store;
mod viewer;

pub use catalog::{Account, Catalog, OwnedRepository, discover, identify_account};
pub use cmake::{INSPECTION_VERSION, parse_project_files};
pub use libraries::{
    Library, LibraryCache, LibraryIdentity, LibraryLookup, apply_library_cache, enrich_libraries,
};
pub use local::resolve_local_repositories;
pub use manifest::{DeclaredDependency, ManifestReport, parse_manifests};
pub use preferences::{Screen, StarFilter, UiPreferences};
pub use process::Cancellation;
pub use registry::CratesIo;
pub use remote::inspect_repository;
pub use repo_stats::{RepositoryStats, RepositoryStatsCache, repository_stars};
pub use store::{Config, DependencyCache, Store, now};
pub use viewer::{RepositoryDetails, ViewerStar, ViewerStarsCache, repository_details};
