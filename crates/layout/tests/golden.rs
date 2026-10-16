//! Golden geometry tests: `tests/layout/<name>.html` laid out in an 800×600
//! viewport must produce the border boxes in `tests/layout/<name>.boxes`,
//! which a reference browser produced with `getBoundingClientRect()` (see
//! `scripts/oracle/server.py`, `/layout` page).
//!
//! Fixture headers:
//! - `<!-- tolerance: N -->` — maximum allowed difference per coordinate (px).
//! - `<!-- fonts: Family -->` — the fixture needs this font installed (it was
//!   observed with it). Without it the fixture is skipped, with a note.

use std::fs;
use std::path::PathBuf;

use rustlite_fonts::FontContext;
use rustlite_layout::{dump_geometry, layout_document};

/// Fixtures where RustLite intentionally differs from the reference browser,
/// with the reason. Unexpected passes fail so stale entries get removed.
const KNOWN_DIFFERENCES: &[(&str, &str)] = &[];

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/layout")
}

fn header<'a>(source: &'a str, key: &str) -> Option<&'a str> {
    let marker = format!("<!-- {key}:");
    let start = source.find(&marker)? + marker.len();
    let end = source[start..].find("-->")? + start;
    Some(source[start..end].trim())
}

/// Compares two geometry dumps; returns human-readable differences.
fn compare(expected: &str, actual: &str, tolerance: f32) -> Vec<String> {
    let mut diffs = Vec::new();
    let (e_lines, a_lines): (Vec<_>, Vec<_>) =
        (expected.lines().collect(), actual.lines().collect());
    if e_lines.len() != a_lines.len() {
        diffs.push(format!(
            "element count: expected {}, got {}",
            e_lines.len(),
            a_lines.len()
        ));
    }
    for (e, a) in e_lines.iter().zip(&a_lines) {
        let (e_name, e_nums) = e.split_once("> ").unwrap_or((e, ""));
        let (a_name, a_nums) = a.split_once("> ").unwrap_or((a, ""));
        if e_name != a_name {
            diffs.push(format!("element mismatch: expected `{e}` got `{a}`"));
            continue;
        }
        let parse = |s: &str| -> Vec<f32> {
            s.split_whitespace()
                .filter_map(|n| n.parse().ok())
                .collect()
        };
        let (ev, av) = (parse(e_nums), parse(a_nums));
        let off = ev
            .iter()
            .zip(&av)
            .map(|(x, y)| (x - y).abs())
            .fold(0.0f32, f32::max);
        if ev.len() != 4 || av.len() != 4 || off > tolerance {
            diffs.push(format!(
                "{e_name}> expected [{e_nums}] got [{a_nums}] (off by {off:.3})"
            ));
        }
    }
    diffs
}

#[test]
fn layout_golden_fixtures_match_reference_browser() {
    let mut entries: Vec<_> = fs::read_dir(fixtures_dir())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "html"))
        .collect();
    entries.sort();
    assert!(!entries.is_empty(), "no fixtures found");

    let mut fonts = FontContext::with_system_fonts();
    let mut failures = Vec::new();
    let mut ran = 0;
    for path in &entries {
        let name = path.file_stem().unwrap().to_str().unwrap().to_owned();
        let source = fs::read_to_string(path).unwrap();
        if let Some(family) = header(&source, "fonts")
            && !fonts.has_family(family)
        {
            eprintln!("note: skipping {name}: font `{family}` is not installed");
            continue;
        }
        let tolerance: f32 = header(&source, "tolerance")
            .and_then(|t| t.parse().ok())
            .unwrap_or(0.01);
        let expected = fs::read_to_string(path.with_extension("boxes"))
            .unwrap_or_else(|_| panic!("{name}: missing .boxes (run the oracle)"));

        let parsed = rustlite_html::parse_document(&source);
        let doc = &parsed.document;
        let stylist = rustlite_style::stylist_for(doc);
        let (styles, _) = rustlite_style::compute_styles(doc, &stylist, (800.0, 600.0));
        let tree = layout_document(doc, &styles, &mut fonts, (800.0, 600.0));
        let actual = dump_geometry(doc, &tree);
        ran += 1;

        let diffs = compare(&expected, &actual, tolerance);
        let known = KNOWN_DIFFERENCES.iter().any(|(n, _)| *n == name);
        match (diffs.is_empty(), known) {
            (true, true) => failures.push(format!(
                "{name}: listed in KNOWN_DIFFERENCES but now matches; remove the entry"
            )),
            (false, false) => failures.push(format!(
                "{name} (tolerance {tolerance}px):\n  {}",
                diffs.join("\n  ")
            )),
            _ => {}
        }
    }
    eprintln!("ran {ran} of {} layout fixtures", entries.len());
    assert!(
        failures.is_empty(),
        "{} layout fixtures failed:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}
