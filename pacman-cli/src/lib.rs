//! Explicit process-based pacman operations.
#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};

use neotales_ospm_core::{Install, PackageName, Remove};

/// Failure invoking pacman.
#[derive(Debug)]
pub enum PacmanError {
    /// Process startup or output error.
    Io(std::io::Error),
    /// Pacman returned an unsuccessful status.
    Exit {
        /// Process exit status.
        status: ExitStatus,
        /// Captured standard error.
        stderr: Vec<u8>,
    },
}
impl fmt::Display for PacmanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "pacman: {e}"),
            Self::Exit { status, stderr } => write!(
                f,
                "pacman exited with {status}: {}",
                String::from_utf8_lossy(stderr)
            ),
        }
    }
}
impl Error for PacmanError {}

/// Explicit pacman CLI backend. It never invokes sudo.
#[derive(Debug, Clone)]
pub struct Pacman {
    executable: PathBuf,
}
impl Default for Pacman {
    fn default() -> Self {
        Self::new()
    }
}
impl Pacman {
    /// Use pacman from PATH.
    pub fn new() -> Self {
        Self::with_executable("pacman")
    }
    /// Use a specific executable.
    pub fn with_executable(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
        }
    }
    /// Build a native command to customize arguments and output handling.
    pub fn command(&self) -> Command {
        Command::new(&self.executable)
    }
    fn run(&self, action: &str, package: &PackageName) -> Result<(), PacmanError> {
        let output = self
            .command()
            .args([action, "--", package.as_str()])
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .output()
            .map_err(PacmanError::Io)?;
        if output.status.success() {
            Ok(())
        } else {
            Err(PacmanError::Exit {
                status: output.status,
                stderr: output.stderr,
            })
        }
    }
}
impl Install for Pacman {
    type Error = PacmanError;
    fn install(&self, package: &PackageName) -> Result<(), Self::Error> {
        self.run("-S", package)
    }
}
impl Remove for Pacman {
    type Error = PacmanError;
    fn remove(&self, package: &PackageName) -> Result<(), Self::Error> {
        self.run("-R", package)
    }
}
