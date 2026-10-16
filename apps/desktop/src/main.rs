use std::process::ExitCode;

use rustlite_desktop::app;
use rustlite_desktop::options::{Options, USAGE};

fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some(rustlite_desktop::renderer::SELFTEST_SWITCH) {
        let dir = std::env::args().nth(2).unwrap_or_else(|| ".".into());
        rustlite_desktop::renderer::selftest(std::path::Path::new(&dir));
        return ExitCode::SUCCESS;
    }
    if std::env::args().nth(1).as_deref() == Some(rustlite_desktop::renderer::RENDERER_SWITCH) {
        if let Err(e) = rustlite_diagnostics::init_logging() {
            eprintln!("warning: logging disabled: {e}");
        }
        return match rustlite_desktop::renderer::run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                tracing::error!(error = %e, "renderer stopped");
                ExitCode::FAILURE
            }
        };
    }
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
    } else if let Some(path) = &options.dump_layout {
        app::dump_layout(path, options.size).map(|boxes| print!("{boxes}"))
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
