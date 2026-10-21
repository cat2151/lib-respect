use std::{fs, path::Path};

use tempfile::TempDir;

pub fn workspace() -> TempDir {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    write(
        root,
        "Cargo.toml",
        r#"
[workspace]
members = ["app", "other"]
exclude = ["shared", "transitive", "optional", "no-repo", "build-helper", "unix-only"]
resolver = "2"
[workspace.dependencies]
renamed = { package = "shared", path = "shared" }
"#,
    );
    package(
        root,
        "app",
        r#"
[dependencies]
renamed.workspace = true
no-repo = { path = "../no-repo" }
optional = { path = "../optional", optional = true }
[dev-dependencies]
renamed.workspace = true
[build-dependencies]
build-helper = { path = "../build-helper" }
[target.'cfg(unix)'.dependencies]
unix-only = { path = "../unix-only" }
"#,
    );
    package(root, "other", "[dependencies]\nrenamed.workspace = true\n");
    package(
        root,
        "shared",
        "repository = \"https://github.com/Example/Shared.git\"\n[dependencies]\ntransitive = { path = \"../transitive\" }\n",
    );
    for name in [
        "transitive",
        "optional",
        "no-repo",
        "build-helper",
        "unix-only",
    ] {
        package(root, name, "");
    }
    directory
}

fn package(root: &Path, name: &str, extra: &str) {
    write(
        root,
        &format!("{name}/Cargo.toml"),
        &format!("[package]\nname = {name:?}\nversion = \"0.1.0\"\nedition = \"2024\"\n{extra}"),
    );
    write(root, &format!("{name}/src/lib.rs"), "");
}

fn write(root: &Path, file: &str, contents: &str) {
    let path = root.join(file);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}
