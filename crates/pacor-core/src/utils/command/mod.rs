use std::{
    ffi::{OsStr, OsString},
    path::Path,
    process::{Command, Stdio},
};

use anyhow::Result;

use crate::error::CommandError;
use crate::utils::command::macros::{command_args, command_args_extend, command_args_push};

pub mod macros;

pub trait ToOsString {
    fn to_os_string(self) -> OsString;
}

impl ToOsString for &str {
    fn to_os_string(self) -> OsString {
        OsString::from(self)
    }
}

impl ToOsString for &Path {
    fn to_os_string(self) -> OsString {
        self.as_os_str().to_os_string()
    }
}

impl ToOsString for &OsString {
    fn to_os_string(self) -> OsString {
        self.clone()
    }
}

macro_rules! impl_to_os_string_for_display {
    ($($t:ty),* $(,)?) => {
        $(
            impl ToOsString for $t {
                fn to_os_string(self) -> OsString {
                    OsString::from(self.to_string())
                }
            }
        )*
    };
}

impl_to_os_string_for_display!(u8, u16, u32, u64, i8, i16, i32, i64, bool, f32, f64);

/// Execute a system command and verify it exits successfully.
///
/// # Arguments
///
/// * `program` - The executable to run
/// * `args` - Command line arguments. **It is recommended to construct this using the
///   [`command_args!`] macro** (or modify it with [`command_args_push!`] /
///   [`command_args_extend!`]) to automatically convert various types (e.g., `&str`,
///   `Path`, or any `Display` type) into `OsString`.
/// * `working_dir` - Optional working directory
/// * `env` - Optional environment variables
/// * `command_output_mode` - Output mode configuration
///
/// # Errors
///
/// Returns `BundleError::Tool` if the command fails or exits with non-zero status.
///
/// # Examples
///
/// ```no_run
/// use std::path::Path;
/// use crate::cli::args::CommandOutputMode;
/// use crate::utils::command::{execute_command, macros::command_args};
///
/// execute_command(
///     "cargo",
///     command_args!("build", "--release", Path::new("src")),
///     Some(Path::new(".")),
///     None::<&[(&str, &str)]>,
///     &CommandOutputMode::Normal,
/// )?;
/// # Ok::<(), crate::error::BundleError>(())
/// ```
///
/// You can also build arguments incrementally:
///
/// ```no_run
/// use std::path::Path;
/// use crate::cli::args::CommandOutputMode;
/// use crate::utils::command::{execute_command, macros::{command_args, command_args_push}};
///
/// let mut args = command_args!("build");
/// command_args_push!(args, "--release");
/// command_args_push!(args, Path::new("src"));
///
/// execute_command(
///     "cargo",
///     args,
///     Some(Path::new(".")),
///     None::<&[(&str, &str)]>,
///     &CommandOutputMode::Normal,
/// )?;
/// # Ok::<(), crate::error::BundleError>(())
/// ```
pub fn execute_command<S, I, K, V>(
    program: S,
    args: I,
    working_dir: Option<&Path>,
    env: Option<&[(K, V)]>,
    command_output_mode: &CommandOutputMode,
) -> Result<(), CommandError>
where
    S: AsRef<OsStr>,
    I: IntoIterator,
    I::Item: AsRef<OsStr>,
    K: AsRef<OsStr>,
    V: AsRef<OsStr>,
{
    let program = program.as_ref();
    let command = build_command(program, args, working_dir, env, command_output_mode);
    execute_and_check_status(command, program)
}

/// Build a Command with all specified configurations.
fn build_command<I, K, V>(
    program: &OsStr,
    args: I,
    working_dir: Option<&Path>,
    env: Option<&[(K, V)]>,
    command_output_mode: &CommandOutputMode,
) -> Command
where
    I: IntoIterator,
    I::Item: AsRef<OsStr>,
    K: AsRef<OsStr>,
    V: AsRef<OsStr>,
{
    let mut command = Command::new(program);
    command.args(args);

    configure_output(&mut command, command_output_mode);

    if let Some(dir) = working_dir {
        command.current_dir(dir);
    }

    if let Some(env_vars) = env {
        command.envs(env_vars.iter().map(|(k, v)| (k.as_ref(), v.as_ref())));
    }

    command
}

/// Configure command output streams based on mode.
fn configure_output(command: &mut Command, mode: &CommandOutputMode) {
    match mode {
        CommandOutputMode::None => {
            command.stdout(Stdio::null()).stderr(Stdio::null());
        }
        CommandOutputMode::Normal | CommandOutputMode::Verbose => {
            command.stdout(Stdio::inherit()).stderr(Stdio::inherit());
        }
    }
}

/// Execute command and check its exit status.
fn execute_and_check_status(mut command: Command, program: &OsStr) -> Result<(), CommandError> {
    let status = command.status().map_err(|e| {
        error!("Failed to execute '{}': {}", program.display(), e);
        CommandError::Execute {
            command: program.to_os_string(),
            error: e,
        }
    })?;

    if !status.success() {
        let exit_info = status.code().map_or_else(
            || "terminated by signal".to_string(),
            |code| format!("exit code {code}"),
        );

        return Err(CommandError::Failed {
            command: program.to_os_string(),
            exit_info,
        });
    }

    Ok(())
}

/// Determine the Flutter executable name based on the platform.
#[inline]
pub const fn flutter_executable() -> &'static str {
    if cfg!(target_os = "windows") {
        "flutter.bat"
    } else {
        "flutter"
    }
}

/// Execute a Flutter subcommand with optional extra arguments.
fn execute_flutter_subcommand(
    subcommand: &str,
    extra_args: &[OsString],
    working_dir: &Path,
    command_output_mode: &CommandOutputMode,
) -> Result<(), CommandError> {
    let mut args = command_args!(capacity = extra_args.len() + 2, subcommand);
    command_args_extend!(args, extra_args);

    if matches!(command_output_mode, CommandOutputMode::Verbose) {
        command_args_push!(args, "--verbose");
    }

    execute_command(
        flutter_executable(),
        args,
        Some(working_dir),
        None::<&[(&str, &str)]>,
        command_output_mode,
    )
}

/// Execute `flutter build` with the given arguments.
///
/// # Errors
///
/// Returns an error if the Flutter build command fails.
///
/// # Examples
///
/// ```no_run
/// # use std::path::Path;
/// # use crate::cli::args::CommandOutputMode;
/// execute_flutter_build_command(
///     &["apk", "--release"],
///     Path::new("./my_flutter_app"),
///     &CommandOutputMode::Normal,
/// )?;
/// # Ok::<(), crate::error::BundleError>(())
/// ```
pub fn execute_flutter_build_command(
    build_args: &[OsString],
    working_dir: &Path,
    command_output_mode: &CommandOutputMode,
) -> Result<(), CommandError> {
    execute_flutter_subcommand("build", build_args, working_dir, command_output_mode)
}

#[cfg(windows)]
fn find_command_with_extension(dir: &Path, command_name: &str) -> Option<std::path::PathBuf> {
    const WINDOWS_EXECUTABLE_EXTENSIONS: [&str; 2] = ["bat", "exe"];

    for extension in WINDOWS_EXECUTABLE_EXTENSIONS.iter() {
        let command_path = dir.join(command_name).with_extension(extension);
        if command_path.is_file() {
            return Some(command_path);
        }
    }

    None
}

pub fn find_command_path(command_name: &str) -> Result<Option<std::path::PathBuf>> {
    let paths = match std::env::var_os("PATH") {
        None => {
            warn!("`PATH` environment variable not found");
            return Ok(None);
        }
        Some(value) => value,
    };

    trace!(command = command_name, "Searching for executable in PATH");
    for dir in std::env::split_paths(&paths) {
        #[cfg(windows)]
        {
            if let Some(candidate) = find_command_with_extension(&dir, command_name) {
                return Ok(Some(candidate));
            }
        }

        #[cfg(unix)]
        {
            let candidate = dir.join(command_name);
            if candidate.is_file() {
                debug!(path = %candidate.display(), command = command_name, "Found executable in PATH");
                return Ok(Some(candidate));
            }
        }
    }

    Ok(None)
}

/// Execute `flutter clean` to remove build artifacts.
///
/// # Errors
///
/// Returns an error if the Flutter clean command fails.
///
/// # Examples
///
/// ```no_run
/// # use std::path::Path;
/// # use crate::cli::args::CommandOutputMode;
/// execute_flutter_clean_command(
///     Path::new("./my_flutter_app"),
///     &CommandOutputMode::Normal,
/// )?;
/// # Ok::<(), crate::error::BundleError>(())
/// ```
pub fn execute_flutter_clean_command(
    working_dir: &Path,
    command_output_mode: &CommandOutputMode,
) -> Result<(), CommandError> {
    execute_flutter_subcommand("clean", &[], working_dir, command_output_mode)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flutter_executable() {
        let executable = flutter_executable();
        if cfg!(target_os = "windows") {
            assert_eq!(executable, "flutter.bat");
        } else {
            assert_eq!(executable, "flutter");
        }
    }

    #[test]
    fn test_configure_output_none() {
        let mut cmd = Command::new("echo");
        configure_output(&mut cmd, &CommandOutputMode::None);
        // Configuration is verified by the command's internal state
    }

    #[test]
    fn test_configure_output_normal() {
        let mut cmd = Command::new("echo");
        configure_output(&mut cmd, &CommandOutputMode::Normal);
        // Configuration is verified by the command's internal state
    }

    #[test]
    fn test_build_command_basic() {
        let cmd = build_command(
            OsStr::new("echo"),
            ["hello"],
            None,
            None::<&[(&str, &str)]>,
            &CommandOutputMode::Normal,
        );
        // Basic smoke test to ensure command builds without panic
        assert!(format!("{:?}", cmd).contains("echo"));
    }

    #[test]
    fn test_build_command_with_env() {
        let env_vars = [("TEST_VAR", "test_value")];
        let cmd = build_command(
            OsStr::new("echo"),
            ["test"],
            None,
            Some(&env_vars),
            &CommandOutputMode::Normal,
        );
        assert!(format!("{:?}", cmd).contains("echo"));
    }

    #[test]
    fn test_execute_command_success() {
        // Test with a command that should always succeed
        let result = execute_command(
            if cfg!(target_os = "windows") {
                "cmd"
            } else {
                "echo"
            },
            if cfg!(target_os = "windows") {
                vec!["/C", "echo", "test"]
            } else {
                vec!["test"]
            },
            None,
            None::<&[(&str, &str)]>,
            &CommandOutputMode::None,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_command_failure() {
        // Test with a command that should fail
        let result = execute_command(
            "nonexistent_command_12345",
            Vec::<&str>::new(),
            None,
            None::<&[(&str, &str)]>,
            &CommandOutputMode::None,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_find_command_path() {
        // Test with a command that should always exist
        #[cfg(windows)]
        {
            let result = find_command_path("tasklist");
            assert!(result.is_ok());
        }
    }
}

/// Controls how external command output is surfaced to the user.
///
/// This type lives in the core crate (not in the CLI) because build backends
/// need it to configure child processes; the CLI only parses it from the
/// command line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CommandOutputMode {
    /// Don't show any command output.
    #[default]
    None,
    /// Show basic command output (stdout/stderr).
    Normal,
    /// Show detailed command output.
    Verbose,
}

impl std::str::FromStr for CommandOutputMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "none" => Ok(Self::None),
            "normal" => Ok(Self::Normal),
            "verbose" => Ok(Self::Verbose),
            other => Err(format!(
                "invalid command output mode `{other}` (expected one of: none, normal, verbose)"
            )),
        }
    }
}

impl std::fmt::Display for CommandOutputMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::None => "none",
            Self::Normal => "normal",
            Self::Verbose => "verbose",
        };
        write!(f, "{value}")
    }
}
