//! Discover direct Cargo dependencies and inspect their GitHub star status.
//!
//! [`collect_dependencies`] uses Cargo metadata. [`collect_stars`] groups those
//! results by repository and accepts an injectable [`StarLookup`] implementation.
//! Neither operation stars repositories or opens a browser.

pub mod browser;
mod cargo;
mod github;
mod model;
mod repository;
mod stars;

pub use cargo::{CollectOptions, collect_dependencies, report_from_metadata};
pub use github::GhClient;
pub use model::*;
pub use repository::GithubRepo;
pub use stars::{StarLookup, collect_stars};
