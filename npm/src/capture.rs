//! Bounded child-process output capture.
//!
//! [`std::process::Command::output`] reads both pipes into unbounded buffers.
//! npm's JSON parser has its own limit, but that limit cannot apply until the
//! allocation has already happened. This module reads each pipe on its own
//! thread with a per-stream cap.
//!
//! The child is spawned as the leader of its own process group on Unix and in
//! a Windows job object. If a stream exceeds the limit, the whole group or job
//! is killed, not only the direct child. That closes pipes inherited by
//! descendants, so a descendant cannot hold a reader open after the direct
//! child is gone. The direct child is then reaped and the reader threads are
//! joined. A descendant that deliberately starts a new session or breaks away
//! from the job is outside this containment.

use std::io::{self, Read};
use std::process::{Command, ExitStatus, Output, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[cfg(windows)]
use process_wrap::std::JobObject;
#[cfg(unix)]
use process_wrap::std::ProcessGroup;
use process_wrap::std::{ChildWrapper, CommandWrap};

/// Per-stream capture limit.
pub(crate) const MAX_STREAM_BYTES: usize = 16 * 1024 * 1024;

/// Failure while capturing a child process.
#[derive(Debug)]
pub(crate) enum CaptureError {
    /// The process could not be started or read.
    Io(io::Error),
    /// A stream exceeded [`MAX_STREAM_BYTES`].
    OutputTooLarge {
        /// `stdout` or `stderr`.
        stream: &'static str,
        /// The per-stream limit in bytes.
        limit: usize,
    },
}

/// Runs a command, captures both streams up to [`MAX_STREAM_BYTES`] each, and
/// reaps the child.
///
/// The child receives no standard input. It runs in its own process group
/// (Unix) or job object (Windows). If either stream exceeds the limit, the
/// group or job is killed and the direct child is reaped before
/// [`CaptureError::OutputTooLarge`] is returned. Killing descendants closes
/// pipes they inherited, so no reader is left waiting on a surviving
/// descendant. A reader that stops early closes its pipe, and the other reader
/// continues until the child tree is gone.
pub(crate) fn run(mut command: Command) -> Result<Output, CaptureError> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut wrapped = CommandWrap::from(command);
    #[cfg(unix)]
    wrapped.wrap(ProcessGroup::leader());
    #[cfg(windows)]
    wrapped.wrap(JobObject);

    let mut child = wrapped.spawn().map_err(CaptureError::Io)?;
    let stdout = child
        .stdout()
        .take()
        .ok_or_else(|| CaptureError::Io(io::Error::other("stdout pipe was not created")))?;
    let stderr = child
        .stderr()
        .take()
        .ok_or_else(|| CaptureError::Io(io::Error::other("stderr pipe was not created")))?;

    let abort = Arc::new(AtomicBool::new(false));
    let stdout_reader = spawn_reader(stdout, Arc::clone(&abort));
    let stderr_reader = spawn_reader(stderr, Arc::clone(&abort));

    let status = wait_until_done(&mut *child, &abort)?;

    // Join both readers before reporting any reader failure, so neither pipe
    // is left with a live thread. A wait error returns above without joining:
    // the child may still hold the pipes, and joining could block.
    let stdout = join_reader(stdout_reader);
    let stderr = join_reader(stderr_reader);

    match (stdout, stderr) {
        (Ok(Captured::TooLarge), _) => Err(CaptureError::OutputTooLarge {
            stream: "stdout",
            limit: MAX_STREAM_BYTES,
        }),
        (_, Ok(Captured::TooLarge)) => Err(CaptureError::OutputTooLarge {
            stream: "stderr",
            limit: MAX_STREAM_BYTES,
        }),
        (Err(error), _) | (_, Err(error)) => Err(error),
        (Ok(Captured::Bytes(stdout)), Ok(Captured::Bytes(stderr))) => Ok(Output {
            status,
            stdout,
            stderr,
        }),
    }
}

fn wait_until_done(
    child: &mut dyn ChildWrapper,
    abort: &AtomicBool,
) -> Result<ExitStatus, CaptureError> {
    loop {
        if abort.load(Ordering::Relaxed) {
            // `kill` is the wrapper operation: it kills the process group or
            // job, then waits for the direct child. Treat a failed kill as
            // best effort so an already-exited child does not mask the
            // overflow; `wait` still reaps the direct child.
            let _ = child.kill();
            return child.wait().map_err(CaptureError::Io);
        }
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(error) => {
                let _ = child.kill();
                return Err(CaptureError::Io(error));
            }
        }
    }
}

fn spawn_reader(
    reader: impl Read + Send + 'static,
    abort: Arc<AtomicBool>,
) -> std::thread::JoinHandle<io::Result<Captured>> {
    std::thread::spawn(move || read_bounded(reader, &abort))
}

fn read_bounded(mut reader: impl Read, abort: &AtomicBool) -> io::Result<Captured> {
    let mut captured = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => return Ok(Captured::Bytes(captured)),
            Ok(count) => {
                if captured.len() + count > MAX_STREAM_BYTES {
                    abort.store(true, Ordering::Relaxed);
                    return Ok(Captured::TooLarge);
                }
                captured.extend_from_slice(&chunk[..count]);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => {
                abort.store(true, Ordering::Relaxed);
                return Err(error);
            }
        }
    }
}

fn join_reader(
    handle: std::thread::JoinHandle<io::Result<Captured>>,
) -> Result<Captured, CaptureError> {
    match handle.join() {
        Ok(Ok(captured)) => Ok(captured),
        Ok(Err(error)) => Err(CaptureError::Io(error)),
        Err(_) => Err(CaptureError::Io(io::Error::other(
            "output reader stopped unexpectedly",
        ))),
    }
}

enum Captured {
    Bytes(Vec<u8>),
    TooLarge,
}
