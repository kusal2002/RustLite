use std::process::ExitCode;

use rustlite_desktop::app;
use rustlite_desktop::options::{Options, USAGE};

fn main() -> ExitCode {
    let options = match Options::parse(std::env::args().skip(1)) {
        Ok(options) => options,
        Err(e) => {
            eprintln!("error: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    if options.help {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    if let Err(e) = rustlite_diagnostics::init_logging() {
        eprintln!("warning: logging disabled: {e}");
    }
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        os = std::env::consts::OS,
        "RustLite starting"
    );

    let result = if let Some(path) = &options.dump_dom {
        app::dump_dom(path).map(|tree| print!("{tree}"))
    } else if let Some(path) = &options.dump_style {
        app::dump_style(path, options.size).map(|styles| print!("{styles}"))
    } else if let Some(path) = &options.headless_output {
        app::render_headless(&options, path).map(drop)
    } else {
        app::run(options).map(drop)
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            let mut message = e.to_string();
            let mut source = std::error::Error::source(&e);
            while let Some(cause) = source {
                message.push_str(&format!(": {cause}"));
                source = cause.source();
            }
            tracing::error!("{message}");
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}
