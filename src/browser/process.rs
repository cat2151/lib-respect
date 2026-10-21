use std::{
    io::Read,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};

#[derive(Clone, Debug, Default)]
pub struct Cancellation(Arc<AtomicBool>);

impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
    pub fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            bail!("request cancelled");
        }
        Ok(())
    }
}

/// REST reads, with bounded duration and cancellable children.
pub(super) fn api(
    endpoint: &str,
    raw: bool,
    paginate: bool,
    cancel: &Cancellation,
) -> Result<Vec<u8>> {
    cancel.check()?;
    let mut command = Command::new("gh");
    command
        .args([
            "api",
            "--hostname",
            "github.com",
            "--method",
            "GET",
            endpoint,
        ])
        .env("GH_PROMPT_DISABLED", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if raw {
        command.args(["--header", "Accept: application/vnd.github.raw+json"]);
    }
    if paginate {
        command.args(["--paginate", "--slurp"]);
    }
    run(&mut command, endpoint, cancel)
}

/// The caller supplies a fixed read-only GraphQL query, never a mutation.
pub(super) fn graphql(
    query: &str,
    repo: &crate::GithubRepo,
    cancel: &Cancellation,
) -> Result<Vec<u8>> {
    let mut command = Command::new("gh");
    command
        .args([
            "api",
            "graphql",
            "--hostname",
            "github.com",
            "--method",
            "POST",
            "--raw-field",
        ])
        .arg(format!("query={query}"))
        .arg("--raw-field")
        .arg(format!("owner={}", repo.owner()))
        .arg("--raw-field")
        .arg(format!("name={}", repo.name()))
        .env("GH_PROMPT_DISABLED", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    run(&mut command, "graphql", cancel)
}

pub(super) fn run(command: &mut Command, endpoint: &str, cancel: &Cancellation) -> Result<Vec<u8>> {
    cancel.check()?;
    let mut child = command
        .spawn()
        .context("cannot run gh; install GitHub CLI and run gh auth login")?;
    let mut stdout = child.stdout.take().context("gh stdout is missing")?;
    let mut stderr = child.stderr.take().context("gh stderr is missing")?;
    let out = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).map(|_| bytes)
    });
    let err = thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).map(|_| bytes)
    });
    let started = Instant::now();
    let result = loop {
        if cancel.is_cancelled() || started.elapsed() > Duration::from_secs(60) {
            let _ = child.kill();
            let _ = child.wait();
            break Err(anyhow::anyhow!(if cancel.is_cancelled() {
                "request cancelled"
            } else {
                "GitHub request timed out after 60 seconds"
            }));
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(error.into());
            }
        }
    };
    let stdout = out
        .join()
        .map_err(|_| anyhow::anyhow!("gh stdout reader failed"))??;
    let stderr = err
        .join()
        .map_err(|_| anyhow::anyhow!("gh stderr reader failed"))??;
    let status = result?;
    if !status.success() {
        bail!(
            "gh api {endpoint}: {}",
            String::from_utf8_lossy(&stderr).trim()
        );
    }
    Ok(stdout)
}
