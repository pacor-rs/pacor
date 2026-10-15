//! Authenticode signing of the produced Windows binaries.
//!
//! Everything is driven by `signtool.exe`-compatible arguments
//! (`sign /fd SHA256 /f <pfx> /p <password> …`), which is what the Windows SDK
//! provides and what custom wrappers usually mimic.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};

use pacor_core::{
    config::WindowsConfig,
    utils::command::{CommandOutputMode, execute_command, find_command_path, macros::command_args},
};

/// `signtool.exe` is not on `PATH` by default; it ships inside the SDK's
/// `Windows Kits\<version>\bin\<toolkit version>\<arch>` layout.
const SDK_BIN_DIRS: &[(&str, &str)] = &[("Windows Kits", "10"), ("Windows Kits", "8.1")];

/// Resolved signing configuration: a tool plus the certificate to use.
pub(super) struct Signer<'a> {
    config: &'a WindowsConfig,

    tool: PathBuf,

    certificate: PathBuf,

    /// Never logged or formatted by this type.
    password: String,
}

impl<'a> Signer<'a> {
    /// Resolves the signing tool and validates the certificate configuration.
    ///
    /// # Errors
    /// Fails when a certificate or password is missing, the certificate does not
    /// exist, or no signing tool can be found. Signing is configured explicitly,
    /// so silently producing an unsigned build would be worse than failing.
    pub(super) fn resolve(config: &'a WindowsConfig) -> Result<Self> {
        let certificate = config.certificate_path().clone().ok_or_else(|| {
            anyhow!("`platforms.windows.certificate_path` must be set to sign binaries")
        })?;

        if !certificate.exists() {
            bail!(
                "the code signing certificate {} does not exist",
                certificate.display()
            );
        }

        let password = config.certificate_password().ok_or_else(|| {
            anyhow!(
                "set `PACOR_WINDOWS_CERT_PASSWORD` (or `platforms.windows.certificate_password`) \
                 to sign binaries"
            )
        })?;

        let tool = resolve_signing_tool(config)?;

        debug!(
            tool = %tool.display(),
            certificate = %certificate.display(),
            timestamp_url = config.timestamp_url().unwrap_or("<none>"),
            "Resolved the code signing toolchain"
        );

        Ok(Self {
            config,
            tool,
            certificate,
            password,
        })
    }

    /// Signs one binary in place.
    pub(super) fn sign(
        &self,
        binary: &Path,
        command_output_mode: &CommandOutputMode,
    ) -> Result<()> {
        debug!(binary = %binary.display(), "Signing binary");

        let mut args = command_args!(
            capacity = 12,
            "sign",
            "/fd",
            "SHA256",
            "/f",
            &self.certificate,
            "/p",
            self.password.as_str(),
        );

        if let Some(timestamp_url) = self.config.timestamp_url() {
            // `/tr` selects RFC 3161 timestamping and requires `/td`.
            args.push("/td".into());
            args.push("SHA256".into());
            args.push("/tr".into());
            args.push(timestamp_url.into());
        } else {
            debug!("Timestamping is disabled: the signature will expire with the certificate");
        }

        args.push(binary.as_os_str().into());

        execute_command(
            &self.tool,
            &args,
            None,
            None::<&[(&str, &str)]>,
            command_output_mode,
        )
        .with_context(|| format!("failed to sign {}", binary.display()))?;

        info!(binary = %binary.display(), "Binary signed");
        Ok(())
    }
}

/// Finds the signing tool: explicit configuration first, then `PATH`, then the
/// Windows SDK.
fn resolve_signing_tool(config: &WindowsConfig) -> Result<PathBuf> {
    if let Some(configured) = config.signing_tool_path() {
        if !configured.exists() {
            bail!(
                "the configured signing tool {} does not exist",
                configured.display()
            );
        }
        return Ok(configured.clone());
    }

    if let Some(found) = find_command_path("signtool")? {
        return Ok(found);
    }

    if let Some(found) = find_signtool_in_windows_sdk() {
        debug!(path = %found.display(), "Found signtool in the Windows SDK");
        return Ok(found);
    }

    bail!(
        "no signing tool found: install the Windows SDK, put `signtool.exe` on `PATH`, or set \
         `platforms.windows.signing_tool_path`"
    )
}

/// Looks for `signtool.exe` in the installed Windows SDKs.
fn find_signtool_in_windows_sdk() -> Option<PathBuf> {
    let mut candidates = Vec::new();

    for variable in ["ProgramFiles(x86)", "ProgramFiles"] {
        let Some(root) = env::var_os(variable) else {
            continue;
        };

        for (kits, version) in SDK_BIN_DIRS {
            let bin_dir = PathBuf::from(&root).join(kits).join(version).join("bin");
            let Ok(entries) = fs::read_dir(&bin_dir) else {
                continue;
            };

            for entry in entries.flatten() {
                for arch in ["x64", "x86"] {
                    candidates.push(entry.path().join(arch).join("signtool.exe"));
                }
            }
        }
    }

    sort_newest_first(candidates)
        .into_iter()
        .find(|candidate| candidate.is_file())
}

/// Orders SDK paths newest first, so the most recent toolkit version wins.
///
/// The version is the directory level right below `bin`, e.g. `10.0.22621.0`.
fn sort_newest_first(mut candidates: Vec<PathBuf>) -> Vec<PathBuf> {
    candidates.sort_by(|left, right| {
        sdk_version(right)
            .cmp(&sdk_version(left))
            .then_with(|| left.cmp(right))
    });
    candidates
}

/// Numeric SDK version of a `…\bin\<version>\<arch>\signtool.exe` path.
fn sdk_version(path: &Path) -> Vec<u32> {
    let mut components = path.components();
    while let Some(component) = components.next() {
        if component.as_os_str() != "bin" {
            continue;
        }

        return components
            .next()
            .map(|version| {
                version
                    .as_os_str()
                    .to_string_lossy()
                    .split('.')
                    .map(|part| part.parse::<u32>().unwrap_or(0))
                    .collect()
            })
            .unwrap_or_default();
    }

    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sdk_candidates_are_ordered_newest_first() {
        let candidates = vec![
            PathBuf::from("/kits/Windows Kits/10/bin/10.0.19041.0/x64/signtool.exe"),
            PathBuf::from("/kits/Windows Kits/10/bin/10.0.22621.0/x64/signtool.exe"),
            PathBuf::from("/kits/Windows Kits/10/bin/10.0.22621.0/x86/signtool.exe"),
            PathBuf::from("/kits/Windows Kits/8.1/bin/8.100.26923.0/x64/signtool.exe"),
        ];

        let ordered = sort_newest_first(candidates);

        assert_eq!(
            ordered[0],
            PathBuf::from("/kits/Windows Kits/10/bin/10.0.22621.0/x64/signtool.exe")
        );
        assert_eq!(
            ordered[1],
            PathBuf::from("/kits/Windows Kits/10/bin/10.0.22621.0/x86/signtool.exe")
        );
        assert_eq!(
            ordered.last().unwrap(),
            &PathBuf::from("/kits/Windows Kits/8.1/bin/8.100.26923.0/x64/signtool.exe")
        );
    }

    #[test]
    fn a_path_without_a_toolkit_version_sorts_last() {
        let candidates = vec![
            PathBuf::from("/odd/signtool.exe"),
            PathBuf::from("/kits/Windows Kits/10/bin/10.0.22621.0/x64/signtool.exe"),
        ];

        let ordered = sort_newest_first(candidates);

        assert_eq!(ordered.last().unwrap(), &PathBuf::from("/odd/signtool.exe"));
    }

    #[test]
    fn sdk_versions_are_read_below_the_bin_directory() {
        assert_eq!(
            sdk_version(Path::new("/kits/10/bin/10.0.22621.0/x64/signtool.exe")),
            vec![10, 0, 22621, 0]
        );
        // A non-numeric toolkit directory must not outrank a real version.
        assert_eq!(
            sdk_version(Path::new("/kits/10/bin/toolkit/x64/signtool.exe")),
            vec![0]
        );
        assert!(sdk_version(Path::new("/usr/local/signtool")).is_empty());
    }

    #[test]
    fn signing_requires_a_certificate_and_a_password() {
        let mut config = WindowsConfig::default();
        assert!(
            Signer::resolve(&config).is_err(),
            "no certificate configured"
        );

        config.certificate_path = Some(PathBuf::from("definitely-missing.pfx"));
        assert!(
            Signer::resolve(&config).is_err(),
            "certificate does not exist"
        );
    }
}
