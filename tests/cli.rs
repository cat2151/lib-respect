use std::{
    fs,
    io::Write,
    process::{Command, Stdio},
};

use lib_respect::DependencyReport;

mod support;

#[test]
fn update_commands_have_help_without_starting_tui_or_updater() {
    for command in [None, Some("check"), Some("update")] {
        let mut process = Command::new(env!("CARGO_BIN_EXE_lib-respect"));
        if let Some(command) = command {
            process.arg(command);
        }
        let output = process.arg("--help").output().unwrap();
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let help = String::from_utf8(output.stdout).unwrap();
        match command {
            None => assert!(help.contains("check") && help.contains("update")),
            Some(command) => {
                let usage = help
                    .lines()
                    .find(|line| line.starts_with("Usage:"))
                    .unwrap();
                assert!(usage.ends_with(&format!(" {command}")));
            }
        }
    }
}

#[test]
fn update_commands_reject_extra_arguments_without_running() {
    for command in ["check", "update"] {
        let output = Command::new(env!("CARGO_BIN_EXE_lib-respect"))
            .args([command, "unexpected"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("unexpected argument"));
    }
}

#[test]
fn deps_binary_emits_only_json_on_stdout() {
    let fixture = support::workspace();
    let output = Command::new(env!("CARGO_BIN_EXE_lib-respect"))
        .args(["deps", "--offline", "--package", "app"])
        .arg(fixture.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let report: DependencyReport = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report.packages.len(), 1);
    assert_eq!(report.packages[0].name, "app");
    assert!(
        !report.packages[0]
            .dependencies
            .iter()
            .any(|d| d.name == "transitive")
    );
}

#[test]
fn malformed_stdin_fails_without_json_on_stdout() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_lib-respect"))
        .arg("stars")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"not JSON").unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid deps JSON"));
}

#[test]
fn utf8_bom_is_accepted_before_schema_validation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("deps.json");
    fs::write(
        &path,
        "\u{feff}{\"schema_version\":999,\"workspace_root\":\"X:/workspace\",\"packages\":[]}",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_lib-respect"))
        .arg("stars")
        .arg(path)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("unsupported dependency report schema_version: 999")
    );
}

#[test]
fn missing_manifest_is_an_actionable_error() {
    let directory = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_lib-respect"))
        .arg("deps")
        .arg(directory.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Cargo manifest does not exist"));
}
