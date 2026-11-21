//! Archive selection and installation tests.
use std::io::{Cursor, Write};

use neotales_ospm_archive::{
    DEFAULT_METADATA_LIMIT, Format, InstallError, InstallOptions, Selector, install_reader,
    install_verified_archive,
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

fn append_tar_entry(
    builder: &mut tar::Builder<Vec<u8>>,
    entry_type: tar::EntryType,
    name: &str,
    data: &[u8],
) {
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(entry_type);
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder.append_data(&mut header, name, data).unwrap();
}

fn tar_with_gnu_longname(long_name: &str) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    let mut body = long_name.as_bytes().to_vec();
    body.push(0);
    append_tar_entry(
        &mut builder,
        tar::EntryType::GNULongName,
        "././@LongLink",
        &body,
    );
    append_tar_entry(&mut builder, tar::EntryType::Regular, "bin/tool", b"binary");
    builder.into_inner().unwrap()
}

fn tar_with_gnu_longlink(long_link: &str) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    let mut body = long_link.as_bytes().to_vec();
    body.push(0);
    append_tar_entry(
        &mut builder,
        tar::EntryType::GNULongLink,
        "././@LongLink",
        &body,
    );
    append_tar_entry(&mut builder, tar::EntryType::Regular, "bin/tool", b"binary");
    builder.into_inner().unwrap()
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
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
fn rejects_zip_duplicate_names_before_install() {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [("bin/tool", &b"first"[..]), ("bin/toom", &b"second"[..])] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    let mut zip = zip.finish().unwrap().into_inner();
    // ZipWriter rejects duplicate names, so alter both headers after writing.
    let mut replacements = 0;
    for offset in 0..zip.len() - b"bin/toom".len() {
        if &zip[offset..offset + 8] == b"bin/toom" {
            zip[offset + 7] = b'l';
            replacements += 1;
        }
    }
    assert_eq!(replacements, 2);
    assert!(
        zip::ZipArchive::new(Cursor::new(&zip))
            .unwrap()
            .has_duplicate_names()
    );
    let dir = tempfile::tempdir().unwrap();
    assert!(install(&zip, &options(dir.path(), Format::Zip)).is_err());
    assert!(!dir.path().join("tool").exists());
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

    let dir = tempfile::tempdir().unwrap();
    let mut opts = options(dir.path(), Format::Tar);
    opts.grep = Selector::Glob("*/tool.exe".into());
    let path = install(&tar_bytes(&[("bin/tool", b"binary")]), &opts).unwrap();
    assert_eq!(path.file_name().unwrap(), "tool");
}

#[cfg(not(windows))]
#[test]
fn exe_embedded_in_pattern_or_rename_is_not_rewritten() {
    let dir = tempfile::tempdir().unwrap();
    let mut opts = options(dir.path(), Format::Tar);
    opts.grep = Selector::Regex(r"^bin/tool\.exe-backup$".into());
    opts.rename = Some("custom.exe.bak".into());
    let path = install(&tar_bytes(&[("bin/tool.exe-backup", b"binary")]), &opts).unwrap();
    assert_eq!(path.file_name().unwrap(), "custom.exe.bak");

    let dir = tempfile::tempdir().unwrap();
    let mut opts = options(dir.path(), Format::Tar);
    opts.grep = Selector::Regex(r"^bin/tool\.exe\.bak$".into());
    let path = install(&tar_bytes(&[("bin/tool.exe.bak", b"binary")]), &opts).unwrap();
    assert_eq!(path.file_name().unwrap(), "tool.exe.bak");
}

#[cfg(not(windows))]
#[test]
fn exe_normalization_can_be_disabled() {
    let tar = tar_bytes(&[("bin/tool.exe", b"exe-binary"), ("bin/tool", b"plain")]);
    let dir = tempfile::tempdir().unwrap();
    assert!(options(dir.path(), Format::Tar).normalize_exe_suffix);

    let mut opts = options(dir.path(), Format::Tar);
    opts.grep = Selector::Regex(r"^bin/tool\.exe$".into());
    opts.rename = Some("tool.exe".into());
    opts.normalize_exe_suffix = false;
    let path = install(&tar, &opts).unwrap();
    assert_eq!(path.file_name().unwrap(), "tool.exe");
    assert_eq!(std::fs::read(path).unwrap(), b"exe-binary");

    let dir = tempfile::tempdir().unwrap();
    let mut opts = options(dir.path(), Format::Tar);
    opts.grep = Selector::Regex(r"^bin/tool\.exe$".into());
    opts.rename = Some("tool.exe".into());
    let path = install(&tar, &opts).unwrap();
    assert_eq!(path.file_name().unwrap(), "tool");
    assert_eq!(std::fs::read(path).unwrap(), b"plain");
}

#[test]
fn scan_limit_counts_skipped_compressed_members() {
    let big = vec![0u8; 1024 * 1024];
    let tar = tar_bytes(&[("other/big", &big), ("bin/tool", b"binary")]);
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(&tar).unwrap();
    let archive = gz.finish().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut opts = options(dir.path(), Format::TarGz);
    opts.sha256 = format!("{:x}", Sha256::digest(&archive));
    opts.max_scan_bytes = 4096;
    let result = install_reader(Cursor::new(&archive), &opts);
    assert!(
        matches!(result, Err(InstallError::ScanLimit)),
        "unexpected: {result:?}"
    );
    assert!(!dir.path().join("tool").exists());
}

#[test]
fn max_metadata_bytes_defaults_to_bounded_limit() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        options(dir.path(), Format::Tar).max_metadata_bytes,
        DEFAULT_METADATA_LIMIT
    );
}

#[test]
fn gnu_long_name_within_limit_still_installs() {
    let long_path = format!("bin/{}/tool", "a".repeat(120));
    let tar = tar_with_gnu_longname(&long_path);
    let dir = tempfile::tempdir().unwrap();
    let mut opts = options(dir.path(), Format::Tar);
    opts.grep = Selector::Glob("bin/*/tool".into());
    let path = install(&tar, &opts).unwrap();
    assert_eq!(path.file_name().unwrap(), "tool");
    assert_eq!(std::fs::read(path).unwrap(), b"binary");
}

#[test]
fn rejects_oversized_gnu_metadata_in_compressed_archive() {
    let long_name = format!("bin/{}/tool", "a".repeat(1024 * 1024));
    let archive = gzip(&tar_with_gnu_longname(&long_name));
    let dir = tempfile::tempdir().unwrap();
    let mut opts = options(dir.path(), Format::TarGz);
    opts.sha256 = format!("{:x}", Sha256::digest(&archive));
    let result = install_reader(Cursor::new(&archive), &opts);
    assert!(
        matches!(result, Err(InstallError::Invalid(_))),
        "unexpected: {result:?}"
    );
    assert!(!dir.path().join("tool").exists());
}

#[test]
fn small_gnu_longlink_is_ignored() {
    let archive = tar_with_gnu_longlink("bin/tool-link");
    let dir = tempfile::tempdir().unwrap();
    let mut opts = options(dir.path(), Format::Tar);
    opts.grep = Selector::Glob("bin/tool".into());
    let path = install(&archive, &opts).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), b"binary");
}

#[test]
fn rejects_oversized_gnu_longlink_before_install() {
    let archive = tar_with_gnu_longlink(&"l".repeat(1024 * 1024));
    let dir = tempfile::tempdir().unwrap();
    let mut opts = options(dir.path(), Format::Tar);
    opts.grep = Selector::Glob("bin/tool".into());
    let result = install(&archive, &opts);
    assert!(
        matches!(result, Err(InstallError::Invalid(_))),
        "unexpected: {result:?}"
    );
    assert!(!dir.path().join("tool").exists());
}

#[test]
fn rejects_pax_local_extended_headers_before_install() {
    let mut builder = tar::Builder::new(Vec::new());
    append_tar_entry(
        &mut builder,
        tar::EntryType::XHeader,
        "pax_header",
        b"path=bin/tool\n",
    );
    append_tar_entry(&mut builder, tar::EntryType::Regular, "bin/tool", b"binary");
    let archive = builder.into_inner().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut opts = options(dir.path(), Format::Tar);
    opts.grep = Selector::Glob("bin/tool".into());
    let result = install(&archive, &opts);
    assert!(
        matches!(result, Err(InstallError::Invalid(_))),
        "unexpected: {result:?}"
    );
    assert!(!dir.path().join("tool").exists());
}

#[test]
fn rejects_global_pax_path_override_before_install() {
    let mut builder = tar::Builder::new(Vec::new());
    append_tar_entry(
        &mut builder,
        tar::EntryType::XGlobalHeader,
        "pax_global_header",
        b"22 path=bin/evil/tool\n",
    );
    append_tar_entry(&mut builder, tar::EntryType::Regular, "bin/tool", b"binary");
    let archive = builder.into_inner().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut opts = options(dir.path(), Format::Tar);
    opts.grep = Selector::Glob("bin/tool".into());
    let result = install(&archive, &opts);
    assert!(
        matches!(result, Err(InstallError::Invalid(_))),
        "unexpected: {result:?}"
    );
    assert!(!dir.path().join("tool").exists());
}

#[test]
fn rejects_global_pax_size_override_before_install() {
    let mut builder = tar::Builder::new(Vec::new());
    append_tar_entry(
        &mut builder,
        tar::EntryType::XGlobalHeader,
        "pax_global_header",
        b"16 size=1048576\n",
    );
    append_tar_entry(&mut builder, tar::EntryType::Regular, "bin/tool", b"binary");
    let archive = builder.into_inner().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut opts = options(dir.path(), Format::Tar);
    opts.grep = Selector::Glob("bin/tool".into());
    let result = install(&archive, &opts);
    assert!(
        matches!(result, Err(InstallError::Invalid(_))),
        "unexpected: {result:?}"
    );
    assert!(!dir.path().join("tool").exists());
}

#[test]
fn rejects_gnu_sparse_members_before_install() {
    let mut builder = tar::Builder::new(Vec::new());
    append_tar_entry(&mut builder, tar::EntryType::GNUSparse, "bin/sparse", b"");
    append_tar_entry(&mut builder, tar::EntryType::Regular, "bin/tool", b"binary");
    let archive = builder.into_inner().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut opts = options(dir.path(), Format::Tar);
    opts.grep = Selector::Glob("bin/tool".into());
    let result = install(&archive, &opts);
    assert!(
        matches!(result, Err(InstallError::Invalid(_))),
        "unexpected: {result:?}"
    );
    assert!(!dir.path().join("tool").exists());
}

#[test]
fn rejects_dangling_gnu_long_name() {
    let mut builder = tar::Builder::new(Vec::new());
    append_tar_entry(
        &mut builder,
        tar::EntryType::GNULongName,
        "././@LongLink",
        b"bin/tool\0",
    );
    let archive = builder.into_inner().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let opts = options(dir.path(), Format::Tar);
    let result = install(&archive, &opts);
    assert!(
        matches!(result, Err(InstallError::Invalid(_))),
        "unexpected: {result:?}"
    );
    assert!(!dir.path().join("tool").exists());
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
        Err(InstallError::PostInstallFailed { .. })
    ));
    assert_eq!(std::fs::read(dir.path().join("tool")).unwrap(), b"binary");
}

#[cfg(unix)]
#[test]
fn post_install_spawn_failure_reports_installed_file() {
    use neotales_ospm_archive::PostInstall;
    let dir = tempfile::tempdir().unwrap();
    let mut opts = options(dir.path(), Format::Tar);
    opts.post_install = Some(PostInstall {
        program: "/nonexistent/neotales-ospm-missing".into(),
        args: vec![],
    });
    let installed = dir.path().join("tool");
    match install(&tar_bytes(&[("bin/tool", b"binary")]), &opts) {
        Err(InstallError::PostInstallSpawnFailed {
            installed: reported,
            ..
        }) => assert_eq!(reported, installed),
        other => panic!("unexpected result: {other:?}"),
    }
    assert_eq!(std::fs::read(installed).unwrap(), b"binary");
}

#[cfg(not(unix))]
#[test]
fn archive_scripts_are_rejected_before_install() {
    use neotales_ospm_archive::ArchiveScript;
    let archive = tar_bytes(&[("bin/tool", b"binary"), ("scripts/install", b"script")]);
    let dir = tempfile::tempdir().unwrap();
    let mut opts = options(dir.path(), Format::Tar);
    opts.sha256 = format!("{:x}", Sha256::digest(&archive));
    opts.archive_script = Some(ArchiveScript::new(Selector::Glob("scripts/install".into())));
    assert!(matches!(
        install_reader(Cursor::new(&archive), &opts),
        Err(InstallError::Invalid(_))
    ));
    assert!(!dir.path().join("tool").exists());
}

#[cfg(unix)]
mod archive_script {
    use std::ffi::OsString;
    use std::path::Path;

    use neotales_ospm_archive::ArchiveScript;

    use super::*;

    fn digest(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }

    fn tar_with_script(script: &str) -> Vec<u8> {
        tar_bytes(&[
            ("bin/tool", b"binary"),
            ("scripts/install.sh", script.as_bytes()),
        ])
    }

    fn options_with_script(
        dir: &Path,
        archive: &[u8],
        selector: &str,
        args: Vec<OsString>,
    ) -> InstallOptions {
        let mut opts = options(dir, Format::Tar);
        opts.grep = Selector::Glob("bin/tool".into());
        opts.sha256 = digest(archive);
        opts.archive_script = Some(ArchiveScript {
            selector: Selector::Glob(selector.into()),
            args,
        });
        opts
    }

    #[test]
    fn archive_script_is_opt_in() {
        let dir = tempfile::tempdir().unwrap();
        assert!(options(dir.path(), Format::Tar).archive_script.is_none());
    }

    #[test]
    fn runs_explicit_archive_script_after_verified_install() {
        let dir = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let marker = output.path().join("marker");
        let cwd_file = output.path().join("cwd");
        let archive = tar_with_script("#!/bin/sh\nprintf ran > \"$1\"\npwd > \"$2\"\n");
        let opts = options_with_script(
            dir.path(),
            &archive,
            "scripts/install.sh",
            vec![
                marker.clone().into_os_string(),
                cwd_file.clone().into_os_string(),
            ],
        );
        let installed = install_reader(Cursor::new(&archive), &opts).unwrap();
        assert_eq!(std::fs::read(installed).unwrap(), b"binary");
        assert_eq!(std::fs::read(&marker).unwrap(), b"ran");
        let cwd = std::fs::read_to_string(&cwd_file).unwrap();
        assert!(
            !Path::new(cwd.trim()).exists(),
            "script working directory must be private and removed"
        );
    }

    #[test]
    fn runs_zip_archive_script() {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file("bin/tool", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"binary").unwrap();
        zip.start_file(
            "scripts/install.sh",
            zip::write::SimpleFileOptions::default().unix_permissions(0o100755),
        )
        .unwrap();
        zip.write_all(b"#!/bin/sh\nprintf ran > \"$1\"\n").unwrap();
        let archive = zip.finish().unwrap().into_inner();
        let dir = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let marker = output.path().join("marker");
        let mut opts = options(dir.path(), Format::Zip);
        opts.grep = Selector::Glob("bin/tool".into());
        opts.sha256 = digest(&archive);
        opts.archive_script = Some(ArchiveScript {
            selector: Selector::Glob("scripts/install.sh".into()),
            args: vec![marker.clone().into_os_string()],
        });
        let installed = install_reader(Cursor::new(&archive), &opts).unwrap();
        assert_eq!(std::fs::read(installed).unwrap(), b"binary");
        assert_eq!(std::fs::read(marker).unwrap(), b"ran");
    }

    #[test]
    fn rejects_script_without_verified_digest() {
        let archive = tar_with_script("#!/bin/sh\nexit 0\n");
        let dir = tempfile::tempdir().unwrap();
        let opts = options_with_script(dir.path(), &archive, "scripts/install.sh", vec![]);
        assert!(matches!(
            install(&archive, &opts),
            Err(InstallError::Invalid(_))
        ));
        assert!(!dir.path().join("tool").exists());
    }

    #[test]
    fn rejects_link_script_before_install() {
        let mut builder = tar::Builder::new(Vec::new());
        let mut tool = tar::Header::new_gnu();
        tool.set_size(6);
        tool.set_mode(0o644);
        tool.set_cksum();
        builder
            .append_data(&mut tool, "bin/tool", &b"binary"[..])
            .unwrap();
        let mut link = tar::Header::new_gnu();
        link.set_size(0);
        link.set_mode(0o777);
        link.set_entry_type(tar::EntryType::Symlink);
        link.set_link_name("/bin/sh").unwrap();
        link.set_cksum();
        builder
            .append_data(&mut link, "scripts/install.sh", Cursor::new([]))
            .unwrap();
        let archive = builder.into_inner().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let opts = options_with_script(dir.path(), &archive, "scripts/install.sh", vec![]);
        assert!(matches!(
            install_reader(Cursor::new(&archive), &opts),
            Err(InstallError::Invalid(_))
        ));
        assert!(!dir.path().join("tool").exists());
    }

    #[test]
    fn rejects_script_path_traversal_before_install() {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file("bin/tool", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"binary").unwrap();
        zip.start_file("../install.sh", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"#!/bin/sh\nexit 0\n").unwrap();
        let archive = zip.finish().unwrap().into_inner();
        let dir = tempfile::tempdir().unwrap();
        let mut opts = options(dir.path(), Format::Zip);
        opts.grep = Selector::Glob("bin/tool".into());
        opts.sha256 = digest(&archive);
        opts.archive_script = Some(ArchiveScript::new(Selector::Glob("../install.sh".into())));
        assert!(matches!(
            install_reader(Cursor::new(&archive), &opts),
            Err(InstallError::Invalid(_))
        ));
        assert!(!dir.path().join("tool").exists());
        assert!(!dir.path().parent().unwrap().join("install.sh").exists());
    }

    #[test]
    fn rejects_oversized_script_before_install() {
        let archive = tar_with_script("#!/bin/sh\nprintf longer than the limit\n");
        let dir = tempfile::tempdir().unwrap();
        let mut opts = options_with_script(dir.path(), &archive, "scripts/install.sh", vec![]);
        opts.max_file_bytes = 8;
        assert!(matches!(
            install_reader(Cursor::new(&archive), &opts),
            Err(InstallError::SizeLimit)
        ));
        assert!(!dir.path().join("tool").exists());
    }

    #[test]
    fn rejects_shebang_less_script_instead_of_running_a_shell() {
        let dir = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let marker = output.path().join("marker");
        let archive = tar_with_script("printf ran > \"$1\"\n");
        let opts = options_with_script(
            dir.path(),
            &archive,
            "scripts/install.sh",
            vec![marker.clone().into_os_string()],
        );
        assert!(matches!(
            install_reader(Cursor::new(&archive), &opts),
            Err(InstallError::Invalid(_))
        ));
        assert!(!marker.exists());
        assert!(!dir.path().join("tool").exists());
    }

    #[test]
    fn rejects_bare_shebang_instead_of_running_a_shell() {
        let dir = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let marker = output.path().join("marker");
        let archive = tar_with_script("#!\nprintf ran > \"$1\"\n");
        let opts = options_with_script(
            dir.path(),
            &archive,
            "scripts/install.sh",
            vec![marker.clone().into_os_string()],
        );
        assert!(matches!(
            install_reader(Cursor::new(&archive), &opts),
            Err(InstallError::Invalid(_))
        ));
        assert!(!marker.exists());
        assert!(!dir.path().join("tool").exists());
    }

    #[test]
    fn accepts_shebang_with_leading_space() {
        let dir = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let marker = output.path().join("marker");
        let archive = tar_with_script("#! /bin/sh\nprintf ran > \"$1\"\n");
        let opts = options_with_script(
            dir.path(),
            &archive,
            "scripts/install.sh",
            vec![marker.clone().into_os_string()],
        );
        install_reader(Cursor::new(&archive), &opts).unwrap();
        assert_eq!(std::fs::read(marker).unwrap(), b"ran");
    }

    #[test]
    fn rejects_ambiguous_scripts_before_install() {
        let tar = tar_bytes(&[
            ("bin/tool", b"binary"),
            ("a/install.sh", b"#!/bin/sh\nexit 0\n"),
            ("b/install.sh", b"#!/bin/sh\nexit 0\n"),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let opts = options_with_script(dir.path(), &tar, "*/install.sh", vec![]);
        assert!(matches!(
            install_reader(Cursor::new(&tar), &opts),
            Err(InstallError::MultipleMatches)
        ));
        assert!(!dir.path().join("tool").exists());
    }

    #[test]
    fn failing_script_keeps_installed_file() {
        let archive = tar_with_script("#!/bin/sh\nexit 3\n");
        let dir = tempfile::tempdir().unwrap();
        let opts = options_with_script(dir.path(), &archive, "scripts/install.sh", vec![]);
        let installed = dir.path().join("tool");
        match install_reader(Cursor::new(&archive), &opts) {
            Err(InstallError::ScriptFailed {
                installed: reported,
                ..
            }) => assert_eq!(reported, installed),
            other => panic!("unexpected result: {other:?}"),
        }
        assert_eq!(std::fs::read(installed).unwrap(), b"binary");
    }

    #[test]
    fn script_spawn_failure_reports_installed_file() {
        let archive = tar_with_script("#!/nonexistent/neotales-ospm-missing\nexit 0\n");
        let dir = tempfile::tempdir().unwrap();
        let opts = options_with_script(dir.path(), &archive, "scripts/install.sh", vec![]);
        let installed = dir.path().join("tool");
        match install_reader(Cursor::new(&archive), &opts) {
            Err(InstallError::ScriptSpawnFailed {
                installed: reported,
                ..
            }) => assert_eq!(reported, installed),
            other => panic!("unexpected result: {other:?}"),
        }
        assert_eq!(std::fs::read(installed).unwrap(), b"binary");
    }

    #[test]
    fn scan_limit_is_shared_across_script_and_executable_scans() {
        let archive = tar_with_script("#!/bin/sh\nexit 0\n");
        let dir = tempfile::tempdir().unwrap();
        let mut opts = options_with_script(dir.path(), &archive, "scripts/install.sh", vec![]);
        opts.max_scan_bytes = archive.len() as u64 * 3 / 2;
        let result = install_reader(Cursor::new(&archive), &opts);
        assert!(
            matches!(result, Err(InstallError::ScanLimit)),
            "unexpected: {result:?}"
        );
        assert!(!dir.path().join("tool").exists());
    }

    #[test]
    fn script_selector_with_embedded_exe_is_not_rewritten() {
        let archive = tar_bytes(&[
            ("bin/tool", b"binary"),
            (
                "scripts/install.exe.sh",
                b"#!/bin/sh\nprintf ran > \"$1\"\n",
            ),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let marker = output.path().join("marker");
        let mut opts = options(dir.path(), Format::Tar);
        opts.grep = Selector::Glob("bin/tool".into());
        opts.sha256 = digest(&archive);
        opts.archive_script = Some(ArchiveScript {
            selector: Selector::Regex(r"^scripts/install\.exe\.sh$".into()),
            args: vec![marker.clone().into_os_string()],
        });
        install_reader(Cursor::new(&archive), &opts).unwrap();
        assert_eq!(std::fs::read(marker).unwrap(), b"ran");
    }
}
