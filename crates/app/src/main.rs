use std::io::Write;
use std::process::ExitCode;

use clap::Parser;
use orre_app::cli::{self, Cli};
use orre_core::paths;

fn main() -> ExitCode {
    let args = Cli::parse();
    match args.command {
        // The headless commands never start GPUI.
        Some(command) => run_command(command),
        None => {
            run_gui(args.showcase);
            ExitCode::SUCCESS
        }
    }
}

/// Runs a CLI command against the database in the data dir
/// (`ORRE_DATA_DIR` when set). Output on stdout, errors on stderr.
fn run_command(command: cli::Command) -> ExitCode {
    let result = paths::database_path()
        .map_err(anyhow::Error::from)
        .and_then(|database| {
            // Not locked for the whole command: `orre mcp` writes stdout
            // from the async runtime's threads, which would wait forever.
            let mut stdout = std::io::stdout();
            let result = cli::run_with(command, &database, &mut stdout, cli::Terminal::detect());
            stdout.flush()?;
            result
        });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

/// Opens the app, or with `--showcase` the component showcase (D2.5).
fn run_gui(showcase: bool) {
    cli::mcp::init_logging();
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            // Must be called before using any GPUI Kit components.
            gpui_kit::init(cx);
            orre_app::theme::install(cx).expect("Failed to install the theme");

            if showcase {
                orre_app::showcase::init(cx);
                orre_app::showcase::open(cx).expect("Failed to open the showcase");
            } else {
                // The app shell (M3.1) around its views; Transactions shows
                // fixture data until the table reads the database (M3.3).
                orre_app::shell::init(cx);
                orre_app::shell::open(cx).expect("Failed to open the window");
            }
        });
}
