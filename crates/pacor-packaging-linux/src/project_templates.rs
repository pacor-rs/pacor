//! Shared handling of Flutter project template files.
//!
//! Both the AppImage and the DEB backend temporarily replace a few files in the
//! user's `linux/` directory with rendered upstream templates, build, then put
//! the originals back. That download/render/backup/restore cycle lives here once.
//!
//! # Recovery guarantees
//!
//! The originals are kept in `<file>.pacor.bak`, next to the file they replace:
//!
//! * a [`TemplateSwap`] restores them when it is dropped, so a failing Flutter
//!   build — or any early `?` return — cannot leave the project modified;
//! * a batch is all-or-nothing, so a failure on the second file restores the
//!   first;
//! * [`TemplateApplier::restore_all`] attempts every file even when one fails,
//!   so one broken backup cannot strand the rest of the project;
//! * the replacement is staged before the original is moved aside, so a failed
//!   write cannot leave a file missing from its path;
//! * a backup left behind by a run that was killed outright is restored
//!   automatically at the start of the next run, instead of being overwritten
//!   (which would destroy the original for good).
//!
//! The `.pacor.bak` suffix is what makes the last two points safe: a plain
//! `.bak` could belong to the user, and would be clobbered.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};

use pacor_core::tools::TEMPLATE_REF;
use pacor_core::utils::downloader::download_file;
use pacor_core::utils::fs::ensure_directory_exists;
use pacor_core::utils::template::render_template;

/// Suffix of the backups pacor creates.
///
/// Deliberately not a plain `.bak`: that name is common enough that a file the
/// user keeps for their own reasons could be overwritten (and then consumed by
/// a restore).
const BACKUP_SUFFIX: &str = ".pacor.bak";

/// Suffix of the staged file used to make the swap atomic-ish.
const STAGED_SUFFIX: &str = ".pacor.new";

/// URL of a Linux template inside the pinned tag.
fn template_url(format: &str, remote_filename: &str) -> String {
    pacor_core::tools::template_url(&format!("linux/{format}/{remote_filename}"))
}

/// The local file name a remote template expands to.
fn local_name(remote_filename: &str) -> String {
    remote_filename.replace("_template", "")
}

/// Where the original of `template` is parked while it is replaced.
fn backup_path_for(template: &ProjectTemplate) -> PathBuf {
    template.local_file_path.with_file_name(format!(
        "{}{BACKUP_SUFFIX}",
        local_name(&template.remote_filename)
    ))
}

/// Where the rendered replacement is written before the swap.
fn staged_path_for(template: &ProjectTemplate) -> PathBuf {
    template.local_file_path.with_file_name(format!(
        "{}{STAGED_SUFFIX}",
        local_name(&template.remote_filename)
    ))
}

/// Values substituted into the upstream templates.
///
/// The directory variables must describe where the backend actually puts the
/// Flutter bundle output, relative to the directory that contains `usr/`. They
/// are built from the same constants the installers use, so the absolute paths
/// baked into the runner cannot drift away from the package layout — the bug
/// that made DEB-installed applications look for their assets in `usr/data`.
#[derive(Debug, Clone)]
pub struct TemplateVariables {
    /// Executable name produced by the Flutter build (CMake `BINARY_NAME`).
    pub executable_name: String,
    /// Directory holding `libapp.so` and the bundled libraries.
    pub lib_dir: String,
    /// Directory holding `flutter_assets` and `icudtl.dat`.
    pub data_dir: String,
    /// Reverse-DNS GTK application id embedded in the runner.
    pub application_id: String,
}

/// A project file that is temporarily replaced by a rendered template.
#[derive(Clone, Debug)]
pub struct ProjectTemplate {
    /// File name in the remote template repository.
    pub remote_filename: String,
    /// Local project file that will be replaced.
    pub local_file_path: PathBuf,
}

/// Downloads, renders and backs up project templates for one packaging format.
pub struct TemplateApplier {
    cache_dir: PathBuf,
    format: String,
    variables: TemplateVariables,
}

impl TemplateApplier {
    /// Create an applier caching templates under
    /// `tools_directory/<format>/<TEMPLATE_REF>`.
    pub fn new(tools_directory: &Path, format: &str, variables: TemplateVariables) -> Result<Self> {
        let cache_dir = tools_directory.join(format).join(TEMPLATE_REF);
        ensure_directory_exists(&cache_dir)?;

        Ok(Self {
            cache_dir,
            format: format.to_string(),
            variables,
        })
    }

    /// Apply every template, returning `[file, backup]` pairs for restoration.
    ///
    /// The batch is all-or-nothing: if one template cannot be applied, the ones
    /// already applied are restored before the error is returned.
    pub fn apply_all(&self, templates: &[ProjectTemplate]) -> Result<Vec<[PathBuf; 2]>> {
        let mut applied: Vec<[PathBuf; 2]> = Vec::with_capacity(templates.len());

        for template in templates {
            match self.apply_one(template) {
                Ok(pair) => applied.push(pair),
                Err(error) => {
                    if let Err(rollback) = self.restore_all(&applied) {
                        return Err(error.context(format!(
                            "rolling back the templates that were already applied also failed: \
                             {rollback:#}"
                        )));
                    }
                    return Err(error);
                }
            }
        }

        Ok(applied)
    }

    /// Restore files previously returned by [`TemplateApplier::apply_all`].
    ///
    /// Every file is attempted even when one fails, so a single failure cannot
    /// leave the rest of the project modified.
    pub fn restore_all(&self, modified: &[[PathBuf; 2]]) -> Result<()> {
        let mut failures = Vec::new();

        for [original, backup] in modified {
            match fs::rename(backup, original) {
                Ok(()) => debug!("Restored {}", original.display()),
                Err(error) => failures.push(format!("{}: {error}", original.display())),
            }
        }

        if failures.is_empty() {
            return Ok(());
        }

        bail!(
            "failed to restore {} file(s); the originals are in `*{BACKUP_SUFFIX}`: {}",
            failures.len(),
            failures.join("; ")
        )
    }

    fn apply_one(&self, template: &ProjectTemplate) -> Result<[PathBuf; 2]> {
        let backup_path = backup_path_for(template);
        let staged_path = staged_path_for(template);
        let cached_path = self.cache_dir.join(&template.remote_filename);

        let template_data = if cached_path.exists() {
            fs::read(&cached_path)?
        } else {
            let url = template_url(&self.format, &template.remote_filename);
            debug!("Downloading template {url}");
            let data = download_file(&url)?;
            fs::write(&cached_path, &data)?;
            data
        };

        let content = String::from_utf8_lossy(&template_data);
        let replacements = HashMap::from([
            (
                "APP_EXECUTABLE_NAME",
                self.variables.executable_name.as_str(),
            ),
            ("APP_LIB_DIR", self.variables.lib_dir.as_str()),
            ("APP_DATA_DIR", self.variables.data_dir.as_str()),
            ("APP_APPLICATION_ID", self.variables.application_id.as_str()),
        ]);
        let rendered = render_template(&content, &replacements)?;

        if backup_path.exists() {
            // A leftover backup means a previous run was killed before its guard
            // could run. Put the original back first, otherwise backing up now
            // would overwrite it with the template it left behind.
            warn!(
                file = %template.local_file_path.display(),
                "found a backup from an interrupted run; restoring it before applying"
            );
            fs::rename(&backup_path, &template.local_file_path).with_context(|| {
                format!(
                    "failed to restore the interrupted backup for {}",
                    template.local_file_path.display()
                )
            })?;
        }

        // Stage the replacement beside the target first (same directory, so the
        // rename below cannot cross a filesystem). Writing must not be able to
        // leave the original missing from its path.
        fs::write(&staged_path, rendered)
            .with_context(|| format!("failed to stage {}", staged_path.display()))?;

        if let Err(error) = fs::rename(&template.local_file_path, &backup_path) {
            let _ = fs::remove_file(&staged_path);
            return Err(anyhow::Error::new(error).context(format!(
                "failed to back up {}",
                template.local_file_path.display()
            )));
        }

        if let Err(error) = fs::rename(&staged_path, &template.local_file_path) {
            // Rather than leaving the file absent, put the original back.
            if let Err(rollback) = fs::rename(&backup_path, &template.local_file_path) {
                error!(
                    error = %rollback,
                    file = %template.local_file_path.display(),
                    "Failed to put the original back; it is in the adjacent backup file"
                );
            }
            let _ = fs::remove_file(&staged_path);
            return Err(anyhow::Error::new(error).context(format!(
                "failed to replace {}",
                template.local_file_path.display()
            )));
        }

        debug!("Applied template to {}", template.local_file_path.display());

        Ok([template.local_file_path.clone(), backup_path])
    }
}

/// An in-progress template swap.
///
/// The project files stay replaced for as long as this value is alive. If it is
/// dropped without an explicit [`TemplateSwap::restore`] — for instance because
/// the Flutter build returned an error via `?` — the originals are still put
/// back, so a failed build cannot leave the user's project modified.
pub struct TemplateSwap {
    applier: TemplateApplier,
    applied: Vec<[PathBuf; 2]>,
}

impl TemplateSwap {
    /// Replace the project files covered by `templates` with rendered ones.
    pub fn apply(applier: TemplateApplier, templates: &[ProjectTemplate]) -> Result<Self> {
        let applied = applier.apply_all(templates)?;
        Ok(Self { applier, applied })
    }

    /// Restore the originals, reporting any failure.
    pub fn restore(mut self) -> Result<()> {
        let applied = std::mem::take(&mut self.applied);
        self.applier.restore_all(&applied)
    }
}

impl Drop for TemplateSwap {
    fn drop(&mut self) {
        if self.applied.is_empty() {
            return;
        }

        let applied = std::mem::take(&mut self.applied);
        if let Err(error) = self.applier.restore_all(&applied) {
            // `Drop` cannot propagate, so make the leftover backup files loud:
            // the user's project is still modified and needs attention.
            error!(
                error = %error,
                "Failed to restore the original project files; the originals are in the `*.pacor.bak` files next to the templates"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_variables() -> TemplateVariables {
        TemplateVariables {
            executable_name: "demo".to_string(),
            lib_dir: "lib/demo".to_string(),
            data_dir: "share/demo".to_string(),
            application_id: "com.example.demo".to_string(),
        }
    }

    /// Build an applier whose cache already holds a rendered-ready template.
    fn applier_with_cached_template(temp: &TempDir, directive: &str) -> TemplateApplier {
        let tools = temp.path().join("tools");
        let applier = TemplateApplier::new(&tools, "deb", test_variables()).unwrap();
        // Pre-seed the cache so no network access is needed.
        fs::write(
            applier.cache_dir.join("my_application_template.cc"),
            directive,
        )
        .unwrap();
        applier
    }

    fn template_for(temp: &TempDir) -> ProjectTemplate {
        ProjectTemplate {
            remote_filename: "my_application_template.cc".to_string(),
            local_file_path: temp.path().join("linux/runner/my_application.cc"),
        }
    }

    fn cmake_template_for(temp: &TempDir) -> ProjectTemplate {
        ProjectTemplate {
            remote_filename: "CMakeLists_template.txt".to_string(),
            local_file_path: temp.path().join("linux/CMakeLists.txt"),
        }
    }

    /// Seed the cache for both templates used by the DEB backend.
    fn applier_with_both_templates(temp: &TempDir, directive: &str) -> TemplateApplier {
        let applier = applier_with_cached_template(temp, directive);
        fs::write(applier.cache_dir.join("CMakeLists_template.txt"), directive).unwrap();
        applier
    }

    fn existing_file(template: &ProjectTemplate) -> PathBuf {
        fs::create_dir_all(template.local_file_path.parent().unwrap()).unwrap();
        fs::write(&template.local_file_path, "original").unwrap();
        template.local_file_path.clone()
    }

    #[test]
    fn applying_replaces_the_file_and_keeps_a_backup() {
        let temp = TempDir::new().unwrap();
        let template = template_for(&temp);
        fs::create_dir_all(template.local_file_path.parent().unwrap()).unwrap();
        fs::write(&template.local_file_path, "original").unwrap();

        let applier = applier_with_cached_template(&temp, "APP={{APP_EXECUTABLE_NAME}}\n");
        let swap = TemplateSwap::apply(applier, std::slice::from_ref(&template)).unwrap();

        assert_eq!(
            fs::read_to_string(&template.local_file_path).unwrap(),
            "APP=demo\n"
        );
        assert_eq!(
            fs::read_to_string(backup_path_for(&template)).unwrap(),
            "original"
        );

        swap.restore().unwrap();
    }

    #[test]
    fn restore_puts_the_original_content_back() {
        let temp = TempDir::new().unwrap();
        let template = template_for(&temp);
        fs::create_dir_all(template.local_file_path.parent().unwrap()).unwrap();
        fs::write(&template.local_file_path, "original").unwrap();

        let applier = applier_with_cached_template(&temp, "replaced\n");
        TemplateSwap::apply(applier, std::slice::from_ref(&template))
            .unwrap()
            .restore()
            .unwrap();

        assert_eq!(
            fs::read_to_string(&template.local_file_path).unwrap(),
            "original"
        );
        assert!(
            !backup_path_for(&template).exists(),
            "the backup must be consumed by the restore"
        );
    }

    #[test]
    fn dropping_the_swap_without_restoring_still_restores_the_file() {
        let temp = TempDir::new().unwrap();
        let template = template_for(&temp);
        fs::create_dir_all(template.local_file_path.parent().unwrap()).unwrap();
        fs::write(&template.local_file_path, "original").unwrap();

        let applier = applier_with_cached_template(&temp, "replaced\n");
        let swap = TemplateSwap::apply(applier, std::slice::from_ref(&template)).unwrap();
        assert_eq!(
            fs::read_to_string(&template.local_file_path).unwrap(),
            "replaced\n"
        );

        // Simulates early-returning from a `?` when the Flutter build fails.
        drop(swap);

        assert_eq!(
            fs::read_to_string(&template.local_file_path).unwrap(),
            "original",
            "a failed build must not leave the project modified"
        );
    }

    #[test]
    fn every_placeholder_the_templates_use_is_substituted() {
        let temp = TempDir::new().unwrap();
        let template = template_for(&temp);
        let original = existing_file(&template);

        // The three variables the upstream templates rely on.
        let applier = applier_with_cached_template(
            &temp,
            "{{APP_EXECUTABLE_NAME}}|{{APP_LIB_DIR}}|{{APP_DATA_DIR}}|{{APP_APPLICATION_ID}}\n",
        );
        let swap = TemplateSwap::apply(applier, std::slice::from_ref(&template)).unwrap();

        assert_eq!(
            fs::read_to_string(&original).unwrap(),
            "demo|lib/demo|share/demo|com.example.demo\n"
        );

        swap.restore().unwrap();
    }

    #[test]
    fn a_placeholder_pacor_does_not_provide_fails_loudly() {
        // Upstream adding a variable before the Rust side knows about it must
        // break the build rather than bake `{{...}}` into the generated source.
        let temp = TempDir::new().unwrap();
        let template = template_for(&temp);
        existing_file(&template);

        let applier = applier_with_cached_template(&temp, "{{APP_SOMETHING_NEW}}\n");
        let error = TemplateSwap::apply(applier, std::slice::from_ref(&template))
            .err()
            .expect("an unknown placeholder must fail");

        assert!(
            format!("{error:#}").contains("APP_SOMETHING_NEW"),
            "{error:#}"
        );
    }

    #[test]
    fn a_failure_mid_batch_rolls_back_the_files_already_replaced() {
        let temp = TempDir::new().unwrap();
        let first = template_for(&temp);
        let original = existing_file(&first);

        // The second template has no local file, so applying it fails.
        let second = cmake_template_for(&temp);
        let applier = applier_with_both_templates(&temp, "replaced\n");

        let error = TemplateSwap::apply(applier, &[first.clone(), second])
            .err()
            .expect("applying a template whose local file is missing must fail");

        assert_eq!(
            fs::read_to_string(&original).unwrap(),
            "original",
            "a mid-batch failure left the first file replaced; error was: {error:#}"
        );
    }

    #[test]
    fn an_interrupted_run_does_not_destroy_the_original_on_the_next_run() {
        let temp = TempDir::new().unwrap();
        let template = template_for(&temp);
        let original = existing_file(&template);

        // Simulate `kill -9` during the Flutter build: the guard never runs.
        let swap = TemplateSwap::apply(
            applier_with_cached_template(&temp, "replaced\n"),
            std::slice::from_ref(&template),
        )
        .unwrap();
        std::mem::forget(swap);
        assert_eq!(fs::read_to_string(&original).unwrap(), "replaced\n");

        // The next run must put the original back, not overwrite the backup.
        TemplateSwap::apply(
            applier_with_cached_template(&temp, "replaced\n"),
            std::slice::from_ref(&template),
        )
        .unwrap()
        .restore()
        .unwrap();

        assert_eq!(
            fs::read_to_string(&original).unwrap(),
            "original",
            "the original was destroyed by a run that followed an interrupted one"
        );
    }

    #[test]
    fn restore_attempts_every_file_even_when_one_fails() {
        let temp = TempDir::new().unwrap();
        let first = template_for(&temp);
        let second = cmake_template_for(&temp);
        let first_path = existing_file(&first);
        let second_path = existing_file(&second);

        let applier = applier_with_both_templates(&temp, "replaced\n");
        let mut applied = applier.apply_all(&[first.clone(), second.clone()]).unwrap();

        // Break the first backup so its restore fails.
        let missing = temp.path().join("linux/runner/does-not-exist.bak");
        applied[0][1] = missing;

        assert!(applier.restore_all(&applied).is_err());

        assert_eq!(
            fs::read_to_string(&second_path).unwrap(),
            "original",
            "the second file must still be restored when the first fails"
        );
        let _ = first_path;
    }

    #[test]
    fn a_failed_batch_leaves_no_staged_or_backup_files_behind() {
        let temp = TempDir::new().unwrap();
        let first = template_for(&temp);
        existing_file(&first);
        let second = cmake_template_for(&temp); // no local file -> fails

        let applier = applier_with_both_templates(&temp, "replaced\n");
        let _ = TemplateSwap::apply(applier, &[first, second]);

        let runner_dir = temp.path().join("linux/runner");
        let leftovers: Vec<_> = fs::read_dir(&runner_dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".pacor."))
            .collect();

        assert!(
            leftovers.is_empty(),
            "staging leftovers were not cleaned up: {leftovers:?}"
        );
    }

    #[test]
    fn a_leftover_staged_file_from_a_crash_does_not_break_the_next_run() {
        let temp = TempDir::new().unwrap();
        let template = template_for(&temp);
        let original = existing_file(&template);

        // Debris from a run that was killed between staging and swapping.
        fs::write(staged_path_for(&template), "half written").unwrap();

        TemplateSwap::apply(
            applier_with_cached_template(&temp, "replaced\n"),
            std::slice::from_ref(&template),
        )
        .unwrap()
        .restore()
        .unwrap();

        assert_eq!(fs::read_to_string(&original).unwrap(), "original");
        assert!(
            !staged_path_for(&template).exists(),
            "the stale staged file must be consumed by the swap"
        );
    }

    #[test]
    fn template_caches_are_keyed_by_the_pinned_tag() {
        let temp = TempDir::new().unwrap();
        let tools = temp.path().join("tools");
        let applier = TemplateApplier::new(&tools, "deb", test_variables()).unwrap();

        assert_eq!(applier.cache_dir, tools.join("deb").join(TEMPLATE_REF));
        assert_eq!(
            applier.cache_dir.parent(),
            Some(tools.join("deb").as_path())
        );
    }

    #[test]
    fn templates_are_fetched_from_the_pinned_tag() {
        let url = template_url("deb", "my_application_template.cc");

        assert!(
            url.contains(&format!("/refs/tags/{TEMPLATE_REF}/")),
            "a branch URL would let a template change reach an incompatible \
             pacor build: {url}"
        );
        assert!(!url.contains("/refs/heads/"), "{url}");
        assert!(
            url.ends_with("/linux/deb/my_application_template.cc"),
            "{url}"
        );
    }

    #[test]
    fn the_tag_is_part_of_the_download_url() {
        // Guards against a URL that is pinned in prose but not in code.
        assert!(
            template_url("appimage", "x").contains(TEMPLATE_REF),
            "TEMPLATE_REF must appear in the URL it claims to pin"
        );
    }

    #[test]
    fn a_users_own_backup_file_is_left_alone() {
        let temp = TempDir::new().unwrap();
        let template = template_for(&temp);
        existing_file(&template);

        // A backup the user keeps for their own reasons.
        let users_backup = template
            .local_file_path
            .with_file_name("my_application.cc.bak");
        fs::write(&users_backup, "the user's own backup").unwrap();

        TemplateSwap::apply(
            applier_with_cached_template(&temp, "replaced\n"),
            std::slice::from_ref(&template),
        )
        .unwrap()
        .restore()
        .unwrap();

        assert_eq!(
            fs::read_to_string(&users_backup).unwrap(),
            "the user's own backup",
            "pacor must not clobber a `*.bak` file it does not own"
        );
    }
}
