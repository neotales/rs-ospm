//! Node runtime discovery, separate from npm package management.

use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use crate::capture::{self, CaptureError};

/// A Node.js runtime invoked as an external process.
///
/// This type reports the runtime version. It does not install packages,
/// manage Node versions, or imply that npm is available. Use [`crate::Npm`]
/// for npm operations.
#[derive(Clone, Debug)]
pub struct NodeRuntime {
    executable: PathBuf,
}

impl Default for NodeRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl NodeRuntime {
    /// Uses `node` from the current process's `PATH`.
    ///
    /// On Windows the platform resolves `node` to `node.exe`.
    pub fn new() -> Self {
        Self::with_executable("node")
    }

    /// Uses an explicit executable, useful for a configured runtime or a test
    /// fixture.
    pub fn with_executable(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
        }
    }

    /// The configured executable; a relative path is resolved when spawned.
    pub fn executable(&self) -> &Path {
        &self.executable
    }

    /// Creates a native `node` command for operations this type does not wrap.
    ///
    /// The command inherits the caller's environment and working directory
    /// until the caller changes them. This method does not execute the command.
    pub fn command(&self) -> Command {
        Command::new(&self.executable)
    }

    /// Runs `node --version` and returns the reported version.
    ///
    /// The output must start with `v` and contain only ASCII letters, digits,
    /// dots, hyphens, and plus signs, which covers release and prerelease
    /// versions. The returned string keeps the `v`, for example `v26.12.0`.
    /// The process receives no interactive standard input, and both streams are
    /// captured with the same per-stream limit as npm commands.
    pub fn version(&self) -> Result<String, NodeError> {
        let mut command = self.command();
        command.arg("--version");
        let output = capture::run(command).map_err(|error| match error {
            CaptureError::Io(error) => NodeError::Io(error),
            CaptureError::OutputTooLarge { stream, limit } => {
                NodeError::OutputTooLarge { stream, limit }
            }
        })?;
        if !output.status.success() {
            return Err(NodeError::Exit {
                status: output.status,
                stderr: output.stderr,
            });
        }
        let text = std::str::from_utf8(&output.stdout).map_err(|_| NodeError::InvalidOutput)?;
        let version = text.trim();
        let Some(rest) = version.strip_prefix('v') else {
            return Err(NodeError::InvalidOutput);
        };
        if rest.is_empty()
            || version.len() > 128
            || !rest
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
        {
            return Err(NodeError::InvalidOutput);
        }
        Ok(version.to_owned())
    }
}

/// Failure invoking the Node runtime.
#[derive(Debug)]
pub enum NodeError {
    /// The process could not be started or its output could not be read.
    Io(std::io::Error),
    /// The runtime returned an unsuccessful status.
    Exit {
        /// The process exit status.
        status: ExitStatus,
        /// Captured standard error.
        stderr: Vec<u8>,
    },
    /// The output was not a Node version line.
    InvalidOutput,
    /// The runtime produced more output than the capture limit.
    OutputTooLarge {
        /// `stdout` or `stderr`.
        stream: &'static str,
        /// The per-stream limit in bytes.
        limit: usize,
    },
}

impl fmt::Display for NodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "node process: {error}"),
            Self::Exit { status, stderr } => {
                let stderr = String::from_utf8_lossy(stderr);
                if stderr.trim().is_empty() {
                    write!(f, "node exited with {status}")
                } else {
                    write!(f, "node exited with {status}: {}", stderr.trim_end())
                }
            }
            Self::InvalidOutput => f.write_str("invalid node version output"),
            Self::OutputTooLarge { stream, limit } => {
                write!(f, "node {stream} exceeded the {limit}-byte capture limit")
            }
        }
    }
}

impl Error for NodeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Exit { .. } | Self::InvalidOutput | Self::OutputTooLarge { .. } => None,
        }
    }
}
