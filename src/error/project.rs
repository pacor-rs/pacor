#[derive(thiserror::Error, Debug)]
pub(crate) enum ProjectError {
    #[error(
        "Could not find pubspec.yaml. Please make sure you are running this inside a Flutter project."
    )]
    RootNotFound,
    #[error("Flutter is not installed or not in PATH")]
    FlutterNotInstalled,
}
