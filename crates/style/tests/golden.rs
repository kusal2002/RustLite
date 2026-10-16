//! Golden tests: computed styles for `tests/style/<name>.html` must match
//! `tests/style/<name>.styles`, which a reference browser produced with
//! `getComputedStyle` in an 800×600 viewport (see `scripts/oracle/server.py`).
//!
//! A fixture may start with `<!-- compare: prop prop ... -->` to choose the
//! properties compared; otherwise `DEFAULT_DUMP_PROPERTIES` is used.

use std::fs;
use std::path::PathBuf;

use rustlite_style::{DEFAULT_DUMP_PROPERTIES, compute_styles, dump_styles, stylist_for};

/// Fixtures where RustLite intentionally differs from the reference browser,
/// with the reason. Unexpected passes fail the test so stale entries get removed.
const KNOWN_DIFFERENCES: &[(&str, &str)] = &[];

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/style")
}

fn compared_properties(source: &str) -> Vec<String> {
    let header = source
        .find("<!--")
        .map(|i| &source[i + 4..])
        .and_then(|rest| rest.trim_start().strip_prefix("compare:"))
        .and_then(|rest| rest.split_once("-->"))
        .map(|(props, _)| props);
    match header {
        Some(props) => props.split_whitespace().map(str::to_owned).collect(),
        None => DEFAULT_DUMP_PROPERTIES
            .iter()
            .map(|s| (*s).to_owned())
            .collect(),
    }
}

/// Line-level diff: only the differing lines, with the element they belong to.
fn diff(expected: &str, actual: &str) -> String {
    let mut out = String::new();
    let mut element = "";
    for (e, a) in expected.lines().zip(actual.lines()) {
        if e.starts_with('<') {
            element = e;
        }
        if e != a {
            out.push_str(&format!(
                "  {element} expected `{}` got `{}`\n",
                e.trim(),
                a.trim()
            ));
        }
    }
    let (el, al) = (expected.lines().count(), actual.lines().count());
    if el != al {
        out.push_str(&format!("  line count: expected {el}, got {al}\n"));
    }
    out
}

#[test]
fn style_golden_fixtures_match_reference_browser() {
    let mut entries: Vec<_> = fs::read_dir(fixtures_dir())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "html"))
        .collect();
    entries.sort();
    assert!(!entries.is_empty(), "no fixtures found");

    let mut failures = Vec::new();
    for path in &entries {
        let name = path.file_stem().unwrap().to_str().unwrap().to_owned();
        let source = fs::read_to_string(path).unwrap();
        let expected = fs::read_to_string(path.with_extension("styles"))
            .unwrap_or_else(|_| panic!("{name}: missing .styles (run the oracle)"));

        let parsed = rustlite_html::parse_document(&source);
        let doc = &parsed.document;
        let stylist = stylist_for(doc);
        let (styles, _) = compute_styles(doc, &stylist, (800.0, 600.0));
        let props = compared_properties(&source);
        let props: Vec<&str> = props.iter().map(String::as_str).collect();
        let actual = dump_styles(doc, &styles, &props);

        let known = KNOWN_DIFFERENCES.iter().any(|(n, _)| *n == name);
        match (actual == expected, known) {
            (true, true) => failures.push(format!(
                "{name}: listed in KNOWN_DIFFERENCES but now matches; remove the entry"
            )),
            (false, false) => failures.push(format!("{name}:\n{}", diff(&expected, &actual))),
            _ => {}
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} style fixtures failed:\n\n{}",
        failures.len(),
        entries.len(),
        failures.join("\n")
    );
}
