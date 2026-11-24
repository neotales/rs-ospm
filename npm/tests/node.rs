//! Verify Node runtime version reporting without a real Node installation.
#![cfg(unix)]

mod support;

use std::path::Path;

use neotales_ospm_npm_cli::{NodeError, NodeRuntime};
use support::{FakeProcess, with_node_retry};

#[test]
fn default_executable_is_node_from_path() {
    assert_eq!(NodeRuntime::new().executable(), Path::new("node"));
}

#[test]
fn version_reports_the_runtime_version() {
    let fake = FakeProcess::new("node", "v26.12.0\n", "", 0);
    let node = NodeRuntime::with_executable(fake.executable());

    let version = with_node_retry(|| node.version()).unwrap();
    assert_eq!(version, "v26.12.0");
    assert_eq!(fake.args(), vec!["--version"]);
}

#[test]
fn version_accepts_a_prerelease() {
    let fake = FakeProcess::new("node", "v22.0.0-nightly20240101\n", "", 0);
    let node = NodeRuntime::with_executable(fake.executable());

    assert_eq!(
        with_node_retry(|| node.version()).unwrap(),
        "v22.0.0-nightly20240101"
    );
}

#[test]
fn version_rejects_output_that_is_not_a_node_version() {
    for stdout in [
        "1.2.3\n",
        "hello\n",
        "v\n",
        "v1.2.3 extra\n",
        "v1.2.3\nsecond\n",
    ] {
        let fake = FakeProcess::new("node", stdout, "", 0);
        let node = NodeRuntime::with_executable(fake.executable());
        let error = with_node_retry(|| node.version()).unwrap_err();
        assert!(
            matches!(error, NodeError::InvalidOutput),
            "stdout {stdout:?} produced {error:?}"
        );
    }
}

#[test]
fn version_failure_reports_status_and_stderr() {
    let fake = FakeProcess::new("node", "", "node: not found\n", 3);
    let node = NodeRuntime::with_executable(fake.executable());

    match with_node_retry(|| node.version()).unwrap_err() {
        NodeError::Exit { status, stderr } => {
            assert_eq!(status.code(), Some(3));
            assert_eq!(stderr, b"node: not found\n");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn command_has_no_arguments() {
    let fake = FakeProcess::new("node", "", "", 0);
    let node = NodeRuntime::with_executable(fake.executable());

    let command = node.command();
    assert_eq!(command.get_program(), fake.executable().as_os_str());
    assert_eq!(command.get_args().count(), 0);
}
