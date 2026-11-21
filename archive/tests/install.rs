//! Archive selection and installation tests.
use std::io::{Cursor, Write};

use neotales_ospm_archive::{
    Format, InstallError, InstallOptions, Selector, install_reader, install_verified_archive,
};
use sha2::{Digest, Sha256};

fn tar_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (name, bytes) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, name, *bytes).unwrap();
    }
    builder.into_inner().unwrap()
}

fn options(dir: &std::path::Path, format: Format) -> InstallOptions {
    InstallOptions::new(format, Selector::Glob("*/tool".into()), dir, "0".repeat(64))
}

fn install(bytes: &[u8], opts: &InstallOptions) -> Result<std::path::PathBuf, InstallError> {
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(bytes).unwrap();
    std::io::Seek::rewind(&mut file).unwrap();
    install_verified_archive(&mut file, opts)
}

#[test]
fn installs_supported_tar_compressions() {
    let tar = tar_bytes(&[("bin/tool", b"hello")]);
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(&tar).unwrap();
    let mut xz = xz2::write::XzEncoder::new(Vec::new(), 6);
    xz.write_all(&tar).unwrap();
    let mut bz = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::default());
    bz.write_all(&tar).unwrap();
    for (format, bytes) in [
        (Format::Tar, tar),
        (Format::TarGz, gz.finish().unwrap()),
        (Format::TarXz, xz.finish().unwrap()),
        (Format::TarBz2, bz.finish().unwrap()),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = install(&bytes, &options(dir.path(), format)).unwrap();
        assert_eq!(std::fs::read(path).unwrap(), b"hello");
    }
}

#[test]
fn zip_rename_regex_and_no_overwrite() {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("bin/tool", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"contents").unwrap();
    let zip = zip.finish().unwrap().into_inner();
    let dir = tempfile::tempdir().unwrap();
    let mut opts = options(dir.path(), Format::Zip);
    opts.grep = Selector::Regex("^bin/tool$".into());
    opts.rename = Some("renamed".into());
    assert_eq!(
        std::fs::read(install(&zip, &opts).unwrap()).unwrap(),
        b"contents"
    );
    assert!(matches!(install(&zip, &opts), Err(InstallError::Io(_))));
    assert_eq!(
        std::fs::read(dir.path().join("renamed")).unwrap(),
        b"contents"
    );
}

#[test]
fn rejects_ambiguous_matches_and_oversized_files() {
    let tar = tar_bytes(&[("a/tool", b"a"), ("b/tool", b"b")]);
    let dir = tempfile::tempdir().unwrap();
    let opts = options(dir.path(), Format::Tar);
    assert!(matches!(
        install(&tar, &opts),
        Err(InstallError::MultipleMatches)
    ));
    assert!(!dir.path().join("tool").exists());
    let mut opts = opts;
    opts.max_file_bytes = 1;
    assert!(matches!(
        install(&tar_bytes(&[("a/tool", b"ab")]), &opts),
        Err(InstallError::SizeLimit)
    ));
}

#[test]
fn rejects_selected_symlink() {
    let mut builder = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_size(0);
    header.set_mode(0o777);
    header.set_entry_type(tar::EntryType::Symlink);
    header.set_link_name("/etc/passwd").unwrap();
    header.set_cksum();
    builder
        .append_data(&mut header, "bin/tool", Cursor::new([]))
        .unwrap();
    let archive = builder.into_inner().unwrap();
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        install(&archive, &options(dir.path(), Format::Tar)),
        Err(InstallError::Invalid(_))
    ));
    assert!(!dir.path().join("tool").exists());
}

#[test]
fn rejects_bad_destination_and_plain_http() {
    let dir = tempfile::tempdir().unwrap();
    let mut opts = options(dir.path(), Format::Tar);
    opts.rename = Some("../outside".into());
    assert!(matches!(
        install(&tar_bytes(&[("a/tool", b"a")]), &opts),
        Err(InstallError::Invalid(_))
    ));
    assert!(matches!(
        neotales_ospm_archive::install_https("http://example.org/tool", &opts),
        Err(InstallError::Invalid(_))
    ));
}

#[test]
fn verifies_checksum_before_installing() {
    let tar = tar_bytes(&[("bin/tool", b"binary")]);
    let dir = tempfile::tempdir().unwrap();
    let mut opts = options(dir.path(), Format::Tar);
    assert!(matches!(
        install_reader(Cursor::new(&tar), &opts),
        Err(InstallError::ChecksumMismatch)
    ));
    assert!(!dir.path().join("tool").exists());
    opts.sha256 = format!("{:x}", Sha256::digest(&tar));
    assert_eq!(
        std::fs::read(install_reader(Cursor::new(&tar), &opts).unwrap()).unwrap(),
        b"binary"
    );
}

#[cfg(not(windows))]
#[test]
fn exe_suffix_is_removed_from_patterns_and_renames() {
    let dir = tempfile::tempdir().unwrap();
    let mut opts = options(dir.path(), Format::Tar);
    opts.grep = Selector::Regex(r"bin/tool\.exe$".into());
    opts.rename = Some("custom.exe".into());
    let path = install(&tar_bytes(&[("bin/tool", b"binary")]), &opts).unwrap();
    assert_eq!(path.file_name().unwrap(), "custom");
}

#[cfg(unix)]
#[test]
fn failing_explicit_command_keeps_installed_file() {
    use neotales_ospm_archive::PostInstall;
    let dir = tempfile::tempdir().unwrap();
    let mut opts = options(dir.path(), Format::Tar);
    opts.post_install = Some(PostInstall {
        program: "/bin/sh".into(),
        args: vec!["-c".into(), "exit 9".into()],
    });
    assert!(matches!(
        install(&tar_bytes(&[("bin/tool", b"binary")]), &opts),
        Err(InstallError::PostInstallFailed(_))
    ));
    assert_eq!(std::fs::read(dir.path().join("tool")).unwrap(), b"binary");
}
