//! `rustlite-bench`: times each pipeline stage on the standard workloads.
//!
//! Usage: rustlite-bench [--quick] [--iterations N] [--save FILE]
//!                       [--compare FILE [--threshold X] [--floor-ms MS]]
//! Run release builds only (`cargo run --release -p rustlite-bench`).

use std::process::ExitCode;

use rustlite_bench::{compare, format_baseline, format_table, run, workloads};
use rustlite_fonts::FontContext;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mut quick = false;
    let mut iterations = 20;
    let mut save = None;
    let mut baseline = None;
    let mut threshold = 1.5;
    let mut floor_ms = 0.5;
    while let Some(arg) = args.next() {
        let mut value = |name: &str| {
            args.next()
                .unwrap_or_else(|| panic!("{name} needs a value"))
        };
        match arg.as_str() {
            "--quick" => quick = true,
            "--iterations" => iterations = value("--iterations").parse().expect("number"),
            "--save" => save = Some(value("--save")),
            "--compare" => baseline = Some(value("--compare")),
            "--threshold" => threshold = value("--threshold").parse().expect("number"),
            "--floor-ms" => floor_ms = value("--floor-ms").parse().expect("number"),
            "-h" | "--help" => {
                println!(
                    "rustlite-bench [--quick] [--iterations N] [--save FILE] \
                     [--compare FILE [--threshold X] [--floor-ms MS]]"
                );
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("unknown argument {other:?}");
                return ExitCode::FAILURE;
            }
        }
    }
    if cfg!(debug_assertions) {
        eprintln!("warning: debug build; numbers are not representative (use --release)");
    }

    let mut fonts = FontContext::with_system_fonts();
    if fonts.face_count() == 0 {
        eprintln!("warning: no system fonts; text is not shaped or rasterized");
    }
    let reports: Vec<_> = workloads(quick)
        .iter()
        .map(|w| run(w, &mut fonts, iterations))
        .collect();
    print!("{}", format_table(&reports));

    if let Some(path) = save {
        if let Err(e) = std::fs::write(&path, format_baseline(&reports)) {
            eprintln!("cannot write {path}: {e}");
            return ExitCode::FAILURE;
        }
        println!("baseline saved to {path}");
    }
    if let Some(path) = baseline {
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("cannot read {path}: {e}");
                return ExitCode::FAILURE;
            }
        };
        let (table, regressed) = compare(&reports, &text, threshold, floor_ms);
        println!("\ncompared with {path} (threshold x{threshold}, floor {floor_ms} ms):");
        print!("{table}");
        if regressed {
            eprintln!("performance regression");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}
