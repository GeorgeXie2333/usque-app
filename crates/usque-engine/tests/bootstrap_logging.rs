//! Invalid configuration exits before IPC, networking or platform initialization.

use std::{path::Path, process::Command};

fn invalid_config_command(config: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_usque-engine"));
    command.arg("--config").arg(config);
    // Cargo's MSVC test binaries find development DLLs beside the test harness.
    // Give this harmless child the same search directory, without changing the
    // parent process or any system environment setting.
    let mut paths =
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect::<Vec<_>>();
    paths.push(
        std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf(),
    );
    command.env("PATH", std::env::join_paths(paths).unwrap());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    command
}

#[test]
fn invalid_bootstrap_configuration_has_a_bounded_local_cause() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.json");
    std::fs::write(&config, b"{invalid-json").unwrap();
    let output = invalid_config_command(&config).output().unwrap();
    assert!(!output.status.success());
    let log = std::fs::read_to_string(directory.path().join("logs/engine.jsonl")).unwrap();
    assert!(log.contains("ENGINE_CONFIGURATION_LOAD_FAILED"));
    assert!(log.contains("ENGINE_RUNTIME_WAIT_STARTED"));
    assert!(log.contains("ENGINE_RUNTIME_WAIT_FINISHED"));
    assert!(!log.contains("invalid-json"));
}

#[test]
fn validate_only_does_not_create_a_log_store() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.json");
    std::fs::write(&config, b"{invalid-json").unwrap();
    let output = invalid_config_command(&config)
        .arg("--validate-only")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!directory.path().join("logs").exists());
}
