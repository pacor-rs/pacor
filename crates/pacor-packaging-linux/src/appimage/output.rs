//! Normalisation of linuxdeploy's output into the expected artifact path.
//!
//! linuxdeploy names the file it produces after the desktop entry, which rarely
//! matches the artifact name `pacor` wants. This module finds what was written,
//! renames it, and makes it executable.

use std::path::{Path, PathBuf};
use std::{fs, io};

use anyhow::{Context, Result};

/// Normalise a file name for prefix comparison.
///
/// linuxdeploy sanitises the desktop entry's `Name` when it derives the output
/// file name, so `My App` becomes `My_App`. Comparing normalised names means a
/// successful build is not mistaken for a missing artifact.
fn normalize_for_matching(name: &str) -> String {
    name.chars()
        .map(|character| match character {
            ' ' | '-' => '_',
            other => other.to_ascii_lowercase(),
        })
        .collect()
}

/// Move the produced AppImage to `expected_path` and make it executable.
///
/// `name_prefixes` lists the names linuxdeploy may have used (display name and
/// executable name). The freshest file wins: linuxdeploy honours `OUTPUT`, so
/// `expected_path` is normally this run's artifact, and an older `*.AppImage`
/// left in the output directory must never replace it. When linuxdeploy ignored
/// `OUTPUT` and wrote a differently named file, that file is newer than
/// `expected_path` and is moved into place instead.
pub(super) fn finalize_appimage(
    output_dir: &Path,
    expected_path: &Path,
    name_prefixes: &[&str],
) -> Result<PathBuf> {
    let expected_modified = modified_time(expected_path);
    let candidate = find_candidate(output_dir, expected_path, name_prefixes)?;

    let generated = match (&candidate, expected_modified) {
        (Some((candidate_modified, candidate)), Some(expected_modified))
            if candidate_modified > &expected_modified =>
        {
            Some(candidate)
        }
        (Some((_, candidate)), None) => Some(candidate),
        _ => None,
    };

    if let Some(generated) = generated {
        // `fs::rename` will not overwrite on every platform.
        if expected_path.exists() {
            fs::remove_file(expected_path)
                .with_context(|| format!("failed to replace {}", expected_path.display()))?;
        }

        fs::rename(generated, expected_path).with_context(|| {
            format!(
                "failed to rename {} to {}",
                generated.display(),
                expected_path.display()
            )
        })?;

        debug!(
            from = %generated.display(),
            to = %expected_path.display(),
            "AppImage renamed to the expected artifact name"
        );
    } else if expected_modified.is_none() {
        return Err(anyhow::anyhow!(
            "linuxdeploy did not produce an AppImage in {} (looked for names starting with: \
             {}, or {})",
            output_dir.display(),
            name_prefixes.join(", "),
            expected_path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        ));
    } else {
        trace!(path = %expected_path.display(), "linuxdeploy used the expected name");
    }

    set_executable(expected_path)?;
    Ok(expected_path.to_path_buf())
}

/// Modification time of `path`, or `None` when it does not exist.
fn modified_time(path: &Path) -> Option<std::time::SystemTime> {
    path.metadata()
        .ok()
        .and_then(|metadata| metadata.modified().ok())
}

/// Pick the freshest AppImage other than `expected_path`, if any.
///
/// Files matching a known prefix are considered, excluding `expected_path`
/// itself; the most recently modified one wins, with the file name as a
/// deterministic tie-breaker. The modification time is returned with it, so
/// the caller can compare it against `expected_path`.
fn find_candidate(
    output_dir: &Path,
    expected_path: &Path,
    name_prefixes: &[&str],
) -> Result<Option<(std::time::SystemTime, PathBuf)>> {
    let entries = match fs::read_dir(output_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("failed to list the output directory"),
    };

    let mut candidates: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path == expected_path || !path.is_file() {
            continue;
        }
        if path
            .extension()
            .is_none_or(|extension| extension != "AppImage")
        {
            continue;
        }

        let matches_prefix = path.file_name().is_some_and(|name| {
            let name = normalize_for_matching(&name.to_string_lossy());
            name_prefixes
                .iter()
                .any(|prefix| name.starts_with(&normalize_for_matching(prefix)))
        });
        if !matches_prefix {
            continue;
        }

        let modified = path
            .metadata()
            .and_then(|metadata| metadata.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        candidates.push((modified, path));
    }

    candidates.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| left.1.file_name().cmp(&right.1.file_name()))
    });

    Ok(candidates.into_iter().next())
}

/// Mark `path` executable (no-op where unix permission bits do not exist).
pub(super) fn set_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions)?;
    }

    #[cfg(not(unix))]
    let _ = path;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};
    use tempfile::TempDir;

    const PREFIXES: [&str; 2] = ["Demo", "demo"];

    /// Create a file and force its modification time, so ordering is testable.
    fn write_at(path: &Path, contents: &str, age: Duration) {
        fs::write(path, contents).unwrap();
        let modified = SystemTime::now() - age;
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(modified)
            .unwrap();
    }

    #[test]
    fn uses_a_differently_named_output_and_renames_it() {
        let temp = TempDir::new().unwrap();
        let expected = temp.path().join("Demo-1.0.0-x64.AppImage");
        write_at(
            &temp.path().join("Demo-x86_64.AppImage"),
            "fresh",
            Duration::ZERO,
        );

        let result = finalize_appimage(temp.path(), &expected, &PREFIXES).unwrap();

        assert_eq!(result, expected);
        assert_eq!(fs::read_to_string(&expected).unwrap(), "fresh");
        assert!(
            !temp.path().join("Demo-x86_64.AppImage").exists(),
            "the intermediate file must be moved, not copied"
        );
    }

    #[test]
    fn keeps_the_expected_name_when_linuxdeploy_used_it() {
        let temp = TempDir::new().unwrap();
        let expected = temp.path().join("demo-1.0.0-x64.AppImage");
        write_at(&expected, "direct", Duration::ZERO);

        let result = finalize_appimage(temp.path(), &expected, &PREFIXES).unwrap();

        assert_eq!(fs::read_to_string(result).unwrap(), "direct");
    }

    #[test]
    fn prefers_the_newest_candidate_over_a_stale_artifact() {
        let temp = TempDir::new().unwrap();
        let expected = temp.path().join("Demo-1.0.0-x64.AppImage");
        // A leftover from a previous build, plus the file just produced.
        write_at(
            &temp.path().join("Demo-0.9.0-x86_64.AppImage"),
            "stale",
            Duration::from_secs(600),
        );
        write_at(
            &temp.path().join("Demo-x86_64.AppImage"),
            "fresh",
            Duration::ZERO,
        );

        finalize_appimage(temp.path(), &expected, &PREFIXES).unwrap();

        assert_eq!(fs::read_to_string(&expected).unwrap(), "fresh");
    }

    #[test]
    fn replaces_a_stale_file_that_already_has_the_expected_name() {
        let temp = TempDir::new().unwrap();
        let expected = temp.path().join("Demo-1.0.0-x64.AppImage");
        // Stale artifact carrying the final name, plus a fresh differently-named one.
        write_at(&expected, "stale", Duration::from_secs(600));
        write_at(
            &temp.path().join("Demo-x86_64.AppImage"),
            "fresh",
            Duration::ZERO,
        );

        finalize_appimage(temp.path(), &expected, &PREFIXES).unwrap();

        assert_eq!(
            fs::read_to_string(&expected).unwrap(),
            "fresh",
            "a fresh build must not be shadowed by a stale artifact"
        );
    }

    #[test]
    fn a_fresh_expected_artifact_is_not_replaced_by_a_stale_candidate() {
        let temp = TempDir::new().unwrap();
        let expected = temp.path().join("Demo-1.0.0-x64.AppImage");
        // linuxdeploy honoured `OUTPUT` and wrote `expected`; the 0.9.0 file is
        // left over from the previous release. Renaming the stale file over the
        // new one published the old binary under the new version and checksum.
        write_at(&expected, "fresh", Duration::ZERO);
        write_at(
            &temp.path().join("Demo-0.9.0-x86_64.AppImage"),
            "stale",
            Duration::from_secs(600),
        );

        let result = finalize_appimage(temp.path(), &expected, &PREFIXES).unwrap();

        assert_eq!(fs::read_to_string(&result).unwrap(), "fresh");
        assert!(
            temp.path().join("Demo-0.9.0-x86_64.AppImage").is_file(),
            "an artifact of an earlier release must be left alone, not moved or deleted"
        );
    }

    #[test]
    fn ignores_files_with_other_names_or_extensions() {
        let temp = TempDir::new().unwrap();
        let expected = temp.path().join("Demo-1.0.0-x64.AppImage");
        write_at(
            &temp.path().join("OtherApp.AppImage"),
            "other",
            Duration::ZERO,
        );
        write_at(&temp.path().join("Demo.tar.gz"), "archive", Duration::ZERO);

        let error = finalize_appimage(temp.path(), &expected, &PREFIXES).unwrap_err();

        assert!(error.to_string().contains("did not produce an AppImage"));
    }

    #[test]
    fn missing_output_directory_is_reported_as_no_output() {
        let temp = TempDir::new().unwrap();
        let expected = temp.path().join("out/Demo.AppImage");

        let error =
            finalize_appimage(&temp.path().join("absent"), &expected, &PREFIXES).unwrap_err();

        assert!(error.to_string().contains("did not produce an AppImage"));
    }

    #[cfg(unix)]
    #[test]
    fn the_final_artifact_is_executable() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new().unwrap();
        let expected = temp.path().join("Demo-1.0.0-x64.AppImage");
        write_at(&temp.path().join("Demo.AppImage"), "bin", Duration::ZERO);

        let result = finalize_appimage(temp.path(), &expected, &PREFIXES).unwrap();

        let mode = fs::metadata(&result).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111, "AppImage must be executable by all");
    }

    #[test]
    fn a_sanitised_display_name_is_still_recognised() {
        // appimagetool turns `Name=My App` into `My_App-x86_64.AppImage`.
        let temp = TempDir::new().unwrap();
        let output_dir = temp.path();
        let expected = output_dir.join("my_app-1.0.0-x64.AppImage");
        fs::write(output_dir.join("My_App-x86_64.AppImage"), "appimage").unwrap();

        let result = finalize_appimage(output_dir, &expected, &["My App", "my_app"]).unwrap();

        assert_eq!(result, expected);
        assert!(expected.exists());
        assert!(!output_dir.join("My_App-x86_64.AppImage").exists());
    }

    #[test]
    fn name_matching_ignores_case_and_separators() {
        assert_eq!(normalize_for_matching("My App"), "my_app");
        assert_eq!(normalize_for_matching("My_App"), "my_app");
        assert_eq!(normalize_for_matching("my-app"), "my_app");
        assert_eq!(normalize_for_matching("MyApp"), "myapp");
    }

    #[test]
    fn an_unrelated_appimage_is_not_claimed() {
        let temp = TempDir::new().unwrap();
        let output_dir = temp.path();
        let expected = output_dir.join("my_app-1.0.0-x64.AppImage");
        fs::write(output_dir.join("SomethingElse-x86_64.AppImage"), "other").unwrap();

        let error = finalize_appimage(output_dir, &expected, &["My App", "my_app"])
            .expect_err("an unrelated artifact must not be adopted");

        // The message must name the file pacor wanted, to make `OUTPUT` obvious.
        assert!(
            format!("{error:#}").contains("my_app-1.0.0-x64.AppImage"),
            "{error:#}"
        );
    }
}
