//! Build backend abstraction shared by every platform packager.
//!
//! The trait is intentionally small: a backend is constructed from the whole
//! configuration, then it validates, builds and cleans. Everything a backend
//! needs (project root, output directory, tool cache) is derived inside the
//! backend. The concrete registry that selects a backend for a target lives in
//! the `pacor-packaging` facade crate so that `pacor-core` stays free of
//! platform-specific code.

use std::path::Path;

use anyhow::Result;

use crate::utils::command::CommandOutputMode;

/// A platform-specific packaging backend.
pub trait BundleBuilder {
    /// Configuration type accepted by the backend.
    type Config;

    /// Output produced by a successful build.
    type Output;

    /// Create a backend from the provided configuration.
    fn new(config: Self::Config) -> Result<Self>
    where
        Self: Sized;

    /// Validate that the build environment is usable.
    fn validate(&self) -> Result<()>;

    /// Execute the packaging process.
    fn build(
        &self,
        use_existing_flutter_build: bool,
        command_output_mode: CommandOutputMode,
    ) -> Result<Self::Output>;

    /// Clean up temporary files created while building.
    fn clean(&self) -> Result<()>;

    /// Root directory of the Flutter project.
    fn project_root(&self) -> &Path;

    /// Flutter build output directory for this target.
    fn flutter_build_path(&self) -> &Path;
}
