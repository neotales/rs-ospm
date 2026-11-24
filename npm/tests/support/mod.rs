//! Fake npm and node executables for integration tests.
//!
//! Each fixture owns a securely created unique temporary directory. The fake
//! executable writes its arguments to `<executable>.args` and copies fixed
//! standard output and error from sibling files. Dropping the fixture removes
//! only that directory.
#![cfg(unix)]
#![allow(dead_code)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use neotales_ospm_npm_cli::{NodeError, NpmError};
use tempfile::TempDir;

pub struct FakeProcess {
    dir: TempDir,
    executable: PathBuf,
}

impl FakeProcess {
    pub fn new(program: &str, stdout: &str, stderr: &str, exit_code: i32) -> Self {
        let fixture = Self::with_body(
            program,
            &format!(
                "#!/bin/sh\n\
                 printf '%s\\n' \"$@\" > \"$0.args\"\n\
                 cat \"$0.stdout\"\n\
                 cat \"$0.stderr\" >&2\n\
                 exit {exit_code}\n"
            ),
        );
        fs::write(fixture.dir.path().join(format!("{program}.stdout")), stdout).unwrap();
        fs::write(fixture.dir.path().join(format!("{program}.stderr")), stderr).unwrap();
        fixture
    }

    /// Creates a fixture with a caller-supplied executable body.
    ///
    /// Empty `.stdout` and `.stderr` sibling files are created so a body can
    /// reuse the fixed `cat` pattern when it wants to.
    pub fn with_body(program: &str, body: &str) -> Self {
        let dir = tempfile::Builder::new()
            .prefix("ospm-npm-")
            .tempdir()
            .unwrap();
        let executable = dir.path().join(program);
        fs::write(&executable, body).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(dir.path().join(format!("{program}.stdout")), "").unwrap();
        fs::write(dir.path().join(format!("{program}.stderr")), "").unwrap();
        Self { dir, executable }
    }

    pub fn executable(&self) -> &Path {
        &self.executable
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    pub fn args(&self) -> Vec<String> {
        fs::read_to_string(self.executable.with_extension("args"))
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    pub fn was_run(&self) -> bool {
        self.executable.with_extension("args").exists()
    }
}

/// Runs an npm operation, retrying the transient "text file busy" race that can
/// happen when a freshly written script is executed.
pub fn with_npm_retry<T>(
    mut operation: impl FnMut() -> Result<T, NpmError>,
) -> Result<T, NpmError> {
    for attempt in 0..20 {
        match operation() {
            Err(NpmError::Io(error)) if error.raw_os_error() == Some(26) && attempt < 19 => {
                std::thread::sleep(Duration::from_millis(10));
            }
            result => return result,
        }
    }
    unreachable!("the final attempt returns instead of retrying")
}

/// Runs a Node operation, retrying the same transient executable race.
pub fn with_node_retry<T>(
    mut operation: impl FnMut() -> Result<T, NodeError>,
) -> Result<T, NodeError> {
    for attempt in 0..20 {
        match operation() {
            Err(NodeError::Io(error)) if error.raw_os_error() == Some(26) && attempt < 19 => {
                std::thread::sleep(Duration::from_millis(10));
            }
            result => return result,
        }
    }
    unreachable!("the final attempt returns instead of retrying")
}
