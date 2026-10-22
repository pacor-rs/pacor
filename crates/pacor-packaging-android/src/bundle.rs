//! Locating Flutter's Android App Bundle and naming the packaged artifact.
//!
//! An `.aab` is not one-per-ABI: Flutter/AGP write exactly one file, and unlike
//! split APKs (which all land flat in `flutter-apk/`) it sits in a directory that
//! carries the build mode, and with a product flavor also the flavor name:
//!
//! ```text
//! build/app/outputs/bundle/release/app-release.aab
//! build/app/outputs/bundle/foo_barRelease/app-foo_bar-release.aab
//! ```
//!
//! The AGP directory casing varies between versions, so — like Flutter's own
//! `findBundleFile` — the flavored lookup compares directory names
//! case-insensitively before falling back to picking the single sensible
//! candidate.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Result;

use pacor_core::error::{BuildError, Error};

/// Build mode part of the directory and file names.
///
/// Android always builds release (see [`crate::flutter`]), so this is a
/// constant rather than a configuration value.
pub(crate) const MODE: &str = "release";

/// File name Flutter uses when the project defines no flavor.
pub(crate) fn plain_file_name() -> String {
    format!("app-{MODE}.aab")
}

/// File name Flutter uses when the project defines `flavor`.
pub(crate) fn flavored_file_name(flavor: &str) -> String {
    format!("app-{flavor}-{MODE}.aab")
}

/// Name of the artifact pacor writes to the output directory.
///
/// A bundle holds every ABI, so unlike the APK name there is nothing to
/// distinguish beyond the app version.
pub(crate) fn artifact_file_name(project_name: &str, version: &str) -> String {
    format!("{project_name}-{version}.aab")
}

/// Finds the App Bundle Flutter produced under `root` (`…/outputs/bundle`).
pub(crate) fn find_bundle(root: &Path, flavor: Option<&str>) -> Result<PathBuf> {
    for candidate in exact_candidates(root, flavor) {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    let candidates = matching_files(root, flavor);
    match candidates.as_slice() {
        [] => Err(missing_bundle_error(root, flavor)),
        [only] => {
            debug!(
                path = %only.display(),
                "Found the App Bundle under an unexpected directory name"
            );
            Ok(only.clone())
        }
        several => Err(Error::Build(BuildError::ArtifactGeneration {
            artifact: root.display().to_string(),
            reason: format!(
                "several App Bundles match: {}; set `platforms.android.flavor` to pick one",
                display_paths(several)
            ),
        })
        .into()),
    }
}

/// The names Flutter uses, in the order Flutter itself looks them up.
fn exact_candidates(root: &Path, flavor: Option<&str>) -> Vec<PathBuf> {
    let Some(flavor) = flavor else {
        return vec![root.join(MODE).join(plain_file_name())];
    };

    // AGP names the variant directory `<flavor><Mode>`, lower-casing the flavor
    // in most versions; both spellings are worth trying before scanning.
    vec![
        root.join(variant_directory(flavor))
            .join(flavored_file_name(flavor)),
        root.join(variant_directory(&flavor.to_lowercase()))
            .join(flavored_file_name(flavor)),
    ]
}

/// AGP's variant directory for `flavor`: `prod` → `prodRelease`.
fn variant_directory(flavor: &str) -> String {
    format!("{flavor}{}", capitalize(MODE))
}

/// `release` → `Release`, the capitalisation Flutter's variant directory uses.
fn capitalize(value: &str) -> String {
    let mut characters = value.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
        None => String::new(),
    }
}

/// Bundles under a mode/flavor directory, using Flutter's own matching rules,
/// in a stable order.
fn matching_files(root: &Path, flavor: Option<&str>) -> Vec<PathBuf> {
    let mut matches: Vec<PathBuf> = all_bundles(root)
        .into_iter()
        .filter(|path| {
            let parent = path
                .parent()
                .and_then(Path::file_name)
                .map(|name| name.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().to_lowercase())
                .unwrap_or_default();

            match flavor {
                // Flutter accepts `<mode>/app-<mode>.aab` for a plain build, and only that
                // name: anything else below a `release` directory (a renamed leftover,
                // `app-release-unsigned.aab`) is not what the build produced.
                None => parent == MODE && name == plain_file_name(),
                // …and `<flavor><Mode>/app-<flavor>-<mode>.aab` for a flavored one.
                Some(flavor) => {
                    parent.contains(&format!("{flavor}{MODE}").to_lowercase())
                        && name.ends_with(&format!("-{MODE}.aab"))
                }
            }
        })
        .collect();

    matches.sort();
    matches
}

/// Every `.aab` below `root`, recursively (AGP nests them one level down).
fn all_bundles(root: &Path) -> Vec<PathBuf> {
    let mut bundles = Vec::new();
    let mut pending = vec![root.to_path_buf()];

    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("aab"))
            {
                bundles.push(path);
            }
        }
    }

    bundles
}

/// Error for "Flutter produced nothing we can package", listing what is there.
///
/// A bundle that sits under a flavor directory pacor was not told about is
/// almost always a missing `flavor`, so the message says so instead of leaving
/// the user with a bare file name.
fn missing_bundle_error(root: &Path, flavor: Option<&str>) -> anyhow::Error {
    let found: Vec<String> = all_bundles(root)
        .iter()
        .map(|path| path.display().to_string())
        .collect();

    let hint = match flavor {
        Some(flavor) => format!(
            " (or {}/{})",
            variant_directory(flavor),
            flavored_file_name(flavor)
        ),
        None if !found.is_empty() => {
            "; if the project defines a Gradle flavor, set `platforms.android.flavor`".to_string()
        }
        None => String::new(),
    };

    let detail = if found.is_empty() {
        format!("{} contains no .aab file", root.display())
    } else {
        format!("{} contains: {}", root.display(), display_list(&found))
    };

    Error::Build(BuildError::ArtifactGeneration {
        artifact: root
            .join(MODE)
            .join(plain_file_name())
            .display()
            .to_string(),
        reason: format!(
            "Flutter did not produce an Android App Bundle; expected {}/{}{hint}. {detail}",
            MODE,
            plain_file_name()
        ),
    })
    .into()
}

/// Renders a list for an error message.
fn display_list(items: &[String]) -> String {
    items.join(", ")
}

/// Renders bundle paths as file names for an error message.
fn display_paths(paths: &[PathBuf]) -> String {
    display_list(
        &paths
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle_dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn stage(root: &Path, relative: &str) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"aab").unwrap();
    }

    #[test]
    fn file_names_follow_flutter_conventions() {
        assert_eq!(plain_file_name(), "app-release.aab");
        assert_eq!(flavored_file_name("prod"), "app-prod-release.aab");
        assert_eq!(
            artifact_file_name("my_app", "1.0.0+1"),
            "my_app-1.0.0+1.aab"
        );
    }

    #[test]
    fn the_plain_bundle_is_found_in_the_mode_directory() {
        let directory = bundle_dir();
        stage(directory.path(), "release/app-release.aab");

        assert_eq!(
            find_bundle(directory.path(), None).unwrap(),
            directory.path().join("release/app-release.aab")
        );
    }

    #[test]
    fn the_flavored_bundle_is_found_whatever_the_directory_casing() {
        for variant in ["prodRelease", "prodrelease"] {
            let directory = bundle_dir();
            stage(directory.path(), &format!("{variant}/app-prod-release.aab"));

            let found = find_bundle(directory.path(), Some("prod")).unwrap();

            assert!(
                found.is_file(),
                "variant directory {variant}: {}",
                found.display()
            );
            assert_eq!(
                found.file_name().unwrap(),
                "app-prod-release.aab",
                "variant directory {variant}"
            );
            // A case-insensitive filesystem accepts a differently spelled
            // directory, so only the directory *name* is compared here.
            assert_eq!(
                found
                    .parent()
                    .unwrap()
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_lowercase(),
                variant.to_lowercase()
            );
        }
    }

    /// A bundle under a flavor directory pacor was not told about must not be
    /// packaged silently: it is usually a leftover, and the error names the fix.
    #[test]
    fn a_flavor_pacor_was_not_told_about_is_reported_as_such() {
        let directory = bundle_dir();
        stage(directory.path(), "stagingRelease/app-staging-release.aab");

        let error = find_bundle(directory.path(), None).unwrap_err().to_string();

        assert!(
            error.contains("did not produce an Android App Bundle"),
            "{error}"
        );
        assert!(
            error.contains("app-staging-release.aab"),
            "the stray bundle is listed: {error}"
        );
        assert!(
            error.contains("platforms.android.flavor"),
            "and the fix is named: {error}"
        );
    }

    /// A differently named bundle below `release/` is not this build's product.
    #[test]
    fn a_differently_named_bundle_is_not_accepted_as_the_plain_one() {
        let directory = bundle_dir();
        stage(directory.path(), "release/app-release-unsigned.aab");

        let error = find_bundle(directory.path(), None).unwrap_err().to_string();

        assert!(
            error.contains("did not produce an Android App Bundle"),
            "{error}"
        );
        assert!(error.contains("app-release-unsigned.aab"), "{error}");
    }

    #[test]
    fn several_candidates_are_rejected_with_their_names() {
        let directory = bundle_dir();
        // Two directories matching `prod` whose exact bundle name is absent.
        stage(directory.path(), "prodRelease/app-other-release.aab");
        stage(directory.path(), "myprodRelease/app-another-release.aab");

        let error = find_bundle(directory.path(), Some("prod"))
            .unwrap_err()
            .to_string();

        assert!(error.contains("several App Bundles match"), "{error}");
        assert!(error.contains("app-other-release.aab"), "{error}");
        assert!(error.contains("app-another-release.aab"), "{error}");
    }

    #[test]
    fn a_missing_bundle_lists_the_bundles_that_are_there() {
        let directory = bundle_dir();
        stage(directory.path(), "release/app-release.aab");
        stage(directory.path(), "oldRelease/app-old-release.aab");

        let error = find_bundle(directory.path(), Some("prod"))
            .unwrap_err()
            .to_string();

        assert!(
            error.contains("did not produce an Android App Bundle"),
            "{error}"
        );
        assert!(
            error.contains("app-prod-release.aab"),
            "the flavor is part of the message: {error}"
        );
        assert!(
            error.contains("app-old-release.aab"),
            "the directory listing is part of the message: {error}"
        );
    }

    #[test]
    fn a_missing_directory_is_reported_as_such() {
        let directory = bundle_dir();
        let missing = directory.path().join("bundle");

        let error = find_bundle(&missing, Some("prod")).unwrap_err().to_string();

        assert!(error.contains("contains no .aab file"), "{error}");
        assert!(error.contains("app-prod-release.aab"), "{error}");
    }

    #[test]
    fn mode_names_are_capitalised_for_variant_directories() {
        assert_eq!(capitalize("release"), "Release");
        assert_eq!(capitalize(""), "");
        assert_eq!(variant_directory("prod"), "prodRelease");
    }
}
