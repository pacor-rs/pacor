// Copyright 2016-2019 Cargo-Bundle developers <https://github.com/burtonageo/cargo-bundle>
// Copyright 2019-2024 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Generation of the `control.tar.gz` members: `control`, maintainer scripts
//! and the `md5sums` list.

use std::fs;
use std::io::Write;
use std::path::Path;

use anyhow::Result;

use pacor_core::config::{AppConfig, DebianConfig};
use pacor_core::core::arch::Arch;
use pacor_core::utils::fs::ensure_directory_exists;

use super::archive::{calculate_installed_size_in_kb, copy_executable_file, write_md5sums};

/// Everything needed to render the control metadata.
pub(super) struct ControlContext<'a> {
    pub(super) app_config: &'a AppConfig,
    pub(super) package_name: &'a str,
    pub(super) version: &'a str,
    pub(super) description: &'a str,
    pub(super) architecture: &'a Arch,
    pub(super) maintainer: Option<&'a str>,
    pub(super) debian: &'a DebianConfig,
}

/// Debian architecture name for `arch`.
///
/// Debian has its own vocabulary (`amd64`, `arm64`, …) and `dpkg` rejects
/// anything else, so the `x64` spelling used elsewhere in pacor must not leak
/// into the control file.
///
/// Reference: <https://www.debian.org/doc/debian-policy/ch-controlfields.html#architecture>
fn debian_architecture(arch: &Arch) -> &'static str {
    match arch {
        Arch::X64 => "amd64",
        Arch::ARM64 => "arm64",
    }
}

/// Write `control`, the maintainer scripts and `md5sums` into `control_dir`.
///
/// The changelog is installed first: `Installed-Size` and `md5sums` both describe the data
/// tree, and writing the changelog after them described a tree that was missing it. It used to
/// be called by the builder once this function had returned, which left the package declaring
/// one file fewer than it carried — `dpkg -V` never verified the changelog, and the space apt
/// reserves for the installation was short by its compressed length.
pub(super) fn write_control_members(
    control_dir: &Path,
    data_dir: &Path,
    context: &ControlContext<'_>,
) -> Result<()> {
    ensure_directory_exists(control_dir)?;

    install_changelog(data_dir, context.package_name, context.debian)?;
    generate_control_file(control_dir, data_dir, context)?;
    install_maintainer_scripts(control_dir, context.debian)?;
    write_md5sums(control_dir, data_dir)?;

    Ok(())
}

/// Generate the `control` file containing package metadata for dpkg.
///
/// Format reference: <https://www.debian.org/doc/debian-policy/ch-controlfields.html>
fn generate_control_file(
    control_dir: &Path,
    data_dir: &Path,
    context: &ControlContext<'_>,
) -> Result<()> {
    let path = control_dir.join("control");
    debug!(path = %path.display(), "Generating package control file");
    let mut file = fs::File::create(&path)?;

    writeln!(file, "Package: {}", context.package_name)?;
    writeln!(file, "Version: {}", context.version)?;
    writeln!(
        file,
        "Architecture: {}",
        debian_architecture(context.architecture)
    )?;
    writeln!(
        file,
        "Installed-Size: {}",
        calculate_installed_size_in_kb(data_dir)?
    )?;
    writeln!(file, "Maintainer: {}", maintainer(context))?;
    writeln!(file, "Homepage: {}", context.app_config.url())?;
    writeln!(
        file,
        "Description: {}",
        description_field(context.description)
    )?;

    if let Some(section) = context.debian.section() {
        writeln!(file, "Section: {section}")?;
    }
    writeln!(
        file,
        "Priority: {}",
        context.debian.priority().unwrap_or("optional")
    )?;

    write_package_relationships(&mut file, context.debian)?;

    file.flush()?;
    Ok(())
}

/// Fold a description into the one field Debian policy allows.
///
/// The first line is the synopsis and follows `Description:`; every further line is a
/// continuation and must start with a space, an empty one written as a single ` .`.
/// Writing the value raw — which is what a `description` with newlines got — put the
/// second line at column 0, where dpkg reads it as a new field and refuses the package.
fn description_field(description: &str) -> String {
    let mut rendered = String::from(description.lines().next().unwrap_or_default().trim());

    for line in description.lines().skip(1) {
        rendered.push('\n');
        if line.trim().is_empty() {
            rendered.push_str(" .");
        } else {
            rendered.push(' ');
            rendered.push_str(line.trim_end());
        }
    }

    rendered
}

/// Resolve the `Maintainer` field: explicit config, then authors, then publisher.
///
/// Falls back to a placeholder because dpkg rejects an empty `Maintainer`.
fn maintainer(context: &ControlContext<'_>) -> String {
    if let Some(maintainer) = context.maintainer {
        return maintainer.to_string();
    }

    if let Some(authors) = context.app_config.authors()
        && !authors.is_empty()
    {
        let mut names: Vec<&str> = authors.iter().map(String::as_str).collect();
        names.sort_unstable();
        return names.join(", ");
    }

    context
        .app_config
        .publisher()
        .filter(|publisher| !publisher.is_empty())
        .unwrap_or("Unknown")
        .to_string()
}

/// Write the optional package relationship fields.
fn write_package_relationships(file: &mut fs::File, debian: &DebianConfig) -> Result<()> {
    let fields = [
        ("Depends", debian.depends()),
        ("Recommends", debian.recommends()),
        ("Provides", debian.provides()),
        ("Conflicts", debian.conflicts()),
        ("Replaces", debian.replaces()),
    ];

    for (name, values) in fields {
        if let Some(values) = values
            && !values.is_empty()
        {
            writeln!(file, "{name}: {}", values.join(", "))?;
        }
    }

    Ok(())
}

/// Install the configured maintainer scripts with executable permissions.
fn install_maintainer_scripts(control_dir: &Path, debian: &DebianConfig) -> Result<()> {
    let scripts = [
        ("preinst", debian.pre_install_script()),
        ("postinst", debian.post_install_script()),
        ("prerm", debian.pre_remove_script()),
        ("postrm", debian.post_remove_script()),
    ];

    for (name, source) in scripts {
        let Some(source) = source else { continue };

        if !source.exists() {
            return Err(anyhow::anyhow!(
                "maintainer script `{}` does not exist: {}",
                name,
                source.display()
            ));
        }

        copy_executable_file(source, &control_dir.join(name))?;
        debug!(script = name, "Maintainer script installed");
    }

    Ok(())
}

/// Copy the configured changelog into the package as `changelog.Debian.gz`.
fn install_changelog(data_root: &Path, package_name: &str, debian: &DebianConfig) -> Result<()> {
    let Some(changelog) = debian.changelog() else {
        return Ok(());
    };

    let doc_dir = doc_dir(data_root, package_name);
    ensure_directory_exists(&doc_dir)?;

    let target = doc_dir.join("changelog.Debian.gz");
    super::archive::write_gzipped_file(changelog, &target)?;

    debug!(path = %target.display(), "Changelog installed and compressed");
    Ok(())
}

/// `/usr/share/doc/<package>` inside the package data root.
fn doc_dir(data_root: &Path, package_name: &str) -> std::path::PathBuf {
    data_root
        .join("usr")
        .join("share")
        .join("doc")
        .join(package_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pacor_core::config::AppMetadata;
    use pacor_core::i18n::Locale;
    use std::collections::{HashMap, HashSet};
    use tempfile::TempDir;

    fn app_config() -> AppConfig {
        AppConfig {
            default_locale: Locale::EnUs,
            i18n: HashMap::from([(
                Locale::EnUs,
                AppMetadata::new("Demo", "Demo", "A demo app", "Demo Publisher", "Copyright"),
            )]),
            authors: Some(HashSet::from(["Zoe <z@example.com>".to_string()])),
            url: "https://example.com".to_string(),
        }
    }

    fn context<'a>(app_config: &'a AppConfig, debian: &'a DebianConfig) -> ControlContext<'a> {
        ControlContext {
            app_config,
            package_name: "demo",
            version: "1.2.3",
            description: "A demo app",
            architecture: &Arch::X64,
            maintainer: None,
            debian,
        }
    }

    fn write_control(debian: &DebianConfig) -> (TempDir, String) {
        let dir = TempDir::new().unwrap();
        let control_dir = dir.path().join("control");
        let data_dir = dir.path().join("data");
        ensure_directory_exists(&control_dir).unwrap();
        ensure_directory_exists(&data_dir).unwrap();
        fs::write(data_dir.join("file.txt"), "content").unwrap();

        let app_config = app_config();
        write_control_members(&control_dir, &data_dir, &context(&app_config, debian)).unwrap();

        let written = fs::read_to_string(control_dir.join("control")).unwrap();
        (dir, written)
    }

    /// A continuation line at column 0 is read as a new field; dpkg then rejects the
    /// package for a malformed control file, which is what a `description` with newlines
    /// used to produce.
    /// The changelog was installed after `Installed-Size` and `md5sums` were computed, so the
    /// package declared one file fewer than it carried.
    #[test]
    fn the_changelog_is_counted_and_listed() {
        let dir = TempDir::new().unwrap();
        let control_dir = dir.path().join("control");
        let data_dir = dir.path().join("data");
        ensure_directory_exists(&control_dir).unwrap();
        ensure_directory_exists(&data_dir).unwrap();
        fs::write(data_dir.join("file.txt"), "content").unwrap();

        let changelog = dir.path().join("changelog");
        fs::write(&changelog, "demo (1.2.3) unstable; urgency=low\n").unwrap();
        let debian = DebianConfig {
            changelog: Some(changelog),
            ..DebianConfig::default()
        };

        let app_config = app_config();
        write_control_members(&control_dir, &data_dir, &context(&app_config, &debian)).unwrap();

        let installed_size = calculate_installed_size_in_kb(&data_dir).unwrap();
        let control = fs::read_to_string(control_dir.join("control")).unwrap();
        assert!(
            control.contains(&format!("Installed-Size: {installed_size}\n")),
            "the size must cover the changelog as well:\n{control}"
        );

        // The listing uses the host separator, so the expectation is built the same way.
        let installed_changelog = Path::new("usr")
            .join("share")
            .join("doc")
            .join("demo")
            .join("changelog.Debian.gz");
        let installed_changelog = installed_changelog.to_string_lossy();

        let md5sums = fs::read_to_string(control_dir.join("md5sums")).unwrap();
        assert!(
            md5sums
                .lines()
                .any(|line| line.ends_with(installed_changelog.as_ref())),
            "every installed file belongs in md5sums:\n{md5sums}"
        );
    }
    #[test]
    fn a_multi_line_description_is_folded_into_one_field() {
        let dir = TempDir::new().unwrap();
        let control_dir = dir.path().join("control");
        let data_dir = dir.path().join("data");
        ensure_directory_exists(&control_dir).unwrap();
        ensure_directory_exists(&data_dir).unwrap();

        let app_config = app_config();
        let debian = DebianConfig::default();
        let mut control_context = context(&app_config, &debian);
        control_context.description = "A demo app\nIt does things\n\nAnd more";
        write_control_members(&control_dir, &data_dir, &control_context).unwrap();

        let control = fs::read_to_string(control_dir.join("control")).unwrap();
        assert!(
            control.contains("Description: A demo app\n It does things\n .\n And more\n"),
            "{control}"
        );
    }

    #[test]
    fn a_single_line_description_is_written_unchanged() {
        assert_eq!(description_field("A demo app"), "A demo app");
    }
    #[test]
    fn control_contains_required_fields() {
        let (_dir, control) = write_control(&DebianConfig::default());

        assert!(control.contains("Package: demo\n"));
        assert!(control.contains("Version: 1.2.3\n"));
        assert!(control.contains("Architecture: amd64\n"));
        assert!(control.contains("Homepage: https://example.com\n"));
        assert!(control.contains("Description: A demo app\n"));
        // Priority always has a value, per Debian policy.
        assert!(control.contains("Priority: optional\n"));
        // An unset Section must not emit an empty field.
        assert!(!control.contains("Section:"));
    }

    #[test]
    fn maintainer_prefers_authors_over_publisher() {
        let (_dir, control) = write_control(&DebianConfig::default());
        assert!(control.contains("Maintainer: Zoe <z@example.com>\n"));
    }

    #[test]
    fn maintainer_prefers_explicit_configuration() {
        let dir = TempDir::new().unwrap();
        let control_dir = dir.path().join("control");
        let data_dir = dir.path().join("data");
        ensure_directory_exists(&control_dir).unwrap();
        ensure_directory_exists(&data_dir).unwrap();

        let app_config = app_config();
        let debian = DebianConfig::default();
        let context = ControlContext {
            maintainer: Some("Explicit <e@example.com>"),
            ..context(&app_config, &debian)
        };
        write_control_members(&control_dir, &data_dir, &context).unwrap();

        let control = fs::read_to_string(control_dir.join("control")).unwrap();
        assert!(control.contains("Maintainer: Explicit <e@example.com>\n"));
    }

    #[test]
    fn optional_section_and_relationships_are_emitted() {
        let debian = DebianConfig {
            section: Some("utils".to_string()),
            depends: Some(vec!["libc6".to_string(), "libgtk-3-0".to_string()]),
            ..Default::default()
        };
        let (_dir, control) = write_control(&debian);

        assert!(control.contains("Section: utils\n"));
        assert!(control.contains("Depends: libc6, libgtk-3-0\n"));
        // Unset relationships must be omitted entirely.
        assert!(!control.contains("Recommends:"));
    }

    #[test]
    fn architecture_uses_the_debian_spelling() {
        // `x64` is pacor's internal name and is not a Debian architecture.
        assert_eq!(debian_architecture(&Arch::X64), "amd64");
        assert_eq!(debian_architecture(&Arch::ARM64), "arm64");
        // Only x86 differs between the two vocabularies; arm64 happens to be
        // spelled the same way, which is why the mapping must not be assumed.
        assert_ne!(debian_architecture(&Arch::X64), Arch::X64.as_str());
    }

    #[test]
    fn package_name_replaces_underscores() {
        // Debian policy forbids underscores in package names; the caller is
        // responsible for normalising, which `deb.rs` does when building.
        let (_dir, control) = write_control(&DebianConfig::default());
        assert!(!control.contains("Package: demo_"));
    }
}
