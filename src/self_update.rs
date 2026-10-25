use cat_self_update_lib::self_update as launch_self_update;
use std::sync::OnceLock;

pub(crate) const REPO_OWNER: &str = "cat2151";
pub(crate) const REPO_NAME: &str = "lib-respect";
const INSTALL_CRATES: &[&str] = &[];
const MAIN_BRANCH: &str = "main";
const BUILD_COMMIT_HASH: &str = env!("BUILD_COMMIT_HASH");

pub fn run_check() -> anyhow::Result<()> {
    let result = cat_self_update_lib::check_remote_commit(
        REPO_OWNER,
        REPO_NAME,
        MAIN_BRANCH,
        BUILD_COMMIT_HASH,
    )
    .map_err(|err| anyhow::anyhow!("更新の確認に失敗しました: {err}"))?;
    println!("{result}");
    Ok(())
}

pub(crate) fn install_cmd() -> String {
    format!("cargo install --force --git {}", git_url())
}

pub(crate) fn owner_repo() -> &'static str {
    static OWNER_REPO: OnceLock<String> = OnceLock::new();
    OWNER_REPO
        .get_or_init(|| format!("{REPO_OWNER}/{REPO_NAME}"))
        .as_str()
}

fn git_url() -> &'static str {
    static GIT_URL: OnceLock<String> = OnceLock::new();
    GIT_URL
        .get_or_init(|| format!("https://github.com/{}", owner_repo()))
        .as_str()
}

fn run_self_update_with<E>(
    launch: impl FnOnce(&str, &str, &[&str]) -> Result<(), E>,
) -> anyhow::Result<()>
where
    E: std::fmt::Display,
{
    launch(REPO_OWNER, REPO_NAME, INSTALL_CRATES)
        .map_err(|err| anyhow::anyhow!("セルフアップデートの起動に失敗しました: {err}"))?;
    println!("Running: {}", install_cmd());
    println!("The application will now exit so the updater can replace the binary.");
    println!("Restart lib-respect after the update finishes.");
    Ok(())
}

pub fn run_self_update() -> anyhow::Result<()> {
    run_self_update_with(launch_self_update)
}

#[cfg(test)]
mod tests {
    use super::{REPO_NAME, REPO_OWNER, install_cmd, owner_repo, run_self_update_with};
    use std::{cell::RefCell, io};

    #[test]
    fn test_owner_repo_matches_github_repo() {
        assert_eq!(owner_repo(), "cat2151/lib-respect");
    }

    #[test]
    fn test_install_cmd_uses_repo_git_url() {
        assert_eq!(
            install_cmd(),
            "cargo install --force --git https://github.com/cat2151/lib-respect"
        );
    }

    #[test]
    fn test_run_self_update_calls_library_with_expected_arguments() {
        let actual = RefCell::new(None);

        run_self_update_with(|owner, repo, bins| -> Result<(), io::Error> {
            actual.replace(Some((
                owner.to_string(),
                repo.to_string(),
                bins.iter()
                    .map(|bin| (*bin).to_string())
                    .collect::<Vec<_>>(),
            )));
            Ok(())
        })
        .expect("self update launch should succeed");

        assert_eq!(
            actual.into_inner(),
            Some((REPO_OWNER.to_string(), REPO_NAME.to_string(), Vec::new(),))
        );
    }

    #[test]
    fn test_run_self_update_wraps_launch_errors() {
        let err = run_self_update_with(|_, _, _| Err(io::Error::other("boom")))
            .expect_err("self update launch should fail");

        assert!(
            err.to_string()
                .contains("セルフアップデートの起動に失敗しました: boom")
        );
    }
}
