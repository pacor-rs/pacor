/// Build process errors with specific context.
#[derive(thiserror::Error, Debug)]
pub enum BuildError {
    #[error("No executable file found in build output")]
    ExecutableNotFound,

    #[error("[{platform}] Flutter build failed with exit code: {output}")]
    FlutterBuild { output: String, platform: String },

    #[error("Failed to generate build artifact {artifact}: {reason}")]
    ArtifactGeneration { artifact: String, reason: String },

    #[error("Template processing failed for {template}: {reason}")]
    TemplateProcessing { template: String, reason: String },

    #[error("Build cleanup failed: {reason}")]
    Cleanup { reason: String },
}
