# ADR-0007: Font stack — rustybuzz + ttf-parser → harfrust + skrifa

- Status: Accepted and implemented (approved by the user 2026-10-17; the
  master specification asks before major dependency changes)
- Date: 2026-10-17
- Amends: [ADR-0002](ADR-0002-font-stack.md) (shaping and font tables)

## Context

`cargo deny` reports both font-table crates from ADR-0002 as unmaintained:

- RUSTSEC-2026-0206: `rustybuzz` (suggested replacement: `harfrust`)
- RUSTSEC-2026-0192: `ttf-parser` (suggested replacement: `skrifa`)

Both are acknowledged in `deny.toml` for now. Unmaintained is not
vulnerable, but RustLite parses untrusted font bytes (system fonts today,
web fonts later), and parser fixes will no longer ship for either crate.

Current releases (crates.io, 2026-10-17):

| Crate | Version | Released | License | Notes |
|---|---|---|---|---|
| `harfrust` | 0.14.0 | 2026-10-04 | MIT | harfbuzz org; rustybuzz's successor, tracks HarfBuzz 14.5.1; built on `read-fonts`; no `unsafe`; MSRV 1.85 |
| `skrifa` | 0.48.0 | 2026-10-03 | MIT OR Apache-2.0 | googlefonts/fontations; outlines, metrics, charmap; used by Chrome (Skia) and FreeType-replacement work |
| `read-fonts` | 0.45.1 | 2026-10-15 | MIT OR Apache-2.0 | fontations table parser; `#![forbid(unsafe_code)]` |
| `font-types` | 0.12.7 | 2026-10-15 | MIT OR Apache-2.0 | fontations scalar types |
| `fontdb` | 0.24.0 | 2026-07-29 | MIT | **No longer depends on `ttf-parser`** (vendors a minimal name/OS/2 subset); no advisory |

The fontations crates release roughly monthly; harfrust 5 releases since
August. Their MSRV (1.85) is below the workspace's `rust-version = "1.95"`.

## Decision

| Concern | Before | After |
|---|---|---|
| Discovery, CSS-style matching | `fontdb 0.24` | `fontdb 0.24` (unchanged) |
| Shaping | `rustybuzz 0.20` | `harfrust 0.14` |
| Metrics, cmap coverage, glyph bounds | `ttf-parser 0.25` | `harfrust`'s `read_fonts::model::Font` (metrics, charmap) |
| Glyph outlines for our rasterizer | `ttf-parser 0.25` (`OutlineBuilder`) | `skrifa 0.48` (`OutlinePen`, unhinted) |
| Self-referential face cache | `self_cell 1.2` | removed: `harfrust::Font` owns its data |

Crates removed from the tree: `rustybuzz`, `ttf-parser`, `self_cell`,
`core_maths`, `libm`, `unicode-ccc`, `unicode-bidi-mirroring`,
`unicode-properties`, `unicode-script`. Added: `harfrust`, `skrifa`,
`read-fonts`, `font-types` (their other dependencies — `bytemuck`,
`smallvec`, `yoke`, `foldhash`, `zerofrom`, `stable_deref_trait` — are
already in the tree or MIT/Apache/Zlib/Unicode-3.0, all in the
`deny.toml` allow-list). Neither new crate has a RustSec entry.

`fontdb` stays: it already left `ttf-parser`, and replacing it (e.g. with
`fontique`, which uses platform APIs and a different matching model) would
touch `describe_faces`/`brokered` and the renderer protocol for no audit
gain. Revisit only if `fontdb` gets an advisory.

## Use map (crates/fonts)

| Where | Today | After |
|---|---|---|
| `lib.rs` `Shaper` (self_cell of `Rc<[u8]>` + `rustybuzz::Face`) | parse once per face | `Face.shaper: Option<harfrust::Font>` built from `Blob::Shared(Arc<Arc<[u8]>>)`; no self_cell |
| `lib.rs` `load()` metrics: `units_per_em`, `global_bounding_box`, `ascender`/`descender`/`line_gap` | `ttf_parser::Face` | `font.metrics()`: `units_per_em`, `bounds`, `h_line()` |
| `lib.rs` `covers()` (primary-face coverage) | `ttf_parser::Face::glyph_index` | `font.charmap().map_unicode(c)` |
| `lib.rs` `fallback_for()` candidate scan | `ttf_parser::Face::parse` per candidate | `skrifa::FontRef::from_index` + `charmap()` (borrowed, per candidate, no copy) |
| `lib.rs` `shape_single()` | `rustybuzz::UnicodeBuffer` + `shape` | `harfrust::Buffer` + `ShaperFont::new(&font)` + `harfrust::shape` (layout data is cached inside the `Font`) |
| `lib.rs` `rasterize()` | `ttf_parser::Face::parse` per glyph miss | `skrifa::FontRef::from_index` per glyph miss (table directory only, as today) |
| `raster.rs` `impl OutlineBuilder for Accumulator` | `ttf_parser::OutlineBuilder` | `skrifa::outline::OutlinePen` (same five methods) |
| `raster.rs` `glyph_bounding_box`, `outline_glyph` | `ttf_parser` | `outline_glyphs().get(id)?.draw(DrawSettings::unhinted(Size::unscaled(), …), &mut path)` into a recorded path; bitmap sized from its control box (see implementation notes), then replayed into the accumulator |
| `FontBroker::face_data`, `FontContext::face_data` | `Rc<[u8]>` | `Arc<[u8]>` (read-fonts blobs must be `Send + Sync`); one-line change in `browser-core/src/remote.rs` (`Rc::from` → `Arc::from`) |

`FaceDescriptor`, `describe_faces`, `brokered`, `fallback_face`,
`position_of` and the IPC wire format do not change.

## Evidence (prototype, scratch crate outside the repo, Windows 11, release)

Shaping "Hello, world! office AV" and "ශ්‍රී ලංකා" with arial, times, cour,
segoeui and Nirmala.ttc (Sinhala), old vs new:

- **Glyph IDs, advances and line metrics identical** in all 10 cases
  (e.g. Arial asc/desc/gap 1854/−434/67; Nirmala Sinhala ids
  `[4258, 3, 3841, 3788, 3808, 3850]`, advance 8023 units both ways).
- **Shaping speed:** 2.8–4.5 µs vs 2.9–8.4 µs per call; 5–48 % faster.
- **Heap per loaded face:** `rustybuzz::Face` 205–267 KB; `harfrust::Font`
  1.7 KB at load, 13–19 KB after a shape (lazy layout caches). For the
  ~6–10 faces a typical page loads, **≈1.5–2.5 MB less per process**
  (browser and each renderer).
- **Binary size (stripped, thin LTO):** shaping-only 850 KB → 1,163 KB
  (+310 KB); shaping + outlines 882 KB → 1,732 KB (**≈ +850 KB**). Most of
  the outline cost is skrifa's TrueType/CFF hinting interpreters, which are
  linked because `draw` dispatches on hinting mode even when unhinted
  (disabling skrifa's default `autohint_shaping` feature saved nothing).
  Against today's 11.8 MB `rustlite.exe` that is ≈ +7 %.
- Line-metric selection: `read-fonts` `Metrics::h_line()` follows the same
  order as `ttf-parser` (USE_TYPO_METRICS → hhea → typo → win when hhea is
  zero); it decides per line rather than per field, which only differs for
  fonts with a zero hhea ascender *or* descender alone.

## Risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| Layout goldens shift | Low (identical advances in the prototype) | Run all `tests/layout` fixtures; any shift blocks the change |
| Outline decomposition differs (skrifa emits TrueType implied points differently; control-point sums differed by 1–5 %) so coverage masks change slightly | Medium, cosmetic | No pixel goldens for text; add a test that rasterizes a known glyph via both paths during the migration and checks max coverage difference, then drop it |
| Symbol-encoded fonts (Wingdings, Symbol, Webdings): skrifa/harfrust map U+0020–00FF through the (3,0) symbol cmap, `ttf-parser` did not | Low | Primary face: new behaviour matches HarfBuzz/Chromium (a `font-family: Wingdings` page shows symbols). Fallback scan: skip faces whose charmap `is_symbol()`, preserving today's fallback choices |
| harfrust errors on malformed fonts where rustybuzz returned a face | Low | `shaper: None` already means "shapes to nothing"; fallback then uses another face |
| Fast-moving 0.x APIs (monthly breaking releases) | Medium | One crate (`rustlite-fonts`) touches them; pin minor versions in the workspace; Renovate-style bumps reviewed with goldens |
| `+850 KB` binary | Certain | Accepted; pays back in per-process memory. Could later drop skrifa by drawing glyf/CFF via `read-fonts` directly (≈ −570 KB) if size matters |

## Alternatives considered

- **Keep both, acknowledged forever.** No code churn, but no parser fixes for
  untrusted font input and the `deny.toml` exceptions never close.
- **swash** (shaping + scaling in one crate, Apache/MIT, maintained): own
  shaper, not a HarfBuzz port, so less Chromium parity than harfrust;
  ADR-0002 already preferred HarfBuzz parity.
- **read-fonts only (no skrifa)**: smaller, but we would maintain glyf
  composite/variation and CFF/CFF2 charstring drawing ourselves.
- **Replace fontdb with fontique**: not needed for the advisories; larger
  change to the broker protocol.

## Implementation notes (2026-10-17)

- Implemented as mapped above. All 19 layout fixtures still match
  Chromium 152; 302 workspace tests pass; clippy `-D warnings` clean.
- Raster parity against the old ttf-parser path (same accumulator, 7
  Windows fonts, 12/16/40 px, up to 4,000 glyphs each): 70,720 of 70,734
  bitmaps pixel-identical, the rest within 5/255, except one unnamed
  Times New Roman glyph. skrifa follows FreeType and places a TrueType
  outline at its `hmtx` left side bearing, which can disagree with the
  `glyf` header box (190 units for that glyph). The first version sized
  bitmaps from `GlyphMetrics::bounds` (the header box) and clipped it, so
  the rasterizer now records the outline once and sizes the bitmap from its
  control box.
- Release `rustlite.exe`: 11,795,456 → 12,236,288 bytes (+0.42 MiB,
  +3.7 %), half the prototype's estimate.
- Missed in the proposal: `ttf-parser` also reaches the tree through
  `winit` (default feature `wayland-csd-adwaita` → `sctk-adwaita` →
  `ab_glyph` → `owned_ttf_parser`), which draws Wayland title-bar text on
  Linux. Resolved with the user: winit now uses `default-features = false`
  with `rwh_06`, `x11`, `wayland`, `wayland-dlopen` and
  `wayland-csd-adwaita-notitle`, so client-side Wayland title bars (GNOME)
  have no title text and `ab_glyph`/`ttf-parser` leave the tree.
- Primary-face coverage uses harfrust's charmap (symbol cmaps map like
  HarfBuzz); the fallback scan uses skrifa's charmap and skips faces whose
  charmap `is_symbol()`.

## Consequences

- `deny.toml` drops both ignores; `cargo deny check` passes without them.
- `rustlite-fonts` public API is unchanged except `FontBroker::face_data` /
  `FontContext::face_data` returning `Arc<[u8]>`.
- Lower per-face memory in every process; ≈ +0.85 MB binary.
- `docs/architecture.md` dependency table, `docs/progress.md` (FONT-003) and
  `CHANGELOG.md` updated; ADR-0002 status notes the amendment.
