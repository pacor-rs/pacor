use std::process;

use clap::Parser;
use colored::Colorize;
use pacor::{Cli, execute_cli_command};

fn main() {
    let cli = Cli::parse();
    if let Err(error) = execute_cli_command(cli) {
        // `{:#}` prints the whole context chain, not just the outermost message.
        eprintln!("{}", format!("{error:#}").red().bold());
        process::exit(1);
    }
}
