//! Mutation fuzzing on stable Rust (no cargo-fuzz/nightly needed).
//!
//! Inputs are the repository's HTML fixtures and pages, mutated with a seeded
//! generator (insert tricky tokens, delete, duplicate, splice, truncate).
//! Each input must parse to a consistent tree and survive the whole pipeline
//! (style, layout, paint, find, resize) without panicking; random sequences
//! of DOM binding calls must never panic either.
//!
//! CI runs a fixed seed with a modest count. Explore locally with e.g.
//! `RUSTLITE_FUZZ_ITERS=20000 RUSTLITE_FUZZ_SEED=$RANDOM cargo test --release
//! -p rustlite-browser-core --test fuzz`. A failing input is written to
//! `target/fuzz-failures/` and its path is in the panic message.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

use rustlite_browser_core::Page;
use rustlite_dom::{Document, NodeId};
use rustlite_fonts::FontContext;
use rustlite_paint::Size;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn iterations() -> usize {
    env_u64("RUSTLITE_FUZZ_ITERS", 150) as usize
}

fn rng(stream: u64) -> Rng {
    // Never zero (xorshift's fixed point).
    Rng(env_u64("RUSTLITE_FUZZ_SEED", 0x5eed_1234) ^ stream.wrapping_mul(0x9e37_79b9) | 1)
}

const TOKENS: &[&str] = &[
    "<table>",
    "<tr>",
    "<td>",
    "</table>",
    "<p>",
    "</p>",
    "<b>",
    "</b>",
    "<i>",
    "<a href=x>",
    "</a>",
    "<div>",
    "</div>",
    "<ul><li>",
    "<select>",
    "<option>",
    "<template>",
    "</template>",
    "<svg>",
    "<math>",
    "<foreignObject>",
    "<frameset>",
    "<plaintext>",
    "<textarea>",
    "<title>",
    "<style>",
    "</style>",
    "<script>",
    "</script>",
    "<!--",
    "-->",
    "<!DOCTYPE html>",
    "<![CDATA[",
    "]]>",
    "&amp;",
    "&#x110000;",
    "&#0;",
    "&notin",
    "\u{0}",
    "\u{FFFD}",
    "\u{FEFF}",
    "\r\n",
    "<img src=x width=-1 height=99999>",
    "<br>",
    "</br>",
    "<nobr>",
    "<button>",
    " style='display:block;margin:-5px;width:200%;font:bold 9e9px x'",
    " style='min-width:100vw;max-height:0;border:9px double red;padding:50%'",
    " class=a id=b",
    "{",
    "}",
    ";",
    "@media screen {",
    ":nth-child(",
    "::before",
    "*",
    "~",
    ">",
    "+",
    "[x=",
    "\"",
    "'",
    "ශ්‍රී",
    "😀",
    "\u{202E}",
    "        ",
];

fn corpus() -> Vec<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests");
    let mut out = Vec::new();
    for dir in ["html", "style", "layout", "pages"] {
        let Ok(entries) = std::fs::read_dir(root.join(dir)) else {
            continue;
        };
        let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        paths.sort();
        for p in paths {
            if p.extension().is_some_and(|e| e == "html")
                && let Ok(text) = std::fs::read_to_string(&p)
            {
                out.push(text);
            }
        }
    }
    assert!(out.len() > 20, "fixture corpus not found");
    out
}

fn boundary(s: &str, rng: &mut Rng) -> usize {
    let mut i = rng.below(s.len() + 1);
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn mutate(corpus: &[String], rng: &mut Rng) -> String {
    let mut s = corpus[rng.below(corpus.len())].clone();
    for _ in 0..1 + rng.below(8) {
        match rng.below(6) {
            0 | 1 => {
                let at = boundary(&s, rng);
                s.insert_str(at, TOKENS[rng.below(TOKENS.len())]);
            }
            2 => {
                let a = boundary(&s, rng);
                let b = boundary(&s, rng);
                let (a, b) = (a.min(b), a.max(b));
                s.replace_range(a..b, "");
            }
            3 => {
                let a = boundary(&s, rng);
                let b = boundary(&s, rng);
                let piece = s[a.min(b)..a.max(b)].to_owned();
                let at = boundary(&s, rng);
                s.insert_str(at, &piece.repeat(1 + rng.below(3)));
            }
            4 => {
                let other = &corpus[rng.below(corpus.len())];
                let cut = boundary(other, rng);
                let at = boundary(&s, rng);
                s.insert_str(at, &other[cut..]);
            }
            _ => {
                let at = boundary(&s, rng);
                s.truncate(at);
            }
        }
        // Keep inputs small enough for quick iterations.
        if s.len() > 64 * 1024 {
            let at = boundary(&s[..64 * 1024], rng);
            s.truncate(at);
        }
    }
    s
}

fn save_failure(name: &str, input: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fuzz-failures");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(name);
    let _ = std::fs::write(&path, input);
    path
}

fn run_case(name: &str, input: &str, f: impl FnOnce()) {
    if catch_unwind(AssertUnwindSafe(f)).is_err() {
        let path = save_failure(name, input);
        panic!("fuzz case failed; input saved to {}", path.display());
    }
}

/// Parent/child/sibling links agree everywhere reachable from the root.
fn check_tree(doc: &Document) {
    let mut count = 0usize;
    for n in doc.descendants(doc.root()) {
        count += 1;
        let mut prev: Option<NodeId> = None;
        for c in doc.children(n) {
            assert_eq!(doc.parent(c), Some(n), "child's parent link");
            assert_eq!(doc.prev_sibling(c), prev, "sibling links");
            prev = Some(c);
        }
        assert_eq!(doc.last_child(n), prev, "last child link");
        assert!(count <= doc.node_count(), "cycle in the tree");
    }
}

#[test]
fn html_parser_survives_mutated_fixtures() {
    let corpus = corpus();
    let mut rng = rng(1);
    for i in 0..iterations() {
        let input = mutate(&corpus, &mut rng);
        run_case(&format!("html-{i}.html"), &input, || {
            let out = rustlite_html::parse_document(&input);
            check_tree(&out.document);
        });
    }
}

#[test]
fn pipeline_survives_mutated_pages() {
    let corpus = corpus();
    let mut rng = rng(2);
    // One system font context shared by every case (loading fonts per case
    // would dominate); without system fonts text is skipped, not shaped.
    let mut fonts = Some(FontContext::with_system_fonts());
    for i in 0..iterations() {
        let input = mutate(&corpus, &mut rng);
        let mut page = Page::with_fonts(Size::new(320, 240), fonts.take().unwrap_or_default());
        run_case(&format!("pipeline-{i}.html"), &input, || {
            // Script behaviour is fuzzed separately; here scripts would only
            // spend the time budget.
            page.set_javascript_enabled(false);
            page.load_html("https://fuzz.test/", &input);
            page.display_list();
            page.scroll_by(500.0);
            page.display_list();
            let _ = page.find("a");
            let _ = page.link_at(10.0, 10.0);
            page.set_viewport(Size::new(97, 1000));
            page.display_list();
            let _ = page.memory_report();
            let _ = page.subresource_requests();
        });
        fonts = page.take_fonts();
    }
}

const CALLS: &[&str] = &[
    "document.getElementById($A)",
    "document.querySelector($A)",
    "document.querySelectorAll($A)",
    "document.createElement($A)",
    "document.createTextNode($A)",
    "document.title = $A",
    "document.body",
    "$N.appendChild($M)",
    "$N.insertBefore($M, $K)",
    "$N.removeChild($M)",
    "$N.remove()",
    "$N.textContent = $A",
    "$N.innerHTML = $A",
    "$N.innerHTML",
    "$N.setAttribute($A, $B)",
    "$N.getAttribute($A)",
    "$N.removeAttribute($A)",
    "$N.classList.toggle($A)",
    "$N.classList.add($A)",
    "$N.closest($A)",
    "$N.matches($A)",
    "$N.click()",
    "$N.addEventListener($A, () => { $M.remove(); })",
    "$N.onclick = () => { throw new Error('x') }",
    "$N.parentNode.firstChild.nextSibling",
    "$N.childNodes.length",
    "$N.contains($M)",
];

const VALUES: &[&str] = &[
    "null",
    "undefined",
    "0",
    "-1",
    "1e300",
    "''",
    "'div'",
    "'click'",
    "'#a'",
    "'['",
    "'p > b'",
    "'<b>x</b><script>1</script>'",
    "'</p><table><tr>'",
    "'id'",
    "'class'",
    "'on click'",
    "({})",
    "[]",
    "'\\u0000'",
    "'a'.repeat(1000)",
];

#[test]
fn dom_bindings_survive_random_calls() {
    let mut rng = rng(3);
    // Each program runs a short sequence of calls; it is cheaper than the
    // pipeline cases, so a few more fit in the same time.
    for i in 0..iterations() {
        let mut program = String::from(
            "const all = () => Array.from(document.querySelectorAll('*'));\n\
             const pick = k => { const a = all(); return a.length ? a[k % a.length] : document.body; };\n",
        );
        for _ in 0..1 + rng.below(12) {
            let call = CALLS[rng.below(CALLS.len())]
                .replace("$K", &format!("pick({})", rng.below(50)))
                .replace("$N", &format!("pick({})", rng.below(50)))
                .replace("$M", &format!("pick({})", rng.below(50)))
                .replace("$B", VALUES[rng.below(VALUES.len())])
                .replace("$A", VALUES[rng.below(VALUES.len())]);
            program.push_str(&format!("try {{ {call}; }} catch (e) {{}}\n"));
        }
        let html = "<!DOCTYPE html><title>t</title><div id=a class=x><p>one <b>two</b></p>\
                    <ul><li>1</li><li>2</li></ul></div><a href=/x onclick='return false'>l</a>";
        run_case(&format!("bindings-{i}.js"), &program, || {
            let mut doc = rustlite_html::parse_document(html).document;
            let mut host = rustlite_script::ScriptHost::new("https://fuzz.test/");
            let out = host.run_script(&mut doc, &program, "fuzz");
            assert!(
                out.errors.is_empty(),
                "calls are wrapped in try/catch: {:?}",
                out.errors
            );
            check_tree(&doc);
            let targets: Vec<NodeId> = doc.descendants(doc.root()).collect();
            for &t in targets.iter().step_by(3) {
                host.dispatch_event(&mut doc, t, "click");
            }
            check_tree(&doc);
        });
    }
}
