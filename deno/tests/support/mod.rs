//! Fake Deno executable for integration tests.
//!
//! Each fixture owns a securely created unique temporary directory. The fake
//! writes its arguments and working directory to sibling files, copies fixed
//! standard output and error from sibling files, and for an install command
//! creates the launcher that the provider verifies afterwards. Dropping the
//! fixture removes only that directory.
#![cfg(unix)]
#![allow(dead_code)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use neotales_ospm_deno_cli::DenoError;
use tempfile::TempDir;

pub struct FakeDeno {
    dir: TempDir,
    executable: PathBuf,
}

impl FakeDeno {
    pub fn new(exit_code: i32) -> Self {
        let body = format!(
            "#!/bin/sh\n\
             printf '%s\\n' \"$@\" > \"$0.args\"\n\
             pwd -P > \"$0.cwd\"\n\
             root=''\n\
             name=''\n\
             prev=''\n\
             for arg in \"$@\"; do\n\
               if [ \"$prev\" = '--root' ]; then root=\"$arg\"; fi\n\
               if [ \"$prev\" = '--name' ]; then name=\"$arg\"; fi\n\
               prev=\"$arg\"\n\
             done\n\
             if [ -n \"$root\" ] && [ -n \"$name\" ]; then\n\
               mkdir -p \"$root/bin\"\n\
               : > \"$root/bin/$name\"\n\
             fi\n\
             cat \"$0.stdout\"\n\
             cat \"$0.stderr\" >&2\n\
             exit {exit_code}\n"
        );
        Self::with_body(&body)
    }

    /// Creates a fixture with a caller-supplied executable body.
    pub fn with_body(body: &str) -> Self {
        let dir = tempfile::Builder::new()
            .prefix("ospm-deno-")
            .tempdir()
            .unwrap();
        let executable = dir.path().join("deno");
        fs::write(&executable, body).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(dir.path().join("deno.stdout"), "").unwrap();
        fs::write(dir.path().join("deno.stderr"), "").unwrap();
        Self { dir, executable }
    }

    pub fn executable(&self) -> &Path {
        &self.executable
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    pub fn set_stdout(&self, stdout: &str) {
        fs::write(self.dir.path().join("deno.stdout"), stdout).unwrap();
    }

    pub fn set_stderr(&self, stderr: &str) {
        fs::write(self.dir.path().join("deno.stderr"), stderr).unwrap();
    }

    pub fn args(&self) -> Vec<String> {
        fs::read_to_string(self.executable.with_extension("args"))
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    pub fn cwd(&self) -> PathBuf {
        PathBuf::from(
            fs::read_to_string(self.executable.with_extension("cwd"))
                .unwrap()
                .trim_end(),
        )
    }

    pub fn was_run(&self) -> bool {
        self.executable.with_extension("args").exists()
    }
}

/// Runs a Deno operation, retrying the transient "text file busy" race that can
/// happen when a freshly written script is executed.
pub fn with_deno_retry<T>(
    mut operation: impl FnMut() -> Result<T, DenoError>,
) -> Result<T, DenoError> {
    for attempt in 0..20 {
        match operation() {
            Err(DenoError::Io(error)) if error.raw_os_error() == Some(26) && attempt < 19 => {
                std::thread::sleep(Duration::from_millis(10));
            }
            result => return result,
        }
    }
    unreachable!("the final attempt returns instead of retrying")
}
