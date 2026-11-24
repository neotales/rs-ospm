//! Verify command construction, JSON parsing and validation with fake Bun
//! processes. No test installs a package or touches a real global directory.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use neotales_ospm_bun_cli::{Bun, BunError, DependencySection, PackageName, PackageSpec};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "ospm-bun-cli-{name}-{}-{}",
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
        let executable = self.0.join("bun");
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

const RECORD: &str = "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$0.args\"\npwd > \"$0.cwd\"\n";

fn json_script(json: &str) -> String {
    format!(
        "#!/bin/sh\n\
         printf '%s\\n' \"$@\" > \"$0.args\"\n\
         pwd > \"$0.cwd\"\n\
         cat <<'JSON'\n\
         {json}\n\
         JSON\n"
    )
}

fn missing_global_script(bin: &Path) -> String {
    format!(
        "#!/bin/sh\n\
         if [ \"$1\" = \"pm\" ] && [ \"$2\" = \"pkg\" ] && [ \"$3\" = \"get\" ] && [ \"$4\" = \"--global\" ]; then\n\
           printf 'missing\\n' >&2\n\
           exit 1\n\
         fi\n\
         if [ \"$1\" = \"pm\" ] && [ \"$2\" = \"bin\" ] && [ \"$3\" = \"-g\" ]; then\n\
           printf '%s\\n' {bin}\n\
           exit 0\n\
         fi\n\
         exit 2\n",
        bin = shell_quote(&bin.display().to_string())
    )
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn read_args(executable: &Path) -> String {
    fs::read_to_string(executable.with_extension("args")).unwrap()
}

fn read_cwd(executable: &Path) -> PathBuf {
    let cwd = fs::read_to_string(executable.with_extension("cwd")).unwrap();
    fs::canonicalize(cwd.trim()).unwrap()
}

fn invoke<T>(mut action: impl FnMut() -> Result<T, BunError>) -> Result<T, BunError> {
    for attempt in 0..20 {
        match action() {
            Err(BunError::Io(error)) if error.raw_os_error() == Some(26) && attempt < 19 => {
                std::thread::sleep(Duration::from_millis(10));
            }
            result => return result,
        }
    }
    unreachable!("retry loop always returns")
}

#[test]
fn add_global_passes_global_flag_and_exact_argument() {
    let scratch = Scratch::new("add-global");
    let executable = scratch.write_script(RECORD);
    let bun = Bun::with_executable(&executable);
    let spec = PackageSpec::exact("example-tool", "1.2.3").unwrap();

    invoke(|| bun.add_global(&spec)).unwrap();

    assert_eq!(
        read_args(&executable),
        "add\n--global\n--exact\nexample-tool@1.2.3\n"
    );
}

#[test]
fn add_global_without_version_omits_exact() {
    let scratch = Scratch::new("add-global-latest");
    let executable = scratch.write_script(RECORD);
    let bun = Bun::with_executable(&executable);
    let spec = PackageSpec::new("example-tool").unwrap();

    invoke(|| bun.add_global(&spec)).unwrap();

    assert_eq!(read_args(&executable), "add\n--global\nexample-tool\n");
}

#[test]
fn remove_global_passes_global_flag_and_name() {
    let scratch = Scratch::new("remove-global");
    let executable = scratch.write_script(RECORD);
    let bun = Bun::with_executable(&executable);
    let package = PackageName::new("@example/tool").unwrap();

    invoke(|| bun.remove_global(&package)).unwrap();

    assert_eq!(read_args(&executable), "remove\n--global\n@example/tool\n");
}

#[test]
fn add_project_uses_directory_and_selected_section() {
    let scratch = Scratch::new("add-project");
    let executable = scratch.write_script(RECORD);
    let bun = Bun::with_executable(&executable).with_project_directory(&scratch.0);
    let spec = PackageSpec::exact("@example/tool", "2.0.0-rc.1").unwrap();

    invoke(|| bun.add_project(&spec, DependencySection::Development)).unwrap();

    assert_eq!(
        read_args(&executable),
        "add\n--dev\n--exact\n@example/tool@2.0.0-rc.1\n"
    );
    assert_eq!(read_cwd(&executable), fs::canonicalize(&scratch.0).unwrap());
}

#[test]
fn add_project_regular_section_passes_save_prod() {
    let scratch = Scratch::new("add-regular");
    let executable = scratch.write_script(RECORD);
    let bun = Bun::with_executable(&executable).with_project_directory(&scratch.0);
    let spec = PackageSpec::new("example-tool").unwrap();

    invoke(|| bun.add_project(&spec, DependencySection::Regular)).unwrap();

    assert_eq!(read_args(&executable), "add\n--save-prod\nexample-tool\n");
}

#[test]
fn add_project_maps_optional_and_peer_sections() {
    let scratch = Scratch::new("add-sections");
    let executable = scratch.write_script(RECORD);
    let bun = Bun::with_executable(&executable).with_project_directory(&scratch.0);
    let spec = PackageSpec::new("example-tool").unwrap();

    invoke(|| bun.add_project(&spec, DependencySection::Optional)).unwrap();
    assert_eq!(read_args(&executable), "add\n--optional\nexample-tool\n");

    invoke(|| bun.add_project(&spec, DependencySection::Peer)).unwrap();
    assert_eq!(read_args(&executable), "add\n--peer\nexample-tool\n");
}

#[test]
fn remove_project_uses_directory() {
    let scratch = Scratch::new("remove-project");
    let executable = scratch.write_script(RECORD);
    let bun = Bun::with_executable(&executable).with_project_directory(&scratch.0);
    let package = PackageName::new("example-tool").unwrap();

    invoke(|| bun.remove_project(&package)).unwrap();

    assert_eq!(read_args(&executable), "remove\nexample-tool\n");
    assert_eq!(read_cwd(&executable), fs::canonicalize(&scratch.0).unwrap());
}

#[test]
fn install_project_uses_directory() {
    let scratch = Scratch::new("install-project");
    let executable = scratch.write_script(RECORD);
    let bun = Bun::with_executable(&executable).with_project_directory(&scratch.0);

    invoke(|| bun.install_project()).unwrap();

    assert_eq!(read_args(&executable), "install\n");
    assert_eq!(read_cwd(&executable), fs::canonicalize(&scratch.0).unwrap());
}

#[test]
fn global_operations_ignore_project_directory() {
    let scratch = Scratch::new("global-cwd");
    let executable = scratch.write_script(RECORD);
    let bun = Bun::with_executable(&executable).with_project_directory(&scratch.0);
    let spec = PackageSpec::new("example-tool").unwrap();

    invoke(|| bun.add_global(&spec)).unwrap();

    let current = std::env::current_dir().unwrap();
    assert_eq!(read_cwd(&executable), fs::canonicalize(current).unwrap());
}

#[test]
fn list_project_parses_direct_sections() {
    let scratch = Scratch::new("list-project");
    fs::write(scratch.0.join("package.json"), "{}").unwrap();
    let executable = scratch.write_script(&json_script(
        "{\n\
           \"dependencies\": { \"regular\": \"1.0.0\" },\n\
           \"devDependencies\": { \"dev\": \"^2.0.0\" },\n\
           \"optionalDependencies\": { \"optional\": \"file:../optional\" },\n\
           \"peerDependencies\": { \"peer\": \"workspace:*\" }\n\
         }",
    ));
    let bun = Bun::with_executable(&executable).with_project_directory(&scratch.0);

    let declared = invoke(|| bun.list_project()).unwrap();

    assert_eq!(
        read_args(&executable),
        format!("pm\npkg\nget\n--cwd\n{}\n", scratch.0.display())
    );
    assert_eq!(declared.dependencies.len(), 1);
    assert_eq!(declared.dependencies[0].name, "regular");
    assert_eq!(declared.dependencies[0].spec, "1.0.0");
    assert_eq!(declared.dev_dependencies[0].name, "dev");
    assert_eq!(declared.dev_dependencies[0].spec, "^2.0.0");
    assert_eq!(declared.optional_dependencies[0].name, "optional");
    assert_eq!(declared.optional_dependencies[0].spec, "file:../optional");
    assert_eq!(declared.peer_dependencies[0].name, "peer");
    assert_eq!(declared.peer_dependencies[0].spec, "workspace:*");
}

#[test]
fn list_project_accepts_an_empty_object() {
    let scratch = Scratch::new("list-project-empty");
    fs::write(scratch.0.join("package.json"), "{}").unwrap();
    let executable = scratch.write_script(&json_script("{}"));
    let bun = Bun::with_executable(&executable).with_project_directory(&scratch.0);

    let declared = invoke(|| bun.list_project()).unwrap();

    assert!(declared.is_empty());
}

#[test]
fn list_project_accepts_a_null_section_as_empty() {
    let scratch = Scratch::new("list-project-null");
    fs::write(scratch.0.join("package.json"), "{}").unwrap();
    let executable = scratch.write_script(&json_script("{\"dependencies\": null}"));
    let bun = Bun::with_executable(&executable).with_project_directory(&scratch.0);

    assert!(invoke(|| bun.list_project()).unwrap().is_empty());
}

#[test]
fn list_project_missing_manifest_is_a_typed_error() {
    let scratch = Scratch::new("list-project-missing");
    let executable = scratch.write_script(RECORD);
    let bun = Bun::with_executable(&executable).with_project_directory(&scratch.0);

    assert!(matches!(
        invoke(|| bun.list_project()).unwrap_err(),
        BunError::ManifestMissing(_)
    ));
    assert!(!executable.with_extension("args").exists());
}

#[test]
fn list_project_directory_invalid_is_a_typed_error() {
    let scratch = Scratch::new("list-project-file");
    let file = scratch.0.join("not-a-directory");
    fs::write(&file, "").unwrap();
    let executable = scratch.write_script(RECORD);
    let bun = Bun::with_executable(&executable).with_project_directory(&file);

    assert!(matches!(
        invoke(|| bun.list_project()).unwrap_err(),
        BunError::DirectoryInvalid(_)
    ));
    assert!(!executable.with_extension("args").exists());
}

#[test]
fn list_global_uses_the_global_flag() {
    let scratch = Scratch::new("list-global");
    let executable = scratch.write_script(&json_script(
        "{\n\
           \"dependencies\": { \"global-tool\": \"1.2.3\" }\n\
         }",
    ));
    let bun = Bun::with_executable(&executable);

    let declared = invoke(|| bun.list_global()).unwrap();

    assert_eq!(read_args(&executable), "pm\npkg\nget\n--global\n");
    assert_eq!(declared.dependencies.len(), 1);
    assert_eq!(declared.dependencies[0].name, "global-tool");
    assert_eq!(declared.dependencies[0].spec, "1.2.3");
}

#[test]
fn list_global_preserves_failure_when_manifest_is_absent() {
    let scratch = Scratch::new("list-global-absent");
    let root = scratch.0.join("bunroot");
    let executable = scratch.write_script(&missing_global_script(&root.join("bin")));
    let bun = Bun::with_executable(&executable);

    match invoke(|| bun.list_global()).unwrap_err() {
        BunError::Exit { status, stderr } => {
            assert_eq!(status.code(), Some(1));
            assert_eq!(stderr, b"missing\n");
        }
        error => panic!("unexpected error: {error}"),
    }
}

#[test]
fn list_global_reports_a_failure_when_the_manifest_exists() {
    let scratch = Scratch::new("list-global-failed");
    let root = scratch.0.join("bunroot");
    let global = root.join("install").join("global");
    fs::create_dir_all(&global).unwrap();
    fs::write(global.join("package.json"), "{}").unwrap();
    let executable = scratch.write_script(&missing_global_script(&root.join("bin")));
    let bun = Bun::with_executable(&executable);

    match invoke(|| bun.list_global()).unwrap_err() {
        BunError::Exit { status, stderr } => {
            assert_eq!(status.code(), Some(1));
            assert_eq!(stderr, b"missing\n");
        }
        error => panic!("unexpected error: {error}"),
    }
}

#[test]
fn list_global_missing_directory_is_not_a_success() {
    let scratch = Scratch::new("list-global-missing-directory");
    let global = scratch.0.join("missing");
    let bun = Bun::with_executable("/nonexistent/bun").with_global_directory(&global);
    assert!(matches!(
        invoke(|| bun.list_global()),
        Err(BunError::DirectoryInvalid(path)) if path == global
    ));
}

#[test]
fn list_global_missing_manifest_is_not_a_success() {
    let scratch = Scratch::new("list-global-missing-manifest");
    let bun = Bun::with_executable("/nonexistent/bun").with_global_directory(&scratch.0);
    assert!(matches!(
        invoke(|| bun.list_global()),
        Err(BunError::ManifestMissing(path)) if path == scratch.0.join("package.json")
    ));
}

#[test]
fn list_global_honors_an_explicit_directory() {
    let scratch = Scratch::new("list-global-explicit");
    let global = scratch.0.join("global");
    fs::create_dir_all(&global).unwrap();
    fs::write(global.join("package.json"), "{}").unwrap();
    let executable = scratch.write_script(&json_script(
        "{\n\
           \"dependencies\": { \"@example/tool\": \"2.0.0\" }\n\
         }",
    ));
    let bun = Bun::with_executable(&executable).with_global_directory(&global);

    let declared = invoke(|| bun.list_global()).unwrap();

    assert_eq!(
        read_args(&executable),
        format!("pm\npkg\nget\n--cwd\n{}\n", global.display())
    );
    assert_eq!(declared.dependencies[0].name, "@example/tool");
    assert_eq!(declared.dependencies[0].spec, "2.0.0");
}

#[test]
fn failure_reports_status_and_captured_stderr() {
    let scratch = Scratch::new("failure");
    let executable = scratch.write_script("#!/bin/sh\nprintf 'cannot add\\n' >&2\nexit 7\n");
    let bun = Bun::with_executable(&executable);
    let spec = PackageSpec::new("example-tool").unwrap();

    match invoke(|| bun.add_global(&spec)).unwrap_err() {
        BunError::Exit { status, stderr } => {
            assert_eq!(status.code(), Some(7));
            assert_eq!(stderr, b"cannot add\n");
        }
        error => panic!("unexpected error: {error}"),
    }
}

#[test]
fn invalid_json_is_a_typed_error() {
    let scratch = Scratch::new("bad-json");
    fs::write(scratch.0.join("package.json"), "{}").unwrap();
    let executable = scratch.write_script("#!/bin/sh\nprintf 'not json'\n");
    let bun = Bun::with_executable(&executable).with_project_directory(&scratch.0);

    assert!(matches!(
        invoke(|| bun.list_project()).unwrap_err(),
        BunError::InvalidOutput
    ));
}

#[test]
fn unexpected_json_shape_is_a_typed_error() {
    let scratch = Scratch::new("bad-shape");
    fs::write(scratch.0.join("package.json"), "{}").unwrap();
    let executable = scratch.write_script(&json_script(
        "{\"dependencies\":{\"tool\":{\"version\":7}}}",
    ));
    let bun = Bun::with_executable(&executable).with_project_directory(&scratch.0);

    assert!(matches!(
        invoke(|| bun.list_project()).unwrap_err(),
        BunError::InvalidOutput
    ));
}

#[test]
fn top_level_json_must_be_an_object() {
    let scratch = Scratch::new("bad-top-level");
    fs::write(scratch.0.join("package.json"), "{}").unwrap();
    let executable = scratch.write_script(&json_script("[]"));
    let bun = Bun::with_executable(&executable).with_project_directory(&scratch.0);

    assert!(matches!(
        invoke(|| bun.list_project()).unwrap_err(),
        BunError::InvalidOutput
    ));
}
