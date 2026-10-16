# HTML-001: HTML parsing and malformed-markup recovery

- **Reference browser:** Chromium 152.0.7977.130 (the Claude desktop app's built-in
  browser pane; UA recorded in `tests/html/oracle.txt`)
- **OS:** Windows 11 Pro 10.0.26200
- **Observation tool:** `scripts/oracle/server.py` (loopback-only) + the browser's
  `DOMParser.parseFromString(source, "text/html")`. **REA was not used** for this
  feature: a parser oracle needs only `DOMParser`, which needs no CDP session.
- **Date:** 2026-10-16

## Scenario

32 deterministic fixtures in `tests/html/*.html` covering: implied
`html/head/body`, misnested formatting (adoption agency), `<a>` inside a block,
`p` auto-closing, lists (`li`/`dd`/`dt`), table foster parenting, implied
`tbody`/`colgroup`, comments in every position, character references (named,
legacy without semicolon, numeric, C1 remapping, attribute exception), RCDATA
/ RAWTEXT / script data, leading newline in `pre`/`textarea`, nested headings,
nested anchors, stray end tags (`</p>` → `<p></p>`, `</br>` → `<br>`), Noah's Ark
clause, nested forms, head elements after `</body>`, attribute case folding and
duplicates, whitespace around structure, nested `button`/`nobr`, `<image>` →
`<img>`, quirks vs standards `<table>` inside `<p>`, frameset, plaintext, EOF in
comment and tag, Sinhala/emoji/combining text, `li` inside `div`, table inside a
cell inside formatting, `<body>` attribute merging, legacy doctype.

## Direct observations

- Expected trees (`tests/html/*.tree`) are the browser's output, serialized in
  html5lib-tests format with attributes sorted by name.
- Document modes (`document.compatMode`) per fixture are in `tests/html/oracle.txt`.

## Result

RustLite matches **32/32** trees exactly and every document mode
(`BackCompat` ⇔ quirks; `CSS1Compat` ⇔ no-quirks or limited-quirks).
Verified by `crates/html/tests/golden.rs`; the harness was checked to fail on an
injected mismatch.

## Inferences

- Matching DOMParser output implies matching the scripting-disabled parse.
  Navigated documents with scripting enabled differ for `<noscript>`.

## Unknowns / not covered

- Template contents, SVG/MathML foreign content, customizable `<select>`
  (Chromium's relaxed select parser) — deliberately not in the fixture set; see
  docs/compatibility.md.
- Script data escape states (`<script><!--…`).
- The full named-character-reference table.
- Encoding sniffing (input is assumed UTF-8).

## Implementation status

Tested; Verified on reference browser for the fixtures listed above.
