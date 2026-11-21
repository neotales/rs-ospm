//! Verify command construction, JSON parsing and validation with fake pnpm
//! processes. No test installs a package or touches a real pnpm directory.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use neotales_ospm_pnpm_cli::{
    DependencySection, PackageName, PackageSpec, Pnpm, PnpmError, SearchOptions,
};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "ospm-pnpm-cli-{name}-{}-{}",
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
        let executable = self.0.join("pnpm");
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

fn read_args(executable: &Path) -> String {
    fs::read_to_string(executable.with_extension("args")).unwrap()
}

fn read_cwd(executable: &Path) -> PathBuf {
    let cwd = fs::read_to_string(executable.with_extension("cwd")).unwrap();
    fs::canonicalize(cwd.trim()).unwrap()
}

fn invoke<T>(mut action: impl FnMut() -> Result<T, PnpmError>) -> Result<T, PnpmError> {
    for attempt in 0..20 {
        match action() {
            Err(PnpmError::Io(error)) if error.raw_os_error() == Some(26) && attempt < 19 => {
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
    let pnpm = Pnpm::with_executable(&executable);
    let spec = PackageSpec::exact("example-tool", "1.2.3").unwrap();

    invoke(|| pnpm.add_global(&spec)).unwrap();

    assert_eq!(
        read_args(&executable),
        "add\n--global\n--save-exact\nexample-tool@1.2.3\n"
    );
}

#[test]
fn add_global_without_version_omits_save_exact() {
    let scratch = Scratch::new("add-global-latest");
    let executable = scratch.write_script(RECORD);
    let pnpm = Pnpm::with_executable(&executable);
    let spec = PackageSpec::new("example-tool").unwrap();

    invoke(|| pnpm.add_global(&spec)).unwrap();

    assert_eq!(read_args(&executable), "add\n--global\nexample-tool\n");
}

#[test]
fn remove_global_passes_global_flag_and_name() {
    let scratch = Scratch::new("remove-global");
    let executable = scratch.write_script(RECORD);
    let pnpm = Pnpm::with_executable(&executable);
    let package = PackageName::new("@example/tool").unwrap();

    invoke(|| pnpm.remove_global(&package)).unwrap();

    assert_eq!(read_args(&executable), "remove\n--global\n@example/tool\n");
}

#[test]
fn add_project_uses_directory_and_selected_section() {
    let scratch = Scratch::new("add-project");
    let executable = scratch.write_script(RECORD);
    let pnpm = Pnpm::with_executable(&executable).with_project_directory(&scratch.0);
    let spec = PackageSpec::exact("@example/tool", "2.0.0-rc.1").unwrap();

    invoke(|| pnpm.add_project(&spec, DependencySection::Development)).unwrap();

    assert_eq!(
        read_args(&executable),
        "add\n--save-dev\n--save-exact\n@example/tool@2.0.0-rc.1\n"
    );
    assert_eq!(read_cwd(&executable), fs::canonicalize(&scratch.0).unwrap());
}

#[test]
fn add_project_regular_section_passes_save_prod() {
    let scratch = Scratch::new("add-regular");
    let executable = scratch.write_script(RECORD);
    let pnpm = Pnpm::with_executable(&executable).with_project_directory(&scratch.0);
    let spec = PackageSpec::new("example-tool").unwrap();

    invoke(|| pnpm.add_project(&spec, DependencySection::Regular)).unwrap();

    assert_eq!(read_args(&executable), "add\n--save-prod\nexample-tool\n");
}

#[test]
fn add_project_maps_optional_and_peer_sections() {
    let scratch = Scratch::new("add-sections");
    let executable = scratch.write_script(RECORD);
    let pnpm = Pnpm::with_executable(&executable).with_project_directory(&scratch.0);
    let spec = PackageSpec::new("example-tool").unwrap();

    invoke(|| pnpm.add_project(&spec, DependencySection::Optional)).unwrap();
    assert_eq!(
        read_args(&executable),
        "add\n--save-optional\nexample-tool\n"
    );

    invoke(|| pnpm.add_project(&spec, DependencySection::Peer)).unwrap();
    assert_eq!(read_args(&executable), "add\n--save-peer\nexample-tool\n");
}

#[test]
fn remove_project_uses_directory() {
    let scratch = Scratch::new("remove-project");
    let executable = scratch.write_script(RECORD);
    let pnpm = Pnpm::with_executable(&executable).with_project_directory(&scratch.0);
    let package = PackageName::new("example-tool").unwrap();

    invoke(|| pnpm.remove_project(&package)).unwrap();

    assert_eq!(read_args(&executable), "remove\nexample-tool\n");
    assert_eq!(read_cwd(&executable), fs::canonicalize(&scratch.0).unwrap());
}

#[test]
fn install_project_isolates_the_project_from_a_workspace() {
    let scratch = Scratch::new("install-project");
    let executable = scratch.write_script(RECORD);
    let pnpm = Pnpm::with_executable(&executable).with_project_directory(&scratch.0);

    invoke(|| pnpm.install_project()).unwrap();

    assert_eq!(read_args(&executable), "install\n--ignore-workspace\n");
    assert_eq!(read_cwd(&executable), fs::canonicalize(&scratch.0).unwrap());
}

#[test]
fn install_workspace_uses_the_plain_install_command() {
    let scratch = Scratch::new("install-workspace");
    let executable = scratch.write_script(RECORD);
    let pnpm = Pnpm::with_executable(&executable).with_project_directory(&scratch.0);

    invoke(|| pnpm.install_workspace()).unwrap();

    assert_eq!(read_args(&executable), "install\n");
    assert_eq!(read_cwd(&executable), fs::canonicalize(&scratch.0).unwrap());
}

#[test]
fn list_global_parses_direct_dependencies() {
    let scratch = Scratch::new("list-global");
    let executable = scratch.write_script(
        "#!/bin/sh\n\
         printf '%s\\n' \"$@\" > \"$0.args\"\n\
         cat <<'JSON'\n\
         [\n\
           {\n\
             \"path\": \"/global/store\",\n\
             \"private\": true,\n\
             \"dependencies\": {\n\
               \"example-tool\": {\n\
                 \"from\": \"example-tool\",\n\
                 \"version\": \"1.2.3\",\n\
                 \"path\": \"/global/store/example-tool\"\n\
               },\n\
               \"@example/scoped\": {\n\
                 \"from\": \"@example/scoped\",\n\
                 \"version\": \"2.0.0\"\n\
               }\n\
             }\n\
           }\n\
         ]\n\
         JSON\n",
    );
    let pnpm = Pnpm::with_executable(&executable);

    let mut packages = invoke(|| pnpm.list_global()).unwrap();
    packages.sort_by(|left, right| left.name.cmp(&right.name));

    assert_eq!(
        read_args(&executable),
        "list\n--global\n--depth\n0\n--json\n"
    );
    assert_eq!(packages.len(), 2);
    assert_eq!(packages[0].name, "@example/scoped");
    assert_eq!(packages[0].version, "2.0.0");
    assert_eq!(packages[1].name, "example-tool");
    assert_eq!(packages[1].version, "1.2.3");
}

#[test]
fn global_operations_ignore_project_directory() {
    let scratch = Scratch::new("global-cwd");
    let executable = scratch.write_script(RECORD);
    let pnpm = Pnpm::with_executable(&executable).with_project_directory(&scratch.0);
    let spec = PackageSpec::new("example-tool").unwrap();

    invoke(|| pnpm.add_global(&spec)).unwrap();

    let current = std::env::current_dir().unwrap();
    assert_eq!(read_cwd(&executable), fs::canonicalize(current).unwrap());
}

#[test]
fn list_project_keeps_sections_and_project_fields() {
    let scratch = Scratch::new("list-project");
    let executable = scratch.write_script(
        "#!/bin/sh\n\
         printf '%s\\n' \"$@\" > \"$0.args\"\n\
         pwd > \"$0.cwd\"\n\
         cat <<'JSON'\n\
         [\n\
           {\n\
             \"name\": \"demo\",\n\
             \"version\": \"0.1.0\",\n\
             \"path\": \"/work/demo\",\n\
             \"private\": true,\n\
             \"dependencies\": { \"regular\": { \"version\": \"1.0.0\" } },\n\
             \"devDependencies\": { \"dev\": { \"version\": \"2.0.0\" } },\n\
             \"optionalDependencies\": { \"optional\": { \"version\": \"3.0.0\" } },\n\
             \"peerDependencies\": { \"peer\": { \"version\": \"4.0.0\" } }\n\
           }\n\
         ]\n\
         JSON\n",
    );
    let pnpm = Pnpm::with_executable(&executable).with_project_directory(&scratch.0);

    let projects = invoke(|| pnpm.list_project()).unwrap();

    assert_eq!(read_args(&executable), "list\n--depth\n0\n--json\n");
    assert_eq!(read_cwd(&executable), fs::canonicalize(&scratch.0).unwrap());
    assert_eq!(projects.len(), 1);
    let project = &projects[0];
    assert_eq!(project.name.as_deref(), Some("demo"));
    assert_eq!(project.version.as_deref(), Some("0.1.0"));
    assert_eq!(project.path.as_deref(), Some("/work/demo"));
    assert_eq!(project.dependencies.len(), 1);
    assert_eq!(project.dependencies[0].name, "regular");
    assert_eq!(project.dev_dependencies[0].name, "dev");
    assert_eq!(project.optional_dependencies[0].name, "optional");
    assert_eq!(project.peer_dependencies[0].name, "peer");
}

#[test]
fn list_project_accepts_an_empty_array() {
    let scratch = Scratch::new("list-project-empty");
    let executable = scratch.write_script("#!/bin/sh\nprintf '[]'\n");
    let pnpm = Pnpm::with_executable(&executable).with_project_directory(&scratch.0);

    assert!(invoke(|| pnpm.list_project()).unwrap().is_empty());
}

#[test]
fn search_passes_limit_and_query_after_separator() {
    let scratch = Scratch::new("search");
    let executable = scratch.write_script(
        "#!/bin/sh\n\
         printf '%s\\n' \"$@\" > \"$0.args\"\n\
         cat <<'JSON'\n\
         [\n\
           { \"name\": \"example-tool\", \"version\": \"1.2.3\", \"description\": \"A tool\" },\n\
           { \"name\": \"other\", \"version\": \"0.0.1\", \"description\": null }\n\
         ]\n\
         JSON\n",
    );
    let pnpm = Pnpm::with_executable(&executable);
    let options = SearchOptions { limit: Some(5) };

    let results = invoke(|| pnpm.search("-example", &options)).unwrap();

    assert_eq!(
        read_args(&executable),
        "search\n--json\n--search-limit\n5\n--\n-example\n"
    );
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].name, "example-tool");
    assert_eq!(results[0].description.as_deref(), Some("A tool"));
    assert_eq!(results[1].description, None);
}

#[test]
fn search_without_limit_leaves_the_default() {
    let scratch = Scratch::new("search-default");
    let executable =
        scratch.write_script("#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$0.args\"\nprintf '[]'\n");
    let pnpm = Pnpm::with_executable(&executable);

    assert!(
        invoke(|| pnpm.search("example", &SearchOptions::default()))
            .unwrap()
            .is_empty()
    );
    assert_eq!(read_args(&executable), "search\n--json\n--\nexample\n");
}

#[test]
fn failure_reports_status_and_captured_stderr() {
    let scratch = Scratch::new("failure");
    let executable = scratch.write_script(
        "#!/bin/sh\n\
         printf 'cannot add\\n' >&2\n\
         exit 7\n",
    );
    let pnpm = Pnpm::with_executable(&executable);
    let spec = PackageSpec::new("example-tool").unwrap();

    match invoke(|| pnpm.add_global(&spec)).unwrap_err() {
        PnpmError::Exit { status, stderr } => {
            assert_eq!(status.code(), Some(7));
            assert_eq!(stderr, b"cannot add\n");
        }
        error => panic!("unexpected error: {error}"),
    }
}

#[test]
fn invalid_json_is_a_typed_error() {
    let scratch = Scratch::new("bad-json");
    let executable = scratch.write_script("#!/bin/sh\nprintf 'not json'\n");
    let pnpm = Pnpm::with_executable(&executable);

    assert!(matches!(
        invoke(|| pnpm.list_global()).unwrap_err(),
        PnpmError::InvalidOutput
    ));
}

#[test]
fn unexpected_json_shape_is_a_typed_error() {
    let scratch = Scratch::new("bad-shape");
    let executable = scratch
        .write_script("#!/bin/sh\nprintf '%s' '[{\"dependencies\":{\"tool\":{\"version\":7}}}]'\n");
    let pnpm = Pnpm::with_executable(&executable);

    assert!(matches!(
        invoke(|| pnpm.list_global()).unwrap_err(),
        PnpmError::InvalidOutput
    ));
}
