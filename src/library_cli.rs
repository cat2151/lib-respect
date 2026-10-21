use anyhow::Result;
use lib_respect::browser::{
    Cancellation, CratesIo, INSPECTION_VERSION, Store, enrich_libraries, inspect_repository, now,
};

pub fn refresh(store: &Store) -> Result<()> {
    let Some(account) = store.config()?.account else {
        anyhow::bail!("no cached account; launch the TUI first");
    };
    let Some(catalog) = store.catalog(&account)? else {
        anyhow::bail!("no cached projects; launch the TUI first");
    };
    let registry = CratesIo::default();
    let cancel = Cancellation::default();
    for repo in &catalog.repositories {
        if let Some(mut data) = store.dependencies(account.id, repo)? {
            if data.report.inspection_version != INSPECTION_VERSION {
                data.report = inspect_repository(repo, &cancel)?;
                data.pushed_at = repo.pushed_at.clone();
                data.fetched_at = now();
            }
            eprintln!(
                "Enriching {} ({} declarations)",
                repo.full_name,
                data.report.dependencies.len()
            );
            enrich_libraries(
                &repo.html_url,
                &mut data.report,
                store,
                &registry,
                &cancel,
                |_| {},
            )?;
            store.save_dependencies(&data)?;
        }
    }
    Ok(())
}
