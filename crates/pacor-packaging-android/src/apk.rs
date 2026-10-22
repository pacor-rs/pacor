//! Locating Flutter's split APKs and naming pacor's artifacts.
//!
//! Flutter's Gradle plugin copies the split APKs into one directory and names
//! them `app<-abi>?<-flavor-name>?-<build-mode>.apk`. The ABI and the flavor are
//! therefore part of the file name, which is why packaging cannot simply assume
//! `app-<abi>-release.apk`.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Result;

use pacor_core::{
    core::abi::TargetAbi,
    error::{BuildError, Error},
};

/// File name Flutter uses for `abi` when the project defines no flavor.
pub(crate) fn plain_file_name(abi: &TargetAbi) -> String {
    format!("app-{abi}-release.apk")
}

/// File name Flutter uses for `abi` when the project defines `flavor`.
pub(crate) fn flavored_file_name(abi: &TargetAbi, flavor: &str) -> String {
    format!("app-{abi}-{flavor}-release.apk")
}

/// Name of the artifact pacor writes to the output directory.
pub(crate) fn artifact_file_name(project_name: &str, version: &str, abi: &TargetAbi) -> String {
    format!("{project_name}-{version}-{abi}.apk")
}

/// Finds the APK Flutter produced for `abi`.
///
/// The file name carries the flavor the build ran with, so a configured flavor decides which
/// file is the product. An `app-<abi>-release.apk` next to a flavored build is left over from
/// an earlier unflavored one, and a file carrying a *different* flavor is left over from an
/// earlier flavored one; either would be packaged, signed and shipped as this build's release
/// artifact. The flavor is compared case-insensitively because AGP lower-cases it in some
/// versions, and without a configured flavor a single remaining candidate is still the best
/// guess — the `--use-existing-flutter-build` case.
pub(crate) fn find_apk(directory: &Path, abi: &TargetAbi, flavor: Option<&str>) -> Result<PathBuf> {
    let expected = match flavor {
        Some(flavor) => flavored_file_name(abi, flavor),
        None => plain_file_name(abi),
    };
    let exact = directory.join(&expected);
    if exact.is_file() {
        return Ok(exact);
    }

    let candidates: Vec<PathBuf> = match flavor {
        None => matching_files(directory, abi),
        Some(flavor) => matching_files(directory, abi)
            .into_iter()
            .filter(|path| {
                flavor_in_file_name(path, abi).is_some_and(|name| name.eq_ignore_ascii_case(flavor))
            })
            .collect(),
    };

    match candidates.as_slice() {
        [] => Err(missing_apk_error(directory, abi, flavor, &expected)),
        [only] => {
            debug!(
                path = %only.display(),
                "Found the APK for {abi} under a name pacor did not expect"
            );
            Ok(only.clone())
        }
        several => Err(Error::Build(BuildError::ArtifactGeneration {
            artifact: directory.join(&expected).display().to_string(),
            reason: format!(
                "several APKs match {abi}: {}; set `platforms.android.flavor` to pick one",
                display_paths(several)
            ),
        })
        .into()),
    }
}

/// The flavor part of an `app-<abi>-<flavor>-release.apk` file name.
fn flavor_in_file_name(path: &Path, abi: &TargetAbi) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let rest = name.strip_prefix(&format!("app-{abi}-"))?;
    rest.strip_suffix("-release.apk").map(ToString::to_string)
}

/// `app-<abi>-*-release.apk` files in `directory`, in a stable order.
fn matching_files(directory: &Path, abi: &TargetAbi) -> Vec<PathBuf> {
    let prefix = format!("app-{abi}-");

    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };

    let mut matches: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(&prefix) && name.ends_with("-release.apk"))
        })
        .collect();

    matches.sort();
    matches
}

/// Error for "Flutter produced nothing we can package", listing what is there.
fn missing_apk_error(
    directory: &Path,
    abi: &TargetAbi,
    flavor: Option<&str>,
    expected: &str,
) -> anyhow::Error {
    let reason = match flavor {
        Some(flavor) => format!(
            "Flutter did not produce an APK for {abi} with flavor `{flavor}`; expected \
             `{expected}`. {} A file above carrying another flavor is left over from an earlier \
             build, or `platforms.android.flavor` does not match the Gradle flavor",
            describe_directory(directory)
        ),
        None => format!(
            "Flutter did not produce an APK for {abi}; expected `{expected}`. {} If the project \
             defines a Gradle flavor, set `platforms.android.flavor`",
            describe_directory(directory)
        ),
    };

    Error::Build(BuildError::ArtifactGeneration {
        artifact: directory.join(expected).display().to_string(),
        reason,
    })
    .into()
}

/// What the APK directory actually contains, for the error message.
fn describe_directory(directory: &Path) -> String {
    let Ok(entries) = fs::read_dir(directory) else {
        return format!("{} does not exist or is not readable", directory.display());
    };

    let mut names: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| entry.file_name().to_str().map(ToString::to_string))
        .collect();
    names.sort();

    if names.is_empty() {
        format!("{} contains no files.", directory.display())
    } else {
        format!("{} contains: {}", directory.display(), display_list(&names))
    }
}

/// Renders a list for an error message.
fn display_list<T: std::fmt::Display>(items: &[T]) -> String {
    items
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Renders APK paths as file names for an error message.
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

    fn apk_dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn touch(directory: &Path, name: &str) {
        fs::write(directory.join(name), b"apk").unwrap();
    }

    #[test]
    fn file_names_follow_flutter_conventions() {
        assert_eq!(
            plain_file_name(&TargetAbi::Arm64),
            "app-arm64-v8a-release.apk"
        );
        assert_eq!(
            flavored_file_name(&TargetAbi::Arm64, "prod"),
            "app-arm64-v8a-prod-release.apk"
        );
        assert_eq!(
            artifact_file_name("my_app", "1.0.0+1", &TargetAbi::Arm64),
            "my_app-1.0.0+1-arm64-v8a.apk"
        );
    }

    #[test]
    fn the_flavored_name_wins_when_a_flavor_is_configured() {
        let directory = apk_dir();
        touch(directory.path(), "app-arm64-v8a-release.apk");
        touch(directory.path(), "app-arm64-v8a-prod-release.apk");

        assert_eq!(
            find_apk(directory.path(), &TargetAbi::Arm64, Some("prod")).unwrap(),
            directory.path().join("app-arm64-v8a-prod-release.apk")
        );
    }

    /// `flutter build apk --flavor prod` cannot have written the plain name: it is left over
    /// from an earlier unflavored build, and packaging it ships the wrong (possibly
    /// debug-signed) APK as this build's release artifact.
    #[test]
    fn a_plain_leftover_is_not_packaged_as_a_flavored_build() {
        let directory = apk_dir();
        touch(directory.path(), "app-arm64-v8a-release.apk");

        let error = find_apk(directory.path(), &TargetAbi::Arm64, Some("prod"))
            .unwrap_err()
            .to_string();

        assert!(error.contains("with flavor `prod`"), "{error}");
        assert!(error.contains("app-arm64-v8a-prod-release.apk"), "{error}");
    }

    #[test]
    fn an_apk_of_another_flavor_is_not_accepted() {
        let directory = apk_dir();
        touch(directory.path(), "app-arm64-v8a-staging-release.apk");

        let error = find_apk(directory.path(), &TargetAbi::Arm64, Some("prod"))
            .unwrap_err()
            .to_string();

        assert!(
            error.contains("app-arm64-v8a-staging-release.apk"),
            "{error}"
        );
    }

    /// AGP lower-cases the flavor in the file name in some versions, which is why the
    /// comparison against the configured flavor ignores case.
    ///
    /// The parsing itself is asserted here rather than through `find_apk`: a case-insensitive
    /// filesystem answers the exact-name probe with the differently cased file anyway, so the
    /// fallback path cannot be observed through the file system on every host.
    #[test]
    fn the_flavor_is_read_out_of_the_file_name() {
        let path = |name: &str| Path::new("/tmp").join(name);

        assert_eq!(
            flavor_in_file_name(&path("app-arm64-v8a-prod-release.apk"), &TargetAbi::Arm64),
            Some("prod".to_string())
        );
        assert_eq!(
            flavor_in_file_name(&path("app-arm64-v8a-release.apk"), &TargetAbi::Arm64),
            None,
            "the plain name carries no flavor"
        );
        assert_eq!(
            flavor_in_file_name(&path("app-armeabi-v7a-prod-release.apk"), &TargetAbi::Arm64),
            None,
            "another ABI is not a candidate at all"
        );
    }

    #[test]
    fn the_flavored_name_is_found() {
        let directory = apk_dir();
        touch(directory.path(), "app-arm64-v8a-prod-release.apk");

        assert_eq!(
            find_apk(directory.path(), &TargetAbi::Arm64, Some("prod")).unwrap(),
            directory.path().join("app-arm64-v8a-prod-release.apk")
        );
    }

    /// The flavor is part of the name Flutter writes, but a project may build a
    /// flavor pacor does not know about; that must still be packageable.
    #[test]
    fn a_single_unknown_candidate_is_accepted() {
        let directory = apk_dir();
        touch(directory.path(), "app-arm64-v8a-staging-release.apk");
        touch(directory.path(), "app-armeabi-v7a-staging-release.apk");

        assert_eq!(
            find_apk(directory.path(), &TargetAbi::Arm64, None).unwrap(),
            directory.path().join("app-arm64-v8a-staging-release.apk")
        );
    }

    #[test]
    fn several_candidates_are_rejected_with_their_names() {
        let directory = apk_dir();
        touch(directory.path(), "app-arm64-v8a-prod-release.apk");
        touch(directory.path(), "app-arm64-v8a-staging-release.apk");

        let error = find_apk(directory.path(), &TargetAbi::Arm64, None)
            .unwrap_err()
            .to_string();

        assert!(error.contains("several APKs match arm64-v8a"), "{error}");
        assert!(error.contains("app-arm64-v8a-prod-release.apk"), "{error}");
        assert!(
            error.contains("app-arm64-v8a-staging-release.apk"),
            "{error}"
        );
    }

    #[test]
    fn a_missing_apk_lists_what_the_directory_contains() {
        let directory = apk_dir();
        touch(directory.path(), "app-armeabi-v7a-release.apk");

        let error = find_apk(directory.path(), &TargetAbi::Arm64, Some("prod"))
            .unwrap_err()
            .to_string();

        assert!(
            error.contains("did not produce an APK for arm64-v8a"),
            "{error}"
        );
        assert!(
            error.contains("app-arm64-v8a-prod-release.apk"),
            "the flavor is part of the message: {error}"
        );
        assert!(
            error.contains("app-armeabi-v7a-release.apk"),
            "the directory listing is part of the message: {error}"
        );
    }

    #[test]
    fn a_missing_directory_is_reported_as_such() {
        let directory = apk_dir();
        let missing = directory.path().join("flutter-apk");

        let error = find_apk(&missing, &TargetAbi::Arm64, None)
            .unwrap_err()
            .to_string();

        assert!(
            error.contains("does not exist or is not readable"),
            "{error}"
        );
    }
}
