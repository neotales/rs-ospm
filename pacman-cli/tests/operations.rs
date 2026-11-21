//! Verify command arguments without touching the package database.
#![cfg(unix)]
use neotales_ospm_core::{Install, PackageName, Remove};
use neotales_ospm_pacman_cli::{Pacman, PacmanError};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn commands_use_separate_arguments() {
    let dir = std::env::temp_dir().join(format!(
        "ospm-pacman-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    let tool = dir.join("pacman");
    fs::write(&tool, "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$0.args\"\n").unwrap();
    fs::set_permissions(&tool, fs::Permissions::from_mode(0o700)).unwrap();
    let pacman = Pacman::with_executable(&tool);
    let package = PackageName::new("curl").unwrap();
    for attempt in 0..20 {
        match pacman.install(&package) {
            Ok(()) => break,
            Err(PacmanError::Io(error)) if error.raw_os_error() == Some(26) && attempt < 19 => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(error) => panic!("install failed: {error}"),
        }
    }
    assert_eq!(
        fs::read_to_string(dir.join("pacman.args")).unwrap(),
        "-S\n--\ncurl\n"
    );
    pacman.remove(&package).unwrap();
    assert_eq!(
        fs::read_to_string(dir.join("pacman.args")).unwrap(),
        "-R\n--\ncurl\n"
    );
    fs::remove_file(dir.join("pacman.args")).unwrap();
    fs::remove_file(tool).unwrap();
    fs::remove_dir(dir).unwrap();
}
