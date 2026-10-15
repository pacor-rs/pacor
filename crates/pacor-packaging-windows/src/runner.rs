//! Reading the parts of a Flutter Windows runner project pacor needs.
//!
//! Flutter writes the executable name into `windows/CMakeLists.txt` and the
//! organisation copyright into `windows/runner/Runner.rc`. Both are generated
//! by `flutter create`, and both can be edited afterwards, so neither is
//! assumed to still match `pubspec.yaml`.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

use pacor_core::error::{BuildError, PlatformError};

/// Name assigned to the runner executable by `windows/CMakeLists.txt`.
const BINARY_NAME_KEY: &str = "BINARY_NAME";

/// Extracts `set(BINARY_NAME "app")` from a `CMakeLists.txt`.
///
/// The value is searched for on any line mentioning `BINARY_NAME`, so spacing
/// and a project that uses `set( BINARY_NAME … )` both work; only the spelling
/// of the key matters.
pub(super) fn parse_binary_name(contents: &str) -> Option<String> {
    for line in contents.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }

        let Some(key_start) = line.find(BINARY_NAME_KEY) else {
            continue;
        };

        // The name is the first quoted string on the line.
        let rest = &line[key_start + BINARY_NAME_KEY.len()..];
        let value_start = rest.find('"')? + 1;
        let value_end = rest[value_start..].find('"')? + value_start;
        let name = rest[value_start..value_end].trim();

        // A name is a file stem, never a path.
        if name.is_empty() || name.contains(['/', '\\']) {
            return None;
        }

        return Some(name.to_string());
    }

    None
}

/// Extracts the `LegalCopyright` value from a `Runner.rc`.
///
/// Flutter writes `VALUE "LegalCopyright", "Copyright (C) 2025 org" "\0"`, but
/// the value is arbitrary user text: it may be empty and does not have to end in
/// a full stop. Taking the first quoted string after the key therefore cannot be
/// replaced by matching a suffix, which used to leak the trailing `" "\0"` into
/// the installer.
pub(super) fn parse_legal_copyright(contents: &str) -> Option<String> {
    const KEY: &str = "VALUE \"LegalCopyright\"";

    for line in contents.lines() {
        let Some(key_start) = line.find(KEY) else {
            continue;
        };

        let rest = &line[key_start + KEY.len()..];
        let value_start = rest.find('"')? + 1;
        let value_end = rest[value_start..].find('"')? + value_start;
        let value = rest[value_start..value_end].trim();

        if !value.is_empty() {
            return Some(value.to_string());
        }
    }

    None
}

/// Reads a text file, tolerating the encodings Visual Studio writes for `.rc`.
///
/// Resource scripts are saved as UTF-16 with a BOM when edited in the Visual
/// Studio resource editor, so a plain `read_to_string` would fail on a perfectly
/// valid project.
pub(super) fn read_text_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;

    if let Some(body) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return decode_utf16(body, u16::from_le_bytes)
            .with_context(|| format!("{} is not valid UTF-16LE", path.display()));
    }

    if let Some(body) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return decode_utf16(body, u16::from_be_bytes)
            .with_context(|| format!("{} is not valid UTF-16BE", path.display()));
    }

    let body = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);

    Ok(String::from_utf8_lossy(body).into_owned())
}

/// Decodes UTF-16 code units into a string, stopping at an unterminated unit.
fn decode_utf16(body: &[u8], decode: fn([u8; 2]) -> u16) -> Result<String> {
    let units: Vec<u16> = body
        .as_chunks::<2>()
        .0
        .iter()
        .map(|chunk| decode([chunk[0], chunk[1]]))
        .collect();

    Ok(String::from_utf16(&units)?)
}

/// Locates the application executable inside the Flutter build output.
///
/// Flutter names the executable after `BINARY_NAME` in
/// `windows/CMakeLists.txt`, which `flutter create` sets from the project name.
/// The two drift apart when a project is renamed, so the CMake value is tried
/// first, then the pubspec name, and finally a unique executable in the build
/// directory — which also covers names neither file mentions.
pub(super) fn find_main_executable(
    build_dir: &Path,
    cmake_binary_name: Option<&str>,
    project_name: &str,
) -> Result<PathBuf> {
    if !build_dir.is_dir() {
        return Err(PlatformError::BuildFailed {
            platform: "Windows".to_string(),
            reason: format!(
                "the Flutter build output {} does not exist; run without \
                 `--use-existing-flutter-build` after `flutter build windows`",
                build_dir.display()
            ),
        }
        .into());
    }

    let mut candidates: Vec<&str> = Vec::new();
    if let Some(name) = cmake_binary_name {
        candidates.push(name);
    }
    if !candidates.contains(&project_name) {
        candidates.push(project_name);
    }

    for candidate in &candidates {
        let path = build_dir.join(format!("{candidate}.exe"));
        if path.is_file() {
            tracing::debug!(
                executable = candidate,
                path = %path.display(),
                "Located the application executable"
            );
            return Ok(path);
        }
    }

    let mut executables = Vec::new();
    for entry in fs::read_dir(build_dir)? {
        let path = entry?.path();
        if path.is_file() && path.extension().is_some_and(|extension| extension == "exe") {
            executables.push(path);
        }
    }
    executables.sort();

    match executables.len() {
        1 => {
            let executable = executables.remove(0);
            tracing::debug!(
                path = %executable.display(),
                "Located the only executable in the build output"
            );
            Ok(executable)
        }
        0 => Err(BuildError::ExecutableNotFound {
            directory: build_dir.to_path_buf(),
        }
        .into()),
        _ => Err(PlatformError::BuildFailed {
            platform: "Windows".to_string(),
            reason: format!(
                "several executables found in {}: {}; set the name in \
                 windows/CMakeLists.txt (`set(BINARY_NAME …)`) to select one",
                build_dir.display(),
                executables
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
        .into()),
    }
}

/// Applies [`parse_binary_name`] to a `CMakeLists.txt` if it can be read.
pub(super) fn binary_name_from_cmake(path: &Path) -> Option<String> {
    let contents = read_text_file(path).ok()?;

    parse_binary_name(&contents)
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::TempDir;

    #[test]
    fn binary_name_is_read_from_the_flutter_template_line() {
        let contents = "# Project-level configuration.\n\
                        cmake_minimum_required(VERSION 3.14)\n\
                        project(my_app LANGUAGES CXX)\n\
                        set(BINARY_NAME \"my_app\")\n";

        assert_eq!(parse_binary_name(contents), Some("my_app".to_string()));
    }

    #[test]
    fn binary_name_tolerates_spacing_and_is_ignored_when_absent() {
        assert_eq!(
            parse_binary_name("set( BINARY_NAME   \"renamed\" )"),
            Some("renamed".to_string())
        );
        assert_eq!(
            parse_binary_name("set(BINARY_NAME \"other\")"),
            Some("other".into())
        );
        assert_eq!(parse_binary_name("project(my_app LANGUAGES CXX)"), None);
        assert_eq!(parse_binary_name("set(BINARY_NAME \"\")"), None);
        assert_eq!(parse_binary_name("set(BINARY_NAME \"a/b\")"), None);
        assert_eq!(
            parse_binary_name("# set(BINARY_NAME \"commented\")"),
            None,
            "a commented-out line must not win"
        );
    }

    /// The value is whatever the user put in `Runner.rc`: it may not end in a
    /// full stop, which is what made the previous suffix-based parsing leak the
    /// trailing `" "\0"` into the installer.
    #[test]
    fn legal_copyright_is_read_without_a_suffix_heuristic() {
        let flutter_default = "            VALUE \"LegalCopyright\", \"Copyright (C) 2025 \
                               com.example. All rights reserved.\" \"\\0\"\n";
        assert_eq!(
            parse_legal_copyright(flutter_default),
            Some("Copyright (C) 2025 com.example. All rights reserved.".to_string())
        );

        let no_full_stop =
            "      VALUE \"LegalCopyright\", \"Copyright 2025 Example Ltd\" \"\\0\"\n";
        assert_eq!(
            parse_legal_copyright(no_full_stop),
            Some("Copyright 2025 Example Ltd".to_string())
        );

        assert_eq!(
            parse_legal_copyright("VALUE \"LegalCopyright\", \"\" \"\\0\"\n"),
            None
        );
        assert_eq!(
            parse_legal_copyright("VALUE \"FileDescription\", \"x\"\n"),
            None
        );
        assert_eq!(parse_legal_copyright("nothing here"), None);
    }

    #[test]
    fn text_files_are_read_through_their_bom() {
        let temp = TempDir::new().unwrap();
        let utf8_bom = temp.path().join("utf8.rc");
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(b"VALUE \"LegalCopyright\", \"caf\xC3\xA9\"\n");
        fs::write(&utf8_bom, bytes).unwrap();

        let contents = read_text_file(&utf8_bom).unwrap();
        assert!(contents.starts_with("VALUE"), "{contents:?}");
        assert_eq!(parse_legal_copyright(&contents), Some("café".to_string()));

        // Visual Studio's resource editor writes UTF-16LE with a BOM.
        let utf16 = temp.path().join("utf16.rc");
        let mut bytes = vec![0xFF, 0xFE];
        for unit in "VALUE \"LegalCopyright\", \"公司\"\n".encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        fs::write(&utf16, bytes).unwrap();

        let contents = read_text_file(&utf16).unwrap();
        assert_eq!(parse_legal_copyright(&contents), Some("公司".to_string()));
    }

    #[test]
    fn the_executable_is_found_by_cmake_name_then_pubspec_name() {
        let temp = TempDir::new().unwrap();
        let build = temp.path().join("Release");
        fs::create_dir_all(&build).unwrap();
        fs::write(build.join("renamed.exe"), b"exe").unwrap();

        // Flutter uses BINARY_NAME, so a renamed project is found even though
        // pubspec.yaml still says otherwise.
        assert_eq!(
            find_main_executable(&build, Some("renamed"), "my_app").unwrap(),
            build.join("renamed.exe")
        );
        assert_eq!(
            find_main_executable(&build, None, "renamed").unwrap(),
            build.join("renamed.exe")
        );
    }

    #[test]
    fn a_single_executable_covers_names_neither_file_mentions() {
        let temp = TempDir::new().unwrap();
        let build = temp.path().join("Release");
        fs::create_dir_all(&build).unwrap();
        fs::write(build.join("something.exe"), b"exe").unwrap();
        fs::write(build.join("flutter_windows.dll"), b"dll").unwrap();

        assert_eq!(
            find_main_executable(&build, None, "my_app").unwrap(),
            build.join("something.exe")
        );
    }

    #[test]
    fn a_missing_or_ambiguous_build_output_is_reported() {
        let temp = TempDir::new().unwrap();

        let missing = find_main_executable(&temp.path().join("Release"), None, "my_app")
            .unwrap_err()
            .to_string();
        assert!(missing.contains("does not exist"), "{missing}");

        let build = temp.path().join("Release");
        fs::create_dir_all(&build).unwrap();
        let empty = find_main_executable(&build, None, "my_app")
            .unwrap_err()
            .to_string();
        assert!(empty.contains("No application executable"), "{empty}");

        fs::write(build.join("a.exe"), b"exe").unwrap();
        fs::write(build.join("b.exe"), b"exe").unwrap();
        let ambiguous = find_main_executable(&build, None, "my_app")
            .unwrap_err()
            .to_string();
        assert!(ambiguous.contains("BINARY_NAME"), "{ambiguous}");
    }
}
