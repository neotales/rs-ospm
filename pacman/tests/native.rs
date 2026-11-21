//! End-to-end tests with an explicitly supplied native shim.
use neotales_ospm_core::{List, QueryOptions};
use neotales_ospm_pacman::Pacman;

#[test]
fn list_installed_when_configured() {
    let (Some(shim), Some(alpm)) = (
        std::env::var_os("NEOTALES_OSPM_PACMAN_SHIM"),
        std::env::var_os("NEOTALES_OSPM_LIBALPM"),
    ) else {
        return;
    };
    // SAFETY: The test environment supplies trusted compatible libraries.
    let pacman =
        unsafe { Pacman::load(shim, alpm.to_str().unwrap(), "/", "/var/lib/pacman/") }.unwrap();
    let packages = pacman.list("pacman", &QueryOptions::default()).unwrap();
    assert!(
        packages
            .iter()
            .any(|p| p.name == "pacman" && !p.version.is_empty())
    );
}
