//! Exercise Cargo's real path-install replacement behaviour.
//!
//! The portable `Install` trait takes only a crate name and drives the default
//! registry, so it cannot install a local path. This test calls
//! `cargo install --path` through [`Cargo::command`] and covers Cargo's
//! replacement rule, not `Install::install`. The registry-backed method is
//! covered only by fake-executable argument tests; no registry install runs
//! here.
//!
//! Cargo's home, install root, target directory, and working directory are
//! inside the fixture, and `--offline` prevents registry access. The rest of
//! the process environment is inherited: rustc and toolchain discovery,
//! `RUSTFLAGS`, Cargo environment configuration, and other inherited variables
//! still apply. The two inherited rustc wrappers are removed below so a host
//! wrapper cannot intercept the fixture build; standard rustc discovery is
//! unchanged.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use neotales_ospm_cargo_cli::Cargo;

const PACKAGE_NAME: &str = "ospm-cargo-fixture";

struct Fixture {
    dir: PathBuf,
}

impl Fixture {
    /// Creates a unique directory under the test target's temporary area. The
    /// fixture owns only this directory and never removes a shared parent.
    fn new() -> Self {
        let base = std::env::var_os("CARGO_TARGET_TMPDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("ospm-cargo-cli-tests"));
        fs::create_dir_all(&base).expect("create test base directory");
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before Unix epoch")
            .as_nanos();
        for attempt in 0..100 {
            let dir = base.join(format!(
                "real-cargo-{}-{stamp}-{attempt}",
                std::process::id()
            ));
            match fs::create_dir(&dir) {
                Ok(()) => return Self { dir },
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create fixture directory: {error}"),
            }
        }
        panic!("could not create a unique fixture directory");
    }

    fn write_crate(&self, name: &str, version: &str, marker: &str) -> PathBuf {
        let dir = self.dir.join(name);
        fs::create_dir_all(dir.join("src")).expect("create fixture crate");
        let manifest = format!(
            "[package]\n\
             name = \"{PACKAGE_NAME}\"\n\
             version = \"{version}\"\n\
             edition = \"2021\"\n\
             \n\
             [[bin]]\n\
             name = \"{PACKAGE_NAME}\"\n\
             path = \"src/main.rs\"\n\
             \n\
             [workspace]\n"
        );
        fs::write(dir.join("Cargo.toml"), manifest).expect("write fixture manifest");
        let main = format!(
            "fn main() {{\n    println!(\"fixture {marker} {{}}\", env!(\"CARGO_PKG_VERSION\"));\n}}\n"
        );
        fs::write(dir.join("src/main.rs"), main).expect("write fixture main");
        dir
    }

    /// Runs real `cargo install --path` with Cargo's home, install root,
    /// target directory, and working directory inside the fixture. `--offline`
    /// and the fixture registry home keep it off the network; the path crate
    /// has no dependencies. The inherited environment still applies except for
    /// the rustc wrappers removed here.
    fn install(&self, crate_dir: &Path) -> Output {
        let mut command = Cargo::with_executable(cargo_executable()).command();
        command
            .args(["install", "--offline", "--root"])
            .arg(self.dir.join("root"))
            .arg("--path")
            .arg(crate_dir)
            .current_dir(&self.dir)
            .env("CARGO_HOME", self.dir.join("cargo-home"))
            .env("CARGO_NET_OFFLINE", "true")
            .env("CARGO_TERM_COLOR", "never")
            .env("CARGO_TARGET_DIR", self.dir.join("target"))
            // A wrapper inherited from the host would intercept rustc. Remove
            // it without touching rustc or toolchain discovery.
            .env_remove("RUSTC_WRAPPER")
            .env_remove("RUSTC_WORKSPACE_WRAPPER");
        let output = command.output().expect("start cargo install");
        assert!(
            output.status.success(),
            "cargo install --path failed with {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            self.dir.join("cargo-home").is_dir(),
            "cargo did not use the isolated CARGO_HOME"
        );
        output
    }

    fn run_installed(&self) -> String {
        let binary = self
            .dir
            .join("root")
            .join("bin")
            .join(format!("{PACKAGE_NAME}{}", std::env::consts::EXE_SUFFIX));
        assert!(
            binary.is_file(),
            "cargo did not install {}",
            binary.display()
        );
        let output = Command::new(&binary)
            .output()
            .expect("run installed fixture binary");
        assert!(
            output.status.success(),
            "installed fixture failed with {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("fixture output is UTF-8")
            .trim()
            .to_owned()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Only the unique directory created by `new`; never a shared target or
        // system temporary directory.
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn cargo_executable() -> PathBuf {
    std::env::var_os("CARGO")
        .map(PathBuf::from)
        .or_else(|| option_env!("CARGO").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("cargo"))
}

#[test]
fn path_install_replaces_an_existing_binary_offline() {
    let fixture = Fixture::new();
    let v1 = fixture.write_crate("v1", "0.1.0", "v1");
    let v2 = fixture.write_crate("v2", "0.2.0", "v2");

    fixture.install(&v1);
    assert_eq!(fixture.run_installed(), "fixture v1 0.1.0");

    // Cargo's install documentation says a path install always builds and
    // installs. This second version must replace the first in the isolated
    // root rather than be skipped as already installed.
    fixture.install(&v2);
    assert_eq!(fixture.run_installed(), "fixture v2 0.2.0");
}
