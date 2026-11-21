//! Read-only integration tests for an explicitly configured libalpm installation.
use neotales_ospm_core::{List, QueryOptions};
use neotales_ospm_pacman_shim::Pacman;

#[test]
fn installed_packages_when_configured() {
    let Some(path) = std::env::var_os("NEOTALES_OSPM_LIBALPM") else {
        return;
    };
    // SAFETY: The test environment supplies a trusted libalpm 16 library.
    let pacman = unsafe { Pacman::load(path, "/", "/var/lib/pacman/") }.unwrap();
    let packages = pacman.list("pacman", &QueryOptions::default()).unwrap();
    assert!(
        packages
            .iter()
            .any(|p| p.name == "pacman" && !p.version.is_empty())
    );
}
