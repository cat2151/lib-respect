fn git_output(args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git").args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|value| !value.is_empty())
}

fn git_path(path: &str) -> Option<std::path::PathBuf> {
    git_output(&["rev-parse", "--git-path", path]).map(std::path::PathBuf::from)
}

fn main() {
    let hash = git_output(&["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=BUILD_COMMIT_HASH={hash}");

    if let Some(head_path) = git_path("HEAD") {
        println!("cargo:rerun-if-changed={}", head_path.display());

        let ref_watch_path = std::fs::read_to_string(&head_path)
            .ok()
            .and_then(|head| head.trim().strip_prefix("ref: ").map(str::to_owned))
            .filter(|reference| {
                !reference.is_empty()
                    && std::path::Path::new(reference)
                        .components()
                        .all(|component| matches!(component, std::path::Component::Normal(_)))
            })
            .and_then(|reference| git_path(&reference));
        if let Some(ref_watch_path) = ref_watch_path {
            println!("cargo:rerun-if-changed={}", ref_watch_path.display());
        }
    }

    if let Some(packed_refs_path) = git_path("packed-refs") {
        println!("cargo:rerun-if-changed={}", packed_refs_path.display());
    }
}
