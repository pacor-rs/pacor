//! Code signing for the `.app` bundle and the DMG built from it.
//!
//! A signature is a seal over the bytes of everything it contains, so the order is
//! not cosmetic: nested code (frameworks, dylibs) is sealed first, the bundle second,
//! and the disk image last, after it has been built. Signing only the image — which
//! is what this module used to do — produces a valid-looking DMG whose payload is
//! unsigned, and `notarytool` rejects exactly that with "code object is not signed at
//! all", an error that arrives minutes after the build and names none of this.
//!
//! The hardened runtime (`--options runtime`) and a timestamp are both required for
//! notarization; they are unconditional here because a signature without them cannot
//! be distributed, which is the only reason to configure an identity at all.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};

use pacor_core::utils::command::{
    CommandOutputMode, execute_command,
    macros::{command_args, command_args_extend, command_args_push},
};

/// Directory inside the bundle that holds the frameworks and libraries.
const FRAMEWORKS_DIR: &str = "Contents/Frameworks";

/// Extensions treated as nested bundles, which `codesign` signs as a whole.
const NESTED_BUNDLE_EXTENSIONS: [&str; 2] = ["framework", "app"];

/// Extension of a bare dynamic library, signed as a single file.
const NESTED_LIBRARY_EXTENSION: &str = "dylib";

/// One `codesign` run: what it signs and the argument vector it is given.
///
/// Kept separate from execution so the plan — order and flags — can be asserted in
/// tests on any host, where `codesign` does not exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodesignInvocation {
    /// The item the invocation applies to, also handed over as the last argument.
    pub target: PathBuf,
    /// Arguments for `codesign`, in the order they are passed.
    pub arguments: Vec<OsString>,
}

impl CodesignInvocation {
    /// Run this invocation, inheriting the output configuration of the build.
    fn execute(&self, command_output_mode: &CommandOutputMode) -> Result<()> {
        debug!(target = %self.target.display(), "Running codesign");
        execute_command(
            "codesign",
            &self.arguments,
            None,
            None::<&[(&str, &str)]>,
            command_output_mode,
        )
        .with_context(|| format!("failed to sign `{}`", self.target.display()))
    }
}

/// The arguments shared by every signing run.
fn signing_arguments(identity: &str, target: &Path, entitlements: Option<&Path>) -> Vec<OsString> {
    let mut args = command_args!(
        capacity = 9,
        "--force",
        "--timestamp",
        "--options",
        "runtime",
        "--sign",
        identity,
    );
    if let Some(plist) = entitlements {
        command_args_extend!(args, "--entitlements", plist);
    }
    command_args_push!(args, target);
    args
}

/// The plan for signing `app_bundle`: nested code first, the bundle second, and a
/// verification run last.
///
/// Nested items are ordered deepest first, so an item that lives inside another is
/// sealed before its container.
pub fn plan_app_signing(
    app_bundle: &Path,
    identity: &str,
    entitlements: Option<&Path>,
) -> Result<Vec<CodesignInvocation>> {
    let mut nested = collect_nested_code(app_bundle);
    nested.sort_by(|left, right| {
        right
            .components()
            .count()
            .cmp(&left.components().count())
            .then_with(|| left.cmp(right))
    });

    let mut plan: Vec<CodesignInvocation> = nested
        .into_iter()
        .map(|target| CodesignInvocation {
            arguments: signing_arguments(identity, &target, None),
            target,
        })
        .collect();

    plan.push(CodesignInvocation {
        arguments: signing_arguments(identity, app_bundle, entitlements),
        target: app_bundle.to_path_buf(),
    });

    // `--deep` is deprecated for signing but still the check Apple recommends after
    // it: it walks the nested code the plan above just sealed.
    let mut verify = command_args!(
        capacity = 5,
        "--verify",
        "--deep",
        "--strict",
        "--verbose=2"
    );
    command_args_push!(verify, app_bundle);
    plan.push(CodesignInvocation {
        target: app_bundle.to_path_buf(),
        arguments: verify,
    });

    Ok(plan)
}

/// Bundles and libraries directly inside `Contents/Frameworks`.
///
/// A missing directory is normal — a project without plugins has no frameworks —
/// and yields an empty list.
fn collect_nested_code(app_bundle: &Path) -> Vec<PathBuf> {
    let frameworks = app_bundle.join(FRAMEWORKS_DIR);
    let Ok(entries) = std::fs::read_dir(&frameworks) else {
        return Vec::new();
    };

    let mut nested = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default();

        let is_nested_bundle = path.is_dir() && NESTED_BUNDLE_EXTENSIONS.contains(&extension);
        let is_library = path.is_file() && extension == NESTED_LIBRARY_EXTENSION;

        if is_nested_bundle || is_library {
            nested.push(path);
        }
    }
    nested
}

/// Sign `app_bundle` and everything it contains, in the order a signature requires.
///
/// # Errors
///
/// Fails when `codesign` does, and the error names the item that could not be
/// signed. A failing verification also fails the build: an app that will not pass
/// `codesign --verify` is one Gatekeeper refuses, so shipping it helps nobody.
pub fn sign_app_bundle(
    identity: &str,
    app_bundle: &Path,
    entitlements: Option<&Path>,
    command_output_mode: &CommandOutputMode,
) -> Result<()> {
    if !app_bundle.is_dir() {
        return Err(anyhow!(
            "cannot sign `{}`: the app bundle does not exist",
            app_bundle.display()
        ));
    }

    info!(
        signing_identity = %identity,
        app_bundle = %app_bundle.display(),
        entitlements = entitlements.map(|path| path.display().to_string()).as_deref().unwrap_or("none"),
        "Signing app bundle"
    );

    let plan = plan_app_signing(app_bundle, identity, entitlements)?;
    let signed_items = plan.len().saturating_sub(2);
    for invocation in &plan {
        invocation.execute(command_output_mode)?;
    }

    info!(
        nested_items = signed_items,
        "App bundle signature applied and verified"
    );
    Ok(())
}

/// Sign `dmg_path` with a `codesign` identity.
///
/// The bundle inside has to be signed first: this is the outer seal of the chain,
/// and notarization is submitted against it.
pub fn sign_dmg(
    identity: &str,
    dmg_path: &Path,
    command_output_mode: &CommandOutputMode,
) -> Result<()> {
    if !dmg_path.is_file() {
        return Err(anyhow!(
            "cannot sign `{}`: the disk image does not exist",
            dmg_path.display()
        ));
    }

    info!(
        signing_identity = %identity,
        dmg_path = %dmg_path.display(),
        "Signing DMG"
    );

    let args = signing_arguments(identity, dmg_path, None);
    execute_command(
        "codesign",
        &args,
        None,
        None::<&[(&str, &str)]>,
        command_output_mode,
    )
    .with_context(|| format!("failed to sign `{}`", dmg_path.display()))?;

    info!("DMG code signature applied");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const IDENTITY: &str = "Developer ID Application: Example (TEAMID)";

    /// Build a bundle with two frameworks, one bare library and the executable.
    fn app_bundle(root: &Path) -> PathBuf {
        let bundle = root.join("Example.app");
        let frameworks = bundle.join(FRAMEWORKS_DIR);
        fs::create_dir_all(frameworks.join("FlutterMacOS.framework/Versions/A")).unwrap();
        fs::create_dir_all(frameworks.join("App.framework/Versions/A")).unwrap();
        fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
        fs::write(
            frameworks.join("FlutterMacOS.framework/Versions/A/FlutterMacOS"),
            b"",
        )
        .unwrap();
        fs::write(frameworks.join("App.framework/Versions/A/App"), b"").unwrap();
        fs::write(frameworks.join("libhelper.dylib"), b"").unwrap();
        fs::write(bundle.join("Contents/MacOS/Example"), b"").unwrap();
        fs::write(bundle.join("Contents/Info.plist"), b"").unwrap();
        bundle
    }

    fn arguments(invocation: &CodesignInvocation) -> Vec<String> {
        invocation
            .arguments
            .iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn every_nested_item_is_signed_before_the_bundle_that_contains_it() {
        let temp = tempfile::tempdir().unwrap();
        let bundle = app_bundle(temp.path());

        let plan = plan_app_signing(&bundle, IDENTITY, None).unwrap();
        let targets: Vec<PathBuf> = plan
            .iter()
            .map(|invocation| invocation.target.clone())
            .collect();

        assert_eq!(
            targets,
            vec![
                bundle.join("Contents/Frameworks/App.framework"),
                bundle.join("Contents/Frameworks/FlutterMacOS.framework"),
                bundle.join("Contents/Frameworks/libhelper.dylib"),
                bundle.clone(),
                bundle.clone(),
            ],
            "frameworks and libraries come first, then the bundle, then its verification"
        );
    }

    #[test]
    fn nested_code_is_signed_with_a_timestamp_and_the_hardened_runtime() {
        let temp = tempfile::tempdir().unwrap();
        let bundle = app_bundle(temp.path());

        let plan = plan_app_signing(&bundle, IDENTITY, None).unwrap();
        let framework = &plan[0];

        assert_eq!(
            arguments(framework),
            vec![
                "--force",
                "--timestamp",
                "--options",
                "runtime",
                "--sign",
                IDENTITY,
                framework.target.to_str().unwrap(),
            ],
            "notarization needs both the hardened runtime and a timestamp"
        );
    }

    #[test]
    fn the_entitlements_file_is_applied_to_the_bundle_and_not_to_nested_code() {
        let temp = tempfile::tempdir().unwrap();
        let bundle = app_bundle(temp.path());
        let plist = temp.path().join("Release.entitlements");
        fs::write(&plist, b"").unwrap();

        let plan = plan_app_signing(&bundle, IDENTITY, Some(&plist)).unwrap();
        let app = &plan[plan.len() - 2];

        assert!(
            arguments(app).contains(&plist.to_string_lossy().into_owned()),
            "the bundle keeps the entitlements Xcode would have signed it with"
        );
        assert!(
            !arguments(&plan[0]).contains(&plist.to_string_lossy().into_owned()),
            "nested code has no entitlements of its own"
        );
    }

    #[test]
    fn the_last_step_verifies_the_bundle_strictly() {
        let temp = tempfile::tempdir().unwrap();
        let bundle = app_bundle(temp.path());

        let plan = plan_app_signing(&bundle, IDENTITY, None).unwrap();
        let verify = plan.last().unwrap();

        assert_eq!(
            arguments(verify),
            vec![
                "--verify",
                "--deep",
                "--strict",
                "--verbose=2",
                bundle.to_str().unwrap(),
            ]
        );
    }

    #[test]
    fn a_bundle_without_frameworks_is_still_signed_and_verified() {
        let temp = tempfile::tempdir().unwrap();
        let bundle = temp.path().join("Empty.app");
        fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();

        let plan = plan_app_signing(&bundle, IDENTITY, None).unwrap();

        assert_eq!(plan.len(), 2, "the bundle, then the verification");
        assert_eq!(plan[0].target, bundle);
        assert_eq!(plan[1].arguments.last().unwrap(), bundle.as_os_str());
    }
}
