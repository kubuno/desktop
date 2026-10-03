//! `kubuno-data-tool --stdio`: the JSON-lines helper of the Visual Studio data tooling (see the
//! library documentation). `--version` prints the version.

use std::io::{stdin, stdout, BufReader};
use std::process::ExitCode;

use kubuno_data_tool::home::Home;
use kubuno_data_tool::server::{serve, VERSION};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version") | Some("-V") => {
            println!("{VERSION}");
            ExitCode::SUCCESS
        }
        Some("--stdio") => {
            // Logs go to stderr, never to the protocol's stdout; never a secret.
            let level = match std::env::var("KUBUNO_DATA_TOOL_LOG").as_deref() {
                Ok("debug") => tracing::Level::DEBUG,
                Ok("warn") => tracing::Level::WARN,
                _ => tracing::Level::INFO,
            };
            tracing_subscriber::fmt().with_writer(std::io::stderr).with_ansi(false).with_max_level(level).init();
            match serve(BufReader::new(stdin().lock()), stdout(), Home::from_env()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    tracing::error!(error = %e, "the tool stopped");
                    ExitCode::FAILURE
                }
            }
        }
        _ => {
            eprintln!("usage: kubuno-data-tool --stdio | --version");
            ExitCode::from(2)
        }
    }
}
