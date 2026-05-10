use std::process;

use clap::Parser;
use colored::Colorize;
use pacor::{Cli, execute_cli_command};

fn main() {
    let cli = Cli::parse();
    if let Err(e) = execute_cli_command(cli) {
        eprintln!("{}", e.to_string().red().bold());
        process::exit(1);
    }
}
