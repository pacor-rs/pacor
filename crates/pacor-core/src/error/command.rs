use std::ffi::OsString;

#[derive(thiserror::Error, Debug)]
pub enum CommandError {
    #[error("Failed to execute {}: {error}",command.display())]
    Execute {
        command: OsString,
        error: std::io::Error,
    },

    #[error("Command {} failed: {exit_info}", command.display())]
    Failed {
        command: OsString,
        exit_info: String,
    },
}
