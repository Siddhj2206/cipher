use cipher::{Cli, exit_with_error, run_command};
use clap::Parser;

#[tokio::main]
async fn main() {
    cipher::ui::interactive::set_cipher_theme();
    let cli = Cli::parse();
    match run_command(cli.command).await {
        Ok(code) => std::process::exit(code),
        Err(e) => exit_with_error(e),
    }
}
