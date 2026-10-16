//! Shared helpers used by packaging backends.
//!
//! These used to live behind five traits, a composite trait, a zero-sized
//! `CommonBuilderImpl` and an `impl_common_builders!` delegation macro. None of
//! that carried state, so they are plain functions again.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::error::Result;

use crate::core::arch::Arch;
use crate::error::{BuildError, ConfigurationError, Error, PlatformError, ValidationError};
use crate::platforms::BuildArtifact;
use crate::utils::command::{CommandOutputMode, execute_flutter_build_command, find_command_path};
use crate::utils::crypto::generate_sha1_checksum_file;

/// Highest value Windows accepts for one version component.
const VERSION_COMPONENT_MAX: u32 = u16::MAX as u32;

/// Converts a pubspec version into the four-part numeric form Windows needs.
///
/// Both `VIProductVersion` (NSIS) and `ProductVersion` (MSI) reject semver
/// spellings, and Flutter's default version is `1.0.0+1`: the build number is a
/// separate field, so it becomes the fourth component (`1.0.0.1`). Pre-release
/// identifiers are dropped (`1.2.0-beta.1+7` becomes `1.2.0.7`) and components
/// are clamped, because Windows version fields are 16-bit.
pub fn windows_version(version: &str) -> String {
    /// Parses one dotted component, ignoring anything non-numeric.
    fn component(value: &str) -> Option<u32> {
        let digits: String = value.chars().take_while(char::is_ascii_digit).collect();
        digits.parse().ok()
    }

    let (base, build) = version.split_once('+').unwrap_or((version, ""));

    let mut components: Vec<u32> = base.split('.').map_while(component).take(3).collect();
    components.resize(3, 0);
    components.push(component(build).unwrap_or(0));

    components
        .iter()
        .map(|component| component.min(&VERSION_COMPONENT_MAX).to_string())
        .collect::<Vec<_>>()
        .join(".")
}

/// Namespace used by [`deterministic_uuid_v5`]: the RFC 4122 DNS namespace.
///
/// The generated ids only have to be stable and collision-free, not to
/// interoperate with any other producer.
const UUID_NAMESPACE: [u8; 16] = [
    0x6b, 0xa7, 0xb8, 0x10, 0x9d, 0xad, 0x11, 0xd1, 0x80, 0xb4, 0x00, 0xc0, 0x4f, 0xd4, 0x30, 0xc8,
];

/// Version-5 UUID (SHA-1 based) of `name`, as an upper-case braced string.
///
/// Deterministic identifiers matter for MSI: component GUIDs and the
/// `UpgradeCode` have to stay identical across builds, otherwise every release
/// looks like a different product. `name` is hashed as UTF-8, so callers must
/// pass a host-independent spelling (forward slashes, no absolute paths).
///
/// Reference: <https://www.rfc-editor.org/rfc/rfc4122#section-4.3>
pub fn deterministic_uuid_v5(name: &str) -> String {
    use sha1::{Digest, Sha1};

    let mut hasher = Sha1::new();
    hasher.update(UUID_NAMESPACE);
    hasher.update(name.as_bytes());
    let digest = hasher.finalize();

    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50; // version 5
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // RFC 4122 variant

    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!(
        "{{{}-{}-{}-{}-{}}}",
        hex[0..8].to_ascii_uppercase(),
        hex[8..12].to_ascii_uppercase(),
        hex[12..16].to_ascii_uppercase(),
        hex[16..20].to_ascii_uppercase(),
        hex[20..32].to_ascii_uppercase(),
    )
}

/// Standard artifact name: [`artifact_base_name`] plus an optional `-{suffix}`
/// and the extension.
pub fn artifact_name(
    project_name: &str,
    version: &str,
    target: &Arch,
    suffix: &str,
    extension: &str,
) -> String {
    let base = artifact_base_name(project_name, version, target);
    if suffix.is_empty() {
        format!("{base}.{extension}")
    } else {
        format!("{base}-{suffix}.{extension}")
    }
}

/// Base (extension-less) artifact name: `{project}-{version}-{target}`.
pub fn artifact_base_name(project_name: &str, version: &str, target: &Arch) -> String {
    format!("{project_name}-{version}-{target}")
}

/// Generate the SHA1 checksum file next to `artifact_path`.
pub fn create_artifact_with_checksum(artifact_path: PathBuf) -> Result<BuildArtifact> {
    let checksum_path = generate_sha1_checksum_file(&artifact_path).map_err(|error| {
        Error::Build(BuildError::ArtifactGeneration {
            artifact: artifact_path.display().to_string(),
            reason: format!("failed to generate checksum: {error}"),
        })
    })?;

    Ok(BuildArtifact::new(artifact_path, checksum_path))
}

/// Verify that every path exists.
pub fn validate_required_files<P, I>(paths: I) -> Result<()>
where
    P: AsRef<Path>,
    I: IntoIterator<Item = P>,
{
    for path in paths {
        let path = path.as_ref();
        if !path.exists() {
            return Err(Error::Validation(ValidationError::FileValidation {
                file: path.to_path_buf(),
                reason: "required file does not exist".to_string(),
            }));
        }
    }
    Ok(())
}

/// Locate a tool on `PATH`, reporting a platform-specific error otherwise.
pub fn find_tool(tool: &str, platform: &str) -> Result<PathBuf> {
    match find_command_path(tool) {
        Ok(Some(path)) => Ok(path),
        _ => Err(Error::Platform(PlatformError::ToolNotFound {
            tool: tool.to_string(),
            platform: platform.to_string(),
        })),
    }
}

/// Check that a directory exists and is writable.
pub fn validate_directory_permissions(directory: &Path, operation: &str) -> Result<()> {
    if !directory.exists() {
        return Err(Error::Validation(ValidationError::FileValidation {
            file: directory.to_path_buf(),
            reason: "directory does not exist".to_string(),
        }));
    }

    let probe = directory.join(".pacor_permission_test");
    match std::fs::write(&probe, b"test") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            Ok(())
        }
        Err(_) => Err(Error::Validation(ValidationError::PermissionDenied {
            path: directory.to_path_buf(),
            operation: operation.to_string(),
        })),
    }
}

/// Create `directory` if needed and check that it is writable.
pub fn prepare_output_directory(directory: &Path) -> Result<()> {
    if !directory.exists() {
        std::fs::create_dir_all(directory)?;
    }
    validate_directory_permissions(directory, "build output")
}

/// Remove the contents of a directory, keeping the directory itself.
pub fn clean_directory_contents(directory: &Path) -> Result<()> {
    if !directory.exists() {
        return Ok(());
    }

    if !directory.is_dir() {
        return Err(Error::Build(BuildError::Cleanup {
            reason: format!("{} is not a directory", directory.display()),
        }));
    }

    for entry in std::fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            std::fs::remove_dir_all(&path)?;
        } else {
            std::fs::remove_file(&path)?;
        }
    }

    Ok(())
}

/// Common pre-build validation: required tools, required files and a writable
/// output directory.
pub fn validate_build_environment(
    platform: &str,
    required_tools: &[&str],
    required_files: &[&Path],
    output_directory: &Path,
) -> Result<()> {
    for tool in required_tools {
        find_tool(tool, platform)?;
    }

    validate_required_files(required_files.iter().copied())?;
    prepare_output_directory(output_directory)
}

/// Run `flutter build` with the given arguments.
pub fn flutter_build(
    args: &[OsString],
    project_root: &Path,
    command_output: &CommandOutputMode,
    platform: &str,
) -> Result<()> {
    execute_flutter_build_command(args, project_root, command_output).map_err(|error| {
        Error::Build(BuildError::FlutterBuild {
            output: error.to_string(),
            platform: platform.to_string(),
        })
    })
}

/// Require that an optional configuration value is present.
pub fn require_config<T>(field: &str, value: &Option<T>) -> Result<()> {
    if value.is_none() {
        return Err(Error::Configuration(ConfigurationError::MissingRequired {
            field: field.to_string(),
        }));
    }
    Ok(())
}

/// Require that a configured file path exists.
pub fn require_existing_path(field: &str, path: &Option<PathBuf>) -> Result<()> {
    match path {
        None => Err(Error::Configuration(ConfigurationError::MissingRequired {
            field: field.to_string(),
        })),
        Some(path) if !path.exists() => {
            Err(Error::Configuration(ConfigurationError::ValidationFailed {
                field: field.to_string(),
                reason: format!("{} does not exist", path.display()),
            }))
        }
        Some(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `VIProductVersion` and MSI's `ProductVersion` both reject anything but
    /// four numeric components, and the Flutter default version is `1.0.0+1`.
    #[test]
    fn the_windows_version_is_always_four_numeric_components() {
        for (version, expected) in [
            ("1.0.0+1", "1.0.0.1"),
            ("1.0.0", "1.0.0.0"),
            ("2.3.4+56", "2.3.4.56"),
            ("1.0.0-beta.1+7", "1.0.0.7"),
            ("1.2", "1.2.0.0"),
            ("1", "1.0.0.0"),
            ("", "0.0.0.0"),
            ("1.0.0+", "1.0.0.0"),
        ] {
            let rendered = windows_version(version);

            assert_eq!(rendered, expected, "version {version:?}");
            assert_eq!(
                rendered.split('.').count(),
                4,
                "version {version:?} produced {rendered}"
            );
            assert!(
                rendered
                    .split('.')
                    .all(|part| part.parse::<u32>().is_ok_and(|n| n <= u32::from(u16::MAX))),
                "version {version:?} produced out-of-range components: {rendered}"
            );
        }
    }

    #[test]
    fn oversized_version_components_are_clamped() {
        assert_eq!(windows_version("70000.0.0+80000"), "65535.0.0.65535");
    }

    /// The RFC 4122 example: UUIDv5 of "python.org" in the DNS namespace.
    #[test]
    fn uuid_v5_matches_the_rfc_example() {
        assert_eq!(
            deterministic_uuid_v5("python.org"),
            "{886313E1-3B8A-5372-9B90-0C9AEE199E5D}"
        );
    }

    /// The whole point of the helper: the same input always yields the same id,
    /// and different inputs practically never collide.
    #[test]
    fn uuid_v5_is_stable_and_input_dependent() {
        assert_eq!(
            deterministic_uuid_v5("my_app/data/app.so"),
            deterministic_uuid_v5("my_app/data/app.so")
        );
        assert_ne!(
            deterministic_uuid_v5("my_app/data/app.so"),
            deterministic_uuid_v5("my_app/data/app.dll")
        );
        // The version and variant bits are the ones RFC 4122 prescribes.
        let id = deterministic_uuid_v5("x");
        assert!(id.starts_with('{') && id.ends_with('}'), "{id}");
        assert_eq!(id.as_bytes()[15], b'5', "version nibble: {id}");
        assert!(
            matches!(id.as_bytes()[20], b'8' | b'9' | b'A' | b'B'),
            "variant nibble: {id}"
        );
    }
}
