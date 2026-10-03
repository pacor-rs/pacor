use tracing::Level;

pub(crate) fn init_tracing(level: Level) {
    tracing_subscriber::fmt()
        .with_file(cfg!(feature = "log-line-number") || cfg!(debug_assertions))
        .with_target(false)
        .with_line_number(cfg!(feature = "log-line-number") || cfg!(debug_assertions))
        .with_env_filter(format!("pacor={}", level))
        .with_writer(std::io::stderr)
        .without_time()
        .compact() // Use compact format for smaller binary
        .init();
}
