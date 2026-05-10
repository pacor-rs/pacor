//! Cross-platform build system for the Pacor packaging tool.
//!
//! This module provides the core abstractions and factory methods for creating
//! platform-specific builders. The design follows the Builder pattern to provide
//! a consistent interface across different target platforms while allowing for
//! platform-specific customization.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────┐    ┌──────────────────┐    ┌─────────────────┐
//! │  BuilderFactory │───▶│   BundleBuilder  │◀───│ Platform Impls  │
//! └─────────────────┘    │     (trait)      │    │ (macOS, Linux,  │
//!                        └──────────────────┘    │  Windows, etc.) │
//!                                                 └─────────────────┘
//! ```
//!
//! # Usage
//!
//! ```rust
//! use crate::core::builder::{BuilderFactory, BundleBuilder};
//! use crate::cli::args::CommandOutputMode;
//!
//! // Create a builder for the current platform
//! let builder = BuilderFactory::create_for_current_platform()?;
//!
//! // Validate the build environment
//! builder.validate()?;
//!
//! // Execute the build
//! let artifacts = builder.build(false, CommandOutputMode::Normal)?;
//!
//! // Clean up temporary files
//! builder.clean()?;
//! ```
//!
//! # Platform Support
//!
//! The builder system supports the following platforms:
//! - **macOS**: Creates DMG and PKG installers
//! - **Windows**: Creates MSI and NSIS installers  
//! - **Linux**: Creates AppImage, DEB, and RPM packages
//! - **Android**: Creates APK packages (cross-platform)
//!
//! # Error Handling
//!
//! All builder operations return `Result<T, PackageError>` for consistent
//! error handling. Platform-specific errors are wrapped in appropriate
//! error variants with context information.

use std::path::Path;

use anyhow::Result;

use crate::cli::args::CommandOutputMode;
use crate::config::PacorConfig;
use crate::platforms::BuildArtifact;

/// A trait defining a unified interface for building application packages.
///
/// Each platform-specific builder must implement this trait to provide
/// consistent build operations across different operating systems. The trait
/// abstracts away platform differences while ensuring all builders follow
/// the same lifecycle: validate → build → clean.
///
/// # Type Parameters
///
/// - `Config`: The configuration type specific to the builder implementation
/// - `Output`: The output type returned after successful build completion
///
/// # Lifecycle
///
/// 1. **Construction**: Create a new builder instance with configuration
/// 2. **Validation**: Verify the build environment and dependencies
/// 3. **Building**: Execute the packaging process
/// 4. **Cleanup**: Remove temporary files and artifacts
///
/// # Example Implementation
///
/// ```rust
/// use crate::core::builder::BundleBuilder;
/// use crate::cli::args::CommandOutputMode;
/// use crate::error::Result;
/// use crate::platforms::BuildArtifact;
/// use std::path::Path;
///
/// struct MyPlatformBuilder {
///     config: MyConfig,
///     project_root: PathBuf,
/// }
///
/// impl BundleBuilder for MyPlatformBuilder {
///     type Config = MyConfig;
///     type Output = Vec<BuildArtifact>;
///
///     fn new(config: Self::Config) -> Result<Self> {
///         // Initialize builder with configuration
///         todo!()
///     }
///
///     fn validate(&self) -> Result<()> {
///         // Check build environment
///         todo!()
///     }
///
///     fn build(&self, use_existing: bool, output_mode: CommandOutputMode) -> Result<Self::Output> {
///         // Execute the build process
///         todo!()
///     }
///
///     // ... other methods
/// }
/// ```
pub(crate) trait BundleBuilder {
    /// Configuration type specific to the builder implementation.
    type Config;

    /// Output type returned after successful build completion.
    type Output;

    /// Creates a new builder instance with the provided configuration.
    ///
    /// This method should initialize all necessary internal state and validate
    /// that the provided configuration is complete and valid for the target platform.
    ///
    /// # Parameters
    ///
    /// - `config`: Platform-specific configuration containing build settings,
    ///   file paths, signing information, and other platform requirements
    ///
    /// # Returns
    ///
    /// Returns `Ok(Self)` if the builder was successfully created, or an error
    /// if the configuration is invalid or initialization fails.
    ///
    /// # Errors
    ///
    /// - `ConfigurationError`: If the configuration is invalid or incomplete
    /// - `ValidationError`: If required files or directories are missing
    /// - `PlatformError`: If platform-specific validation fails
    fn new(config: Self::Config) -> Result<Self>
    where
        Self: Sized;

    /// Executes the build process with the specified options.
    ///
    /// This is the main method that orchestrates the entire packaging process,
    /// including Flutter compilation (if needed), asset processing, platform-specific
    /// packaging, and artifact generation.
    ///
    /// # Parameters
    ///
    /// - `use_existing_flutter_build`: When `true`, skips the Flutter build step
    ///   and uses existing build artifacts. This can significantly speed up the
    ///   packaging process during development iterations.
    /// - `command_output_mode`: Controls the verbosity of command output displayed
    ///   to the user during the build process.
    ///
    /// # Returns
    ///
    /// Returns the build output artifacts on success, typically including the
    /// main package file and associated metadata (checksums, signatures, etc.).
    ///
    /// # Errors
    ///
    /// - `BuildError::FlutterBuildFailed`: If the Flutter build process fails
    /// - `BuildError::CompilationFailed`: If platform-specific compilation fails
    /// - `BuildError::ArtifactGenerationFailed`: If package generation fails
    /// - `BuildError::SigningFailed`: If code signing fails (when enabled)
    ///
    /// # Platform Notes
    ///
    /// - **Linux**: The `use_existing_flutter_build` option is not supported as
    ///   Linux packaging requires modifying Flutter configuration files
    /// - **Android**: Supports cross-platform builds from any host OS
    /// - **macOS/Windows**: May require platform-specific tools and certificates
    fn build(
        &self,
        use_existing_flutter_build: bool,
        command_output_mode: CommandOutputMode,
    ) -> Result<Self::Output>;

    /// Cleans up temporary files and build artifacts created during the build process.
    ///
    /// This method should remove all temporary files, intermediate build artifacts,
    /// and other resources created during the packaging process. It should be safe
    /// to call multiple times and should not fail if files are already cleaned up.
    ///
    /// # Important Notes
    ///
    /// - This method should NOT remove the final package artifacts (the actual
    ///   output files that users want to keep)
    /// - It should be safe to call even if the build failed partway through
    /// - Should handle missing files gracefully (they may have been cleaned already)
    ///
    /// # Errors
    ///
    /// - `BuildError::CleanupFailed`: If cleanup fails due to permission issues
    ///   or other file system errors
    /// - I/O errors are wrapped in appropriate error types with context
    fn clean(&self) -> Result<()>;

    /// Validates the build environment and configuration before starting the build.
    ///
    /// This method performs comprehensive validation of the build environment,
    /// including checking for required tools, dependencies, file permissions,
    /// and configuration validity. It should be called before attempting to build.
    ///
    /// # Validation Checks
    ///
    /// The validation typically includes:
    /// - **Tool Availability**: Required platform tools (e.g., `xcodebuild`, `makensis`)
    /// - **Flutter Environment**: Flutter SDK installation and version compatibility
    /// - **File Permissions**: Write access to output directories
    /// - **Configuration Validity**: All required configuration fields are present
    /// - **Platform Requirements**: Platform-specific requirements (certificates, etc.)
    /// - **Dependency Resolution**: External dependencies are available
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if all validation checks pass, indicating the build
    /// environment is ready for packaging.
    ///
    /// # Errors
    ///
    /// - `ValidationError::EnvironmentValidation`: If required tools are missing
    /// - `ValidationError::FileValidation`: If required files are missing or inaccessible
    /// - `ValidationError::PermissionDenied`: If insufficient permissions for build
    /// - `PlatformError::ToolNotFound`: If platform-specific tools are missing
    /// - `ConfigurationError::ValidationFailed`: If configuration is invalid
    ///
    /// # Example
    ///
    /// ```rust
    /// let builder = BuilderFactory::create_for_current_platform()?;
    ///
    /// // Validate before building
    /// match builder.validate() {
    ///     Ok(()) => {
    ///         println!("Build environment is valid");
    ///         // Proceed with build
    ///     }
    ///     Err(e) => {
    ///         eprintln!("Validation failed: {}", e);
    ///         // Handle validation errors
    ///     }
    /// }
    /// ```
    fn validate(&self) -> Result<()>;

    /// Returns a reference to the project root directory.
    ///
    /// The project root is the top-level directory containing the Flutter project,
    /// typically identified by the presence of a `pubspec.yaml` file.
    fn project_root(&self) -> &Path;

    /// Returns a reference to the Flutter build output directory.
    ///
    /// This directory contains the compiled Flutter application artifacts
    /// that will be packaged into the final installer or package.
    fn flutter_build_path(&self) -> &Path;
}

/// Factory for creating platform-specific bundle builders.
///
/// The BuilderFactory provides a centralized way to create appropriate builders
/// based on the target platform or specific build requirements. It handles
/// the complexity of platform detection and builder instantiation, ensuring
/// that the correct builder type is created for each platform.
///
/// # Design Pattern
///
/// This factory implements the Abstract Factory pattern, providing a single
/// interface for creating related families of objects (platform builders) without
/// specifying their concrete classes.
///
/// # Platform Detection
///
/// The factory uses Rust's conditional compilation features (`#[cfg(...)]`) to
/// determine the current platform and create the appropriate builder. This ensures
/// that only the relevant platform code is compiled and included in the final binary.
///
/// # Usage Examples
///
/// ```rust
/// use crate::core::builder::BuilderFactory;
///
/// // Create a builder for the current compilation target
/// let current_platform_builder = BuilderFactory::create_for_current_platform()?;
///
/// // Create an Android builder (works from any host platform)
/// let android_builder = BuilderFactory::create_android()?;
/// ```
pub(crate) struct BuilderFactory;

impl BuilderFactory {
    /// Creates a builder appropriate for the current compilation target platform.
    ///
    /// This method automatically detects the current platform and returns the
    /// appropriate builder implementation. The platform detection is performed
    /// at compile time using Rust's conditional compilation features.
    ///
    /// # Platform Mapping
    ///
    /// - **macOS** (`target_os = "macos"`): Returns `MacOSAppBuilder`
    /// - **Linux** (`target_os = "linux"`): Returns `LinuxBuilder`
    /// - **Windows** (`target_os = "windows"`): Returns `WindowsBuilder`
    ///
    /// # Returns
    ///
    /// Returns a boxed trait object implementing `BundleBuilder` for the current
    /// platform. The trait object allows for runtime polymorphism while maintaining
    /// type safety.
    ///
    /// # Errors
    ///
    /// - `ConfigurationError`: If the project configuration cannot be loaded
    /// - `ValidationError`: If the configuration is invalid for the platform
    /// - `PlatformError`: If the platform builder cannot be initialized
    ///
    /// # Example
    ///
    /// ```rust
    /// use crate::core::builder::BuilderFactory;
    /// use crate::cli::args::CommandOutputMode;
    ///
    /// let builder = BuilderFactory::create_for_current_platform()?;
    /// builder.validate()?;
    /// let artifacts = builder.build(false, CommandOutputMode::Normal)?;
    /// ```
    ///
    /// # Note
    ///
    /// This method loads the configuration from the project's `pacor.toml` file
    /// automatically. If you need to use a custom configuration, consider creating
    /// the builder directly or using a different factory method.
    pub(crate) fn create_for_current_platform<'a>(
        config: &'a PacorConfig,
    ) -> Result<Box<dyn BundleBuilder<Config = &'a PacorConfig, Output = Vec<BuildArtifact>> + 'a>>
    {
        #[cfg(target_os = "macos")]
        {
            use crate::platforms::macos::MacOSAppBuilder;
            let builder = MacOSAppBuilder::new(config)?;
            Ok(Box::new(builder))
        }

        #[cfg(target_os = "linux")]
        {
            use crate::platforms::linux::LinuxBuilder;
            let builder = LinuxBuilder::new(config)?;
            Ok(Box::new(builder))
        }

        #[cfg(target_os = "windows")]
        {
            use crate::platforms::windows::WindowsBuilder;
            let builder = WindowsBuilder::new(config)?;
            Ok(Box::new(builder))
        }
    }

    /// Creates an Android builder for cross-platform Android builds.
    ///
    /// Unlike `create_for_current_platform`, this method can be called from any host
    /// platform to build Android applications. This enables cross-platform Android
    /// development and CI/CD scenarios where Android packages need to be built on
    /// non-Android systems.
    ///
    /// # Cross-Platform Support
    ///
    /// The Android builder can run on:
    /// - **macOS**: Full Android development environment
    /// - **Linux**: Full Android development environment  
    /// - **Windows**: Full Android development environment
    ///
    /// # Requirements
    ///
    /// For Android builds to succeed, the following must be available:
    /// - Android SDK with appropriate API levels
    /// - Flutter Android toolchain configuration
    /// - Java Development Kit (JDK) 8 or higher
    /// - Gradle build system
    ///
    /// # Returns
    ///
    /// Returns an `AndroidBuilder` instance configured with the project settings.
    /// Unlike the current platform builder, this returns a concrete type rather
    /// than a trait object.
    ///
    /// # Errors
    ///
    /// - `ConfigurationError`: If the project configuration cannot be loaded
    /// - `ValidationError`: If Android-specific configuration is missing
    /// - `PlatformError`: If the Android builder cannot be initialized
    ///
    /// # Example
    ///
    /// ```rust
    /// use crate::core::builder::BuilderFactory;
    /// use crate::cli::args::CommandOutputMode;
    ///
    /// // Build Android APK from any platform
    /// let android_builder = BuilderFactory::create_android()?;
    /// android_builder.validate()?;
    /// let apk_artifacts = android_builder.build(false, CommandOutputMode::Normal)?;
    /// ```
    pub(crate) fn create_android() -> Result<crate::platforms::android::AndroidBuilder> {
        let config = crate::config::PacorConfig::load()?;
        crate::platforms::android::AndroidBuilder::new(config)
    }
}
