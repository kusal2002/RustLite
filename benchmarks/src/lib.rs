//! Pipeline benchmarks: deterministic workloads and per-stage timing.
//!
//! No external harness: each stage runs a few warm-up iterations, then is
//! timed `iterations` times; the median is the headline number (robust to
//! the odd scheduler hiccup), min and max show the spread.

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use rustlite_fonts::FontContext;
use rustlite_paint::{Color, DisplayList};
use rustlite_renderer::{Framebuffer, rasterize};

/// Viewport for layout and raster (CSS px = device px).
pub const VIEWPORT: (u32, u32) = (1280, 800);

/// A named HTML document to benchmark.
#[derive(Debug, Clone)]
pub struct Workload {
    pub name: String,
    pub html: String,
}

/// A synthetic article: `sections` sections, each with a heading, prose with
/// inline markup and links, a list, and a grid of styled boxes, plus a
/// stylesheet of class, descendant and attribute rules (`rules` of them).
pub fn article(sections: usize, rules: usize) -> String {
    let mut html = String::from("<!DOCTYPE html><html><head><title>Bench</title><style>\n");
    html.push_str(
        "body { font-family: serif; margin: 16px; line-height: 1.4 }\n\
         h2 { font-size: 22px; border-bottom: 1px solid #ccc }\n\
         .grid div { display: block; padding: 4px; margin: 2px; border: 1px solid #ddd }\n\
         nav a, article p a { color: #1a5fb4 }\n\
         li + li { margin-top: 2px }\n",
    );
    for i in 0..rules {
        let _ = writeln!(
            html,
            ".c{i} {{ color: rgb({}, {}, {}); padding-left: {}px }} \
             section .c{i} em {{ font-weight: bold }} [data-k=\"{i}\"] {{ margin: 1px }}",
            i * 7 % 256,
            i * 13 % 256,
            i * 29 % 256,
            i % 9
        );
    }
    html.push_str("</style></head><body><nav>");
    for i in 0..20 {
        let _ = write!(html, "<a href=\"/p{i}\">Link {i}</a> ");
    }
    html.push_str("</nav>");
    let words = vocabulary(3000);
    let mut seed = 0x5eed_u64;
    for s in 0..sections {
        let _ = write!(html, "<section id=s{s}><h2>Section {s}</h2>");
        for p in 0..4 {
            let class = (s * 4 + p) % rules.max(1);
            let _ = write!(html, "<p class=\"c{class}\" data-k=\"{class}\">");
            for w in 0..60 {
                // Squaring a uniform draw skews toward common words, roughly
                // like natural text (a few words very frequent, a long tail).
                let r = lcg(&mut seed) % 1000;
                let word = &words[(r * r * words.len() as u64 / 1_000_000) as usize];
                match w % 15 {
                    3 => {
                        let _ = write!(html, "<em>{word}</em> ");
                    }
                    7 => {
                        let _ = write!(html, "<b>{word}</b> ");
                    }
                    11 => {
                        let _ = write!(html, "<a href=\"#s{s}\">{word}</a> ");
                    }
                    _ => {
                        let _ = write!(html, "{word} ");
                    }
                }
            }
            html.push_str("</p>");
        }
        html.push_str("<ul>");
        for li in 0..5 {
            let _ = write!(html, "<li>Item {li} of section {s}</li>");
        }
        html.push_str("</ul><div class=grid>");
        for d in 0..8 {
            let _ = write!(
                html,
                "<div style=\"width: {}px\">Box {d}</div>",
                80 + d * 10
            );
        }
        html.push_str("</div></section>");
    }
    html.push_str("</body></html>");
    html
}

fn lcg(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    *state >> 33
}

/// `n` distinct pronounceable pseudo-words of 1-4 syllables.
pub fn vocabulary(n: usize) -> Vec<String> {
    const SYLLABLES: &[&str] = &[
        "ba", "con", "de", "fi", "gor", "hu", "in", "ja", "ker", "lo", "mi", "nus", "or", "pa",
        "qui", "ret", "sa", "ti", "um", "vel", "wa", "xen", "yo", "zu", "the", "and", "ing",
        "tion", "al", "er",
    ];
    let mut seed = 7_u64;
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(n);
    while out.len() < n {
        let len = 1 + lcg(&mut seed) % 4;
        let word: String = (0..len)
            .map(|_| SYLLABLES[(lcg(&mut seed) % SYLLABLES.len() as u64) as usize])
            .collect();
        if seen.insert(word.clone()) {
            out.push(word);
        }
    }
    out
}

/// The standard workload set.
pub fn workloads(quick: bool) -> Vec<Workload> {
    let mut list = vec![Workload {
        name: "sample".into(),
        html: include_str!("../../tests/pages/sample.html").into(),
    }];
    list.push(Workload {
        name: "article-50".into(),
        html: article(50, 100),
    });
    if !quick {
        list.push(Workload {
            name: "article-400".into(),
            html: article(400, 400),
        });
    }
    list
}

/// Timing of one stage.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub median: Duration,
    pub min: Duration,
    pub max: Duration,
}

/// Results for one workload.
#[derive(Debug, Clone)]
pub struct Report {
    pub workload: String,
    pub bytes: usize,
    pub elements: usize,
    pub display_items: usize,
    pub document_height: f32,
    /// (stage name, timing) in pipeline order.
    pub stages: Vec<(&'static str, Timing)>,
}

fn time<T>(warmup: usize, iterations: usize, mut f: impl FnMut() -> T) -> (Timing, T) {
    for _ in 0..warmup {
        std::hint::black_box(f());
    }
    let mut samples = Vec::with_capacity(iterations.max(1));
    let mut last = None;
    for _ in 0..iterations.max(1) {
        let start = Instant::now();
        let out = f();
        samples.push(start.elapsed());
        last = Some(out);
    }
    samples.sort();
    let timing = Timing {
        median: samples[samples.len() / 2],
        min: samples[0],
        max: samples[samples.len() - 1],
    };
    (timing, last.expect("at least one iteration"))
}

/// Runs every stage of the pipeline on `w`.
pub fn run(w: &Workload, fonts: &mut FontContext, iterations: usize) -> Report {
    let warmup = (iterations / 5).max(1);
    let viewport = (VIEWPORT.0 as f32, VIEWPORT.1 as f32);

    let (parse, parsed) = time(warmup, iterations, || {
        rustlite_html::parse_document(&w.html)
    });
    let doc = parsed.document;
    let elements = doc
        .descendants(doc.root())
        .filter(|&n| doc.tag_name(n).is_some())
        .count();

    let (cascade, stylist) = time(warmup, iterations, || rustlite_style::stylist_for(&doc));
    let (style, (styles, _)) = time(warmup, iterations, || {
        rustlite_style::compute_styles(&doc, &stylist, viewport)
    });
    // Cold: every word is shaped again, as on a first visit to a page.
    let (layout_cold, _) = time(warmup, iterations, || {
        fonts.clear_shape_cache();
        rustlite_layout::layout_document(&doc, &styles, fonts, viewport)
    });
    // Warm: relayout (resize, image arrival) reuses shaped words.
    let (layout, tree) = time(warmup, iterations, || {
        rustlite_layout::layout_document(&doc, &styles, fonts, viewport)
    });
    let mut list = DisplayList::new();
    let (paint, ()) = time(warmup, iterations, || {
        list.clear();
        rustlite_layout::paint(&tree, 0.0, 1.0, &mut list);
    });
    let mut fb = Framebuffer::new(VIEWPORT.0, VIEWPORT.1).expect("valid size");
    let (raster, _) = time(warmup, iterations, || {
        rasterize(&mut fb.canvas(), &list, Color::WHITE, fonts)
    });

    Report {
        workload: w.name.clone(),
        bytes: w.html.len(),
        elements,
        display_items: list.items().len(),
        document_height: tree.document_height,
        stages: vec![
            ("parse", parse),
            ("cascade", cascade),
            ("style", style),
            ("layout-cold", layout_cold),
            ("layout", layout),
            ("paint", paint),
            ("raster", raster),
        ],
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// A human-readable table.
pub fn format_table(reports: &[Report]) -> String {
    let mut out = String::new();
    for r in reports {
        let _ = writeln!(
            out,
            "{}: {} bytes, {} elements, {} display items, {:.0}px tall",
            r.workload, r.bytes, r.elements, r.display_items, r.document_height
        );
        for (stage, t) in &r.stages {
            let _ = writeln!(
                out,
                "  {stage:<11} median {:>9.3} ms   min {:>9.3}   max {:>9.3}",
                ms(t.median),
                ms(t.min),
                ms(t.max)
            );
        }
    }
    out
}

/// Baseline format: one `workload<TAB>stage<TAB>median_ms` line per stage.
pub fn format_baseline(reports: &[Report]) -> String {
    let mut out = String::new();
    for r in reports {
        for (stage, t) in &r.stages {
            let _ = writeln!(out, "{}\t{stage}\t{:.4}", r.workload, ms(t.median));
        }
    }
    out
}

/// Compares against a baseline. Returns lines describing each comparison
/// and whether any stage got slower than `threshold` × its baseline.
/// Stages under `floor_ms` in both runs are too noisy to judge and pass.
pub fn compare(
    reports: &[Report],
    baseline: &str,
    threshold: f64,
    floor_ms: f64,
) -> (String, bool) {
    let mut out = String::new();
    let mut regressed = false;
    for line in baseline.lines() {
        let mut parts = line.split('\t');
        let (Some(name), Some(stage), Some(Ok(base))) = (
            parts.next(),
            parts.next(),
            parts.next().map(str::parse::<f64>),
        ) else {
            continue;
        };
        let current = reports
            .iter()
            .find(|r| r.workload == name)
            .and_then(|r| r.stages.iter().find(|(s, _)| *s == stage))
            .map(|(_, t)| ms(t.median));
        let Some(current) = current else {
            continue;
        };
        let ratio = if base > 0.0 { current / base } else { 1.0 };
        let bad = ratio > threshold && current.max(base) >= floor_ms;
        regressed |= bad;
        let _ = writeln!(
            out,
            "{name:<12} {stage:<11} {base:>9.3} -> {current:>9.3} ms  x{ratio:.2}{}",
            if bad { "  REGRESSION" } else { "" }
        );
    }
    (out, regressed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn article_is_deterministic_and_scales() {
        assert_eq!(article(3, 5), article(3, 5));
        assert!(article(20, 5).len() > 5 * article(3, 5).len());
    }

    #[test]
    fn run_reports_every_stage() {
        let w = Workload {
            name: "tiny".into(),
            html: article(2, 3),
        };
        let r = run(&w, &mut FontContext::new(), 2);
        assert_eq!(r.stages.len(), 7);
        assert!(r.elements > 50, "{} elements", r.elements);
        assert!(r.display_items > 0);
    }

    #[test]
    fn baseline_round_trip_and_regression_detection() {
        let mk = |ms: u64| Report {
            workload: "w".into(),
            bytes: 0,
            elements: 0,
            display_items: 0,
            document_height: 0.0,
            stages: vec![(
                "parse",
                Timing {
                    median: Duration::from_millis(ms),
                    min: Duration::ZERO,
                    max: Duration::ZERO,
                },
            )],
        };
        let base = format_baseline(&[mk(10)]);
        assert_eq!(base, "w\tparse\t10.0000\n");
        assert!(!compare(&[mk(12)], &base, 1.5, 1.0).1);
        assert!(compare(&[mk(20)], &base, 1.5, 1.0).1);
        // Below the noise floor nothing counts as a regression.
        assert!(!compare(&[mk(20)], &base, 1.5, 50.0).1);
    }
}
