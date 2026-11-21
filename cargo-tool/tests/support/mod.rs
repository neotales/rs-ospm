//! Fake Cargo executables for integration tests.
//!
//! Each fixture writes its arguments to `<executable>.args` and copies fixed
//! standard output and error from sibling files. Nothing here reads or changes
//! the caller's Cargo state.
#![cfg(unix)]
#![allow(dead_code)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use neotales_ospm_cargo_cli::CargoError;

static NEXT: AtomicU64 = AtomicU64::new(0);

pub struct FakeCargo {
    pub dir: PathBuf,
    executable: PathBuf,
}

impl FakeCargo {
    pub fn new(stdout: &str, stderr: &str, exit_code: i32) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "ospm-cargo-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        let executable = dir.join("cargo");
        let script = format!(
            "#!/bin/sh\n\
             printf '%s\\n' \"$@\" > \"$0.args\"\n\
             cat \"$0.stdout\"\n\
             cat \"$0.stderr\" >&2\n\
             exit {exit_code}\n"
        );
        fs::write(&executable, script).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(dir.join("cargo.stdout"), stdout).unwrap();
        fs::write(dir.join("cargo.stderr"), stderr).unwrap();
        Self { dir, executable }
    }

    pub fn executable(&self) -> &Path {
        &self.executable
    }

    pub fn args(&self) -> Vec<String> {
        fs::read_to_string(self.executable.with_extension("args"))
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

impl Drop for FakeCargo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// Runs an operation, retrying the transient "text file busy" race that can
/// happen when a freshly written script is executed.
pub fn with_retry<T>(
    mut operation: impl FnMut() -> Result<T, CargoError>,
) -> Result<T, CargoError> {
    for attempt in 0..20 {
        match operation() {
            Err(CargoError::Io(error)) if error.raw_os_error() == Some(26) && attempt < 19 => {
                std::thread::sleep(Duration::from_millis(10));
            }
            result => return result,
        }
    }
    unreachable!("the final attempt returns instead of retrying")
}
