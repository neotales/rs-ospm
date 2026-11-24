//! Verify command construction, JSON parsing and validation with fake `brew`
//! processes. No test installs a package or touches a real Homebrew
//! installation.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use neotales_ospm_brew_cli::{Brew, BrewError, CaskToken, FormulaName, Installed, Outdated};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "ospm-brew-cli-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&directory).unwrap();
        Self(directory)
    }

    fn write_script(&self, body: &str) -> PathBuf {
        let executable = self.0.join("brew");
        fs::write(&executable, body).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        executable
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Records arguments, the working directory and the safety environment.
const RECORD: &str = r#"#!/bin/sh
printf '%s\n' "$@" > "$0.args"
pwd > "$0.cwd"
printf '%s\n' "${HOMEBREW_NO_AUTO_UPDATE-unset}" > "$0.auto-update"
printf '%s\n' "${HOMEBREW_NO_AUTOREMOVE-unset}" > "$0.autoremove"
printf '%s\n' "${HOMEBREW_NO_INSTALL_CLEANUP-unset}" > "$0.install-cleanup"
printf '%s\n' "${HOMEBREW_NO_INSTALLED_DEPENDENTS_CHECK-unset}" > "$0.dependents"
printf '%s\n' "${HOMEBREW_NO_INSTALL_UPGRADE-unset}" > "$0.install-upgrade"
"#;

fn sidecar(executable: &Path, suffix: &str) -> PathBuf {
    let mut path = executable.as_os_str().to_os_string();
    path.push(suffix);
    PathBuf::from(path)
}

fn read_sidecar(executable: &Path, suffix: &str) -> String {
    fs::read_to_string(sidecar(executable, suffix)).unwrap()
}

fn read_args(executable: &Path) -> String {
    read_sidecar(executable, ".args")
}

fn json_script(body: &str) -> String {
    format!("{RECORD}cat <<'JSON'\n{body}\nJSON\n")
}

fn invoke<T>(mut action: impl FnMut() -> Result<T, BrewError>) -> Result<T, BrewError> {
    for attempt in 0..20 {
        match action() {
            Err(BrewError::Io(error)) if error.raw_os_error() == Some(26) && attempt < 19 => {
                std::thread::sleep(Duration::from_millis(10));
            }
            result => return result,
        }
    }
    unreachable!("retry loop always returns")
}

fn assert_safety_environment(executable: &Path) {
    assert_eq!(read_sidecar(executable, ".auto-update"), "1\n");
    assert_eq!(read_sidecar(executable, ".autoremove"), "1\n");
    assert_eq!(read_sidecar(executable, ".install-cleanup"), "1\n");
    assert_eq!(read_sidecar(executable, ".dependents"), "1\n");
}

#[test]
fn installed_parses_formulae_and_casks_and_skips_uninstalled_entries() {
    let scratch = Scratch::new("installed");
    let executable = scratch.write_script(&json_script(
        r#"{
  "formulae": [
    {
      "name": "hello",
      "full_name": "hello",
      "installed": [
        { "version": "2.12.3" },
        { "version": "2.12.2" }
      ],
      "linked_keg": "2.12.3",
      "pinned": false,
      "outdated": true
    },
    {
      "name": "tap-tool",
      "full_name": "example/tap/tap-tool",
      "installed": [ { "version": "1.0.0" } ],
      "linked_keg": null,
      "pinned": true,
      "outdated": false
    },
    {
      "name": "available-only",
      "full_name": "available-only",
      "installed": [],
      "linked_keg": null,
      "pinned": false,
      "outdated": false
    }
  ],
  "casks": [
    {
      "token": "example-app",
      "full_token": "example-app",
      "installed": "1.2.3",
      "pinned": false,
      "outdated": true
    },
    {
      "token": "available-only",
      "full_token": "available-only",
      "installed": null,
      "pinned": false,
      "outdated": false
    }
  ]
}"#,
    ));
    let brew = Brew::with_executable(&executable);

    let installed = invoke(|| brew.installed()).unwrap();

    assert_eq!(read_args(&executable), "info\n--json=v2\n--installed\n");
    assert_eq!(installed.formulae.len(), 2);
    assert_eq!(installed.formulae[0].name, "hello");
    assert_eq!(installed.formulae[0].versions, ["2.12.3", "2.12.2"]);
    assert_eq!(installed.formulae[0].linked.as_deref(), Some("2.12.3"));
    assert!(!installed.formulae[0].pinned);
    assert!(installed.formulae[0].outdated);
    assert_eq!(installed.formulae[1].name, "example/tap/tap-tool");
    assert!(installed.formulae[1].pinned);
    assert_eq!(installed.casks.len(), 1);
    assert_eq!(installed.casks[0].token, "example-app");
    assert_eq!(installed.casks[0].version, "1.2.3");
    assert!(installed.casks[0].outdated);
    assert_safety_environment(&executable);
    assert_eq!(read_sidecar(&executable, ".install-upgrade"), "unset\n");
}

#[test]
fn installed_accepts_an_empty_inventory() {
    let scratch = Scratch::new("installed-empty");
    let executable = scratch.write_script(&json_script(r#"{ "formulae": [], "casks": [] }"#));
    let brew = Brew::with_executable(&executable);

    let installed = invoke(|| brew.installed()).unwrap();

    assert_eq!(
        installed,
        Installed {
            formulae: Vec::new(),
            casks: Vec::new(),
        }
    );
}

#[test]
fn outdated_parses_formulae_and_casks() {
    let scratch = Scratch::new("outdated");
    let executable = scratch.write_script(&json_script(
        r#"{
  "formulae": [
    {
      "name": "hello",
      "installed_versions": ["2.12.2"],
      "current_version": "2.12.3",
      "pinned": false,
      "pinned_version": null
    },
    {
      "name": "example/tap/tap-tool",
      "installed_versions": ["0.9.0", "0.9.1"],
      "current_version": "1.0.0",
      "pinned": true,
      "pinned_version": "0.9.1"
    }
  ],
  "casks": [
    {
      "name": "example-app",
      "installed_versions": ["1.2.2"],
      "current_version": "1.2.3",
      "pinned": false,
      "pinned_version": null
    }
  ]
}"#,
    ));
    let brew = Brew::with_executable(&executable);

    let outdated = invoke(|| brew.outdated()).unwrap();

    assert_eq!(read_args(&executable), "outdated\n--json=v2\n");
    assert_eq!(
        outdated,
        Outdated {
            formulae: vec![
                neotales_ospm_brew_cli::OutdatedPackage {
                    name: "hello".to_owned(),
                    installed_versions: vec!["2.12.2".to_owned()],
                    current_version: "2.12.3".to_owned(),
                    pinned: false,
                    pinned_version: None,
                },
                neotales_ospm_brew_cli::OutdatedPackage {
                    name: "example/tap/tap-tool".to_owned(),
                    installed_versions: vec!["0.9.0".to_owned(), "0.9.1".to_owned()],
                    current_version: "1.0.0".to_owned(),
                    pinned: true,
                    pinned_version: Some("0.9.1".to_owned()),
                },
            ],
            casks: vec![neotales_ospm_brew_cli::OutdatedPackage {
                name: "example-app".to_owned(),
                installed_versions: vec!["1.2.2".to_owned()],
                current_version: "1.2.3".to_owned(),
                pinned: false,
                pinned_version: None,
            }],
        }
    );
}

#[test]
fn install_formula_scopes_the_name_and_disables_upgrade() {
    let scratch = Scratch::new("install-formula");
    let executable = scratch.write_script(RECORD);
    let brew = Brew::with_executable(&executable);
    let formula = FormulaName::new("example-tool").unwrap();

    invoke(|| brew.install_formula(&formula)).unwrap();

    assert_eq!(
        read_args(&executable),
        "install\n--formula\n--no-ask\nexample-tool\n"
    );
    assert_safety_environment(&executable);
    assert_eq!(read_sidecar(&executable, ".install-upgrade"), "1\n");
}

#[test]
fn install_cask_scopes_the_token_and_disables_upgrade() {
    let scratch = Scratch::new("install-cask");
    let executable = scratch.write_script(RECORD);
    let brew = Brew::with_executable(&executable);
    let cask = CaskToken::new("example-app").unwrap();

    invoke(|| brew.install_cask(&cask)).unwrap();

    assert_eq!(
        read_args(&executable),
        "install\n--cask\n--no-ask\nexample-app\n"
    );
    assert_safety_environment(&executable);
    assert_eq!(read_sidecar(&executable, ".install-upgrade"), "1\n");
}

#[test]
fn uninstall_formula_disables_autoremove_and_does_not_upgrade() {
    let scratch = Scratch::new("uninstall-formula");
    let executable = scratch.write_script(RECORD);
    let brew = Brew::with_executable(&executable);
    let formula = FormulaName::new("example-tool").unwrap();

    invoke(|| brew.uninstall_formula(&formula)).unwrap();

    assert_eq!(
        read_args(&executable),
        "uninstall\n--formula\nexample-tool\n"
    );
    assert_safety_environment(&executable);
    assert_eq!(read_sidecar(&executable, ".install-upgrade"), "unset\n");
}

#[test]
fn uninstall_cask_does_not_zap() {
    let scratch = Scratch::new("uninstall-cask");
    let executable = scratch.write_script(RECORD);
    let brew = Brew::with_executable(&executable);
    let cask = CaskToken::new("example-app").unwrap();

    invoke(|| brew.uninstall_cask(&cask)).unwrap();

    assert_eq!(read_args(&executable), "uninstall\n--cask\nexample-app\n");
    assert_safety_environment(&executable);
}

#[test]
fn upgrade_formula_scopes_the_name() {
    let scratch = Scratch::new("upgrade-formula");
    let executable = scratch.write_script(RECORD);
    let brew = Brew::with_executable(&executable);
    let formula = FormulaName::new("example/tap/tool").unwrap();

    invoke(|| brew.upgrade_formula(&formula)).unwrap();

    assert_eq!(
        read_args(&executable),
        "upgrade\n--formula\n--no-ask\nexample/tap/tool\n"
    );
    assert_safety_environment(&executable);
    assert_eq!(read_sidecar(&executable, ".install-upgrade"), "unset\n");
}

#[test]
fn upgrade_cask_scopes_the_token() {
    let scratch = Scratch::new("upgrade-cask");
    let executable = scratch.write_script(RECORD);
    let brew = Brew::with_executable(&executable);
    let cask = CaskToken::new("example/tap/app").unwrap();

    invoke(|| brew.upgrade_cask(&cask)).unwrap();

    assert_eq!(
        read_args(&executable),
        "upgrade\n--cask\n--no-ask\nexample/tap/app\n"
    );
    assert_safety_environment(&executable);
}

#[test]
fn failure_reports_status_and_captured_stderr() {
    let scratch = Scratch::new("failure");
    let executable = scratch.write_script(
        r#"#!/bin/sh
printf 'cannot install\n' >&2
exit 7
"#,
    );
    let brew = Brew::with_executable(&executable);
    let formula = FormulaName::new("example-tool").unwrap();

    match invoke(|| brew.install_formula(&formula)).unwrap_err() {
        BrewError::Exit { status, stderr } => {
            assert_eq!(status.code(), Some(7));
            assert_eq!(stderr, b"cannot install\n");
        }
        error => panic!("unexpected error: {error}"),
    }
}

#[test]
fn invalid_json_is_a_typed_error() {
    let scratch = Scratch::new("bad-json");
    let executable = scratch.write_script("#!/bin/sh\nprintf 'not json'\n");
    let brew = Brew::with_executable(&executable);

    assert!(matches!(
        invoke(|| brew.installed()).unwrap_err(),
        BrewError::InvalidOutput
    ));
}

#[test]
fn unexpected_json_shape_is_a_typed_error() {
    let scratch = Scratch::new("bad-shape");
    let executable =
        scratch.write_script("#!/bin/sh\nprintf '%s' '{\"formulae\":{},\"casks\":[]}'\n");
    let brew = Brew::with_executable(&executable);

    assert!(matches!(
        invoke(|| brew.installed()).unwrap_err(),
        BrewError::InvalidOutput
    ));
}

#[test]
fn raw_command_has_no_wrapper_environment_or_arguments() {
    let scratch = Scratch::new("raw-command");
    let executable = scratch.write_script(
        r#"#!/bin/sh
printf '%s\n' "$@" > "$0.args"
printf '%s\n' "${HOMEBREW_NO_AUTO_UPDATE-unset}" > "$0.auto-update"
"#,
    );
    let brew = Brew::with_executable(&executable);

    let mut command = brew.command();
    assert_eq!(command.get_program(), executable.as_os_str());
    command.arg("--version");
    let status = command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(read_args(&executable), "--version\n");
}
