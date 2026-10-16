//! Golden tests: `tests/html/<name>.html` must parse to `tests/html/<name>.tree`.
//!
//! Expected trees are produced by a reference browser's `DOMParser`
//! (see `scripts/oracle/server.py`, `/html` page), not written by hand. Document modes
//! from the same run are in `tests/html/oracle.txt`. Failures print the
//! input, expected, and actual trees.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use rustlite_dom::{QuirksMode, dump_tree};
use rustlite_html::parse_document;

/// Fixtures where RustLite intentionally differs from the reference browser.
/// Every entry must say why; an unexpected pass also fails the test so stale
/// entries get removed.
const KNOWN_DIFFERENCES: &[(&str, &str)] = &[];

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/html")
}

fn oracle_modes() -> BTreeMap<String, String> {
    let text = fs::read_to_string(fixtures_dir().join("oracle.txt")).expect("oracle.txt missing");
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .filter_map(|l| l.split_once(' '))
        .map(|(n, m)| (n.to_owned(), m.to_owned()))
        .collect()
}

#[test]
fn html_golden_fixtures_match_reference_browser() {
    let modes = oracle_modes();
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
        let input = fs::read_to_string(path).unwrap();
        let expected = fs::read_to_string(path.with_extension("tree"))
            .unwrap_or_else(|_| panic!("{name}: missing .tree (run the oracle)"));
        let out = parse_document(&input);
        let actual = dump_tree(&out.document, out.document.root());

        let expected_mode = match modes.get(&name).map(String::as_str) {
            Some("BackCompat") => Some(QuirksMode::Quirks),
            // CSS1Compat covers both no-quirks and limited-quirks.
            Some("CSS1Compat") => None,
            other => panic!("{name}: no oracle mode ({other:?})"),
        };
        let mode_ok = match expected_mode {
            Some(m) => out.document.quirks_mode() == m,
            None => out.document.quirks_mode() != QuirksMode::Quirks,
        };

        let known = KNOWN_DIFFERENCES.iter().find(|(n, _)| *n == name);
        let pass = actual == expected && mode_ok;
        match (pass, known) {
            (true, Some(_)) => failures.push(format!(
                "{name}: listed in KNOWN_DIFFERENCES but now matches; remove the entry"
            )),
            (false, None) => {
                let what = if mode_ok {
                    "tree mismatch".to_owned()
                } else {
                    format!("quirks mode {:?}", out.document.quirks_mode())
                };
                failures.push(format!(
                    "{name}: {what}\n--- input\n{input}\n--- expected\n{expected}--- actual\n{actual}"
                ));
            }
            _ => {}
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} fixtures failed:\n\n{}",
        failures.len(),
        entries.len(),
        failures.join("\n\n")
    );
}
