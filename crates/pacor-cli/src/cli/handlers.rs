//! Implementations of the individual CLI commands.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};

use pacor_core::config::PacorConfig;
use pacor_core::core::project::{find_flutter_project_root, validate_project_environment};
use pacor_core::error::{ConfigurationError, Error};
use pacor_core::platforms::Platform;
use pacor_core::utils::command::{CommandOutputMode, execute_flutter_clean_command};
use pacor_core::utils::fs::{clean_dir, directory_size};
use pacor_packaging::packager_for;

use super::output::{OutputFormatter, format_size};

/// Everything needed to run a build.
pub struct BuildRequest {
    pub target: Option<Platform>,
    pub formats: Vec<String>,
    pub use_existing_flutter_build: bool,
    pub skip_cleanup: bool,
    pub command_output: CommandOutputMode,
    pub config_path: Option<PathBuf>,
}

/// Build an installer for the requested target.
pub fn build(request: BuildRequest) -> Result<()> {
    let target = resolve_target(request.target)?;
    let formatter = OutputFormatter::new(CommandOutputMode::Normal);

    let mut config = PacorConfig::load_from(request.config_path.as_deref())?;
    pacor_packaging::apply_format_overrides(&mut config, target, &request.formats)?;

    formatter.section(&format!("Building for {target}"));

    let builder = packager_for(target, &config)?;
    builder.validate()?;

    let artifacts = builder.build(request.use_existing_flutter_build, request.command_output)?;

    if !request.skip_cleanup {
        cleanup_after_build(builder.project_root(), &*builder, request.command_output)?;
    }

    formatter.build_success("Build Completed Successfully", &artifacts);
    Ok(())
}

/// Remove the output directory and Flutter build artifacts.
pub fn clean(target: Option<Platform>, config_path: Option<&std::path::Path>) -> Result<()> {
    let formatter = OutputFormatter::new(CommandOutputMode::Normal);
    formatter.section("Cleaning Build Artifacts");

    let config = PacorConfig::load_from(config_path)?;
    let output_dir = config.output_dir()?;
    if output_dir.exists() {
        formatter.info(&format!("Removing {}", output_dir.display()));
        clean_dir(&output_dir)?;
    }

    let project_root = find_flutter_project_root()?;
    let flutter_build_dir = project_root.join("build");
    if flutter_build_dir.exists() {
        formatter.info(&format!("Removing {}", flutter_build_dir.display()));
        clean_dir(&flutter_build_dir)?;
    }

    if let Some(target) = target {
        formatter.info(&format!("Cleanup completed for target `{}`", target));
    }
    formatter.success("All build artifacts cleaned successfully");
    Ok(())
}

/// Remove pacor's cache of downloaded tools and templates.
///
/// Global rather than per-project: the cache lives in the user's configuration
/// directory and is shared by every project. Everything removed is fetched again
/// on demand.
pub fn cache_clean(dry_run: bool) -> Result<()> {
    let formatter = OutputFormatter::new(CommandOutputMode::Normal);
    formatter.section("Cleaning Cache");

    let root = pacor_core::tools::cache_root()
        .ok_or_else(|| anyhow!("failed to determine the user cache directory"))?;

    // Report the individual entries, so a destructive command says exactly what
    // it is about to remove.
    let mut entries: Vec<(PathBuf, u64)> = Vec::new();
    if root.exists() {
        for entry in
            fs::read_dir(&root).with_context(|| format!("failed to read {}", root.display()))?
        {
            let path = entry?.path();
            entries.push((path.clone(), directory_size(&path)));
        }
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));

    if entries.is_empty() {
        formatter.info(&format!("Nothing cached in {}", root.display()));
        formatter.success("Cache is already empty");
        return Ok(());
    }

    let total: u64 = entries.iter().map(|(_, size)| *size).sum();
    for (path, size) in &entries {
        formatter.info(&format!("{} ({})", path.display(), format_size(*size)));
    }

    if dry_run {
        formatter.success(&format!(
            "Would free {} (dry run, nothing was removed)",
            format_size(total)
        ));
        return Ok(());
    }

    for (path, _) in &entries {
        fs::remove_dir_all(path).with_context(|| format!("failed to remove {}", path.display()))?;
    }

    formatter.success(&format!(
        "Freed {} in {}",
        format_size(total),
        root.display()
    ));
    Ok(())
}

/// Validate the project and the build environment for a target.
pub fn check(target: Option<Platform>, config_path: Option<&std::path::Path>) -> Result<()> {
    let formatter = OutputFormatter::new(CommandOutputMode::Normal);
    formatter.section("Environment Validation");

    validate_project_environment().context("project environment is not valid")?;
    let project_root = find_flutter_project_root()?;
    formatter.success(&format!("Project root: {}", project_root.display()));

    let config = PacorConfig::load_from(config_path)?;
    formatter.success("Configuration loaded and validated");

    let target = resolve_target(target)?;
    let builder = packager_for(target, &config)?;
    builder.validate()?;
    formatter.success(&format!("`{target}` build environment is valid"));

    Ok(())
}

/// Write the commented configuration template.
pub fn init(force_overwrite: bool, config_path: Option<&std::path::Path>) -> Result<()> {
    let formatter = OutputFormatter::new(CommandOutputMode::Normal);

    let path = match config_path {
        Some(path) => path.to_path_buf(),
        None => find_flutter_project_root()
            .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
            .join(pacor_core::config::CONFIG_FILE_NAME),
    };

    if path.exists() && !force_overwrite {
        return Err(Error::Configuration(ConfigurationError::FileAlreadyExists {
            path: path.clone(),
        })
        .into());
    }

    std::fs::write(&path, pacor_core::config::CONFIG_TEMPLATE).map_err(|error| {
        Error::Configuration(ConfigurationError::WriteFailed {
            path: path.clone(),
            reason: error.to_string(),
        })
    })?;

    formatter.success(&format!("Configuration file written to {}", path.display()));
    formatter.info("Edit it and run `pacor build --target <os>`.");
    Ok(())
}

/// Print the effective configuration.
pub fn show(show_full_config: bool, config_path: Option<&std::path::Path>) -> Result<()> {
    let formatter = OutputFormatter::new(CommandOutputMode::Normal);
    let config = PacorConfig::load_from(config_path)?;

    formatter.section("Current Configuration");

    if show_full_config {
        let serialized = toml::to_string_pretty(&config).map_err(|error| {
            Error::Configuration(ConfigurationError::SerializationFailed {
                reason: error.to_string(),
            })
        })?;
        formatter.info(&serialized);
        return Ok(());
    }

    formatter.info(&config.app.to_string());
    formatter.info("Configured platforms:");
    for target in Platform::ALL {
        if let Some(summary) = platform_summary(&config, target) {
            formatter.success(&format!("  - {target}: {summary}"));
        }
    }

    Ok(())
}

/// Human-readable summary of a configured target, or `None` when absent.
fn platform_summary(config: &PacorConfig, target: Platform) -> Option<String> {
    let platforms = &config.platforms;
    match target {
        Platform::Linux => platforms.linux.as_ref().map(|linux| {
            let formats = linux
                .formats()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            format!("formats = [{formats}]")
        }),
        Platform::Windows => platforms.windows.as_ref().map(|windows| {
            format!(
                "installer = {}, portable = {}",
                windows.installer_format(),
                windows.is_portable_build_enabled()
            )
        }),
        Platform::MacOS => platforms
            .macos
            .as_ref()
            .map(|_| "dmg + updater archive".to_string()),
        Platform::Android => platforms.android.as_ref().map(|android| {
            let abis = android
                .target_abi
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "formats = [{}], target_abi = [{abis}]",
                android.format_list()
            )
        }),
    }
}

/// Resolve the target, defaulting to the host platform.
pub fn resolve_target(target: Option<Platform>) -> Result<Platform> {
    target.or_else(Platform::host).ok_or_else(|| {
        anyhow!(
            "the host platform `{}` is not supported; pass --target explicitly",
            std::env::consts::OS
        )
    })
}

/// Run `flutter clean` and the backend's own cleanup.
fn cleanup_after_build(
    project_root: &std::path::Path,
    builder: &dyn pacor_core::core::builder::BundleBuilder<
        Config = &PacorConfig,
        Output = Vec<pacor_core::platforms::BuildArtifact>,
    >,
    command_output: CommandOutputMode,
) -> Result<()> {
    execute_flutter_clean_command(project_root, &command_output)
        .context("`flutter clean` failed")?;
    builder.clean()?;
    Ok(())
}
