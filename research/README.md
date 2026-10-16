# Research

Evidence-backed specifications for RustLite features. See
[docs/rea-workflow.md](../docs/rea-workflow.md) for procedure.

| Folder | Contents |
|---|---|
| `specifications/` | One authored spec per feature ID (`HTML-001.md`, …) |
| `browser-features/` | Feature notes not yet formalized |
| `captures/` | Reviewed capture summaries; raw captures go in `captures/raw/` (git-ignored) |
| `screenshots/` | Reviewed reference screenshots |
| `decisions/` | Architecture decision records (ADR-NNNN) |

## Spec template

```markdown
# <ID>: <title>
- Reference browser + version / OS / REA version:
- Scenario and initial state:
- Action:
- Final state:
- Captures / screenshots:
- Direct observations:
- Inferences:
- Unknowns:
- Proposed RustLite behavior:
- Regression tests:
- Status:
```

Review every capture for credentials, tokens and personal data before committing.

## Index

- [ADR-0001](decisions/ADR-0001-cpu-raster-first.md) — CPU rasterization first, wgpu later.
- [ADR-0002](decisions/ADR-0002-font-stack.md) — Font stack: fontdb + rustybuzz (+ fontdue, superseded by ADR-0003).
- [ADR-0003](decisions/ADR-0003-own-glyph-rasterizer.md) — On-demand glyph rasterizer replaces fontdue (73.6 → 23.9 MB).
- [ADR-0004](decisions/ADR-0004-javascript-engine-boa.md) — Boa as the JavaScript engine; DOM lent to the engine, unforgeable wrappers.
- [ADR-0005](decisions/ADR-0005-process-isolation.md) — Process isolation design (proposed): renderer per site, OS sandboxes.
- [HTML-001](specifications/HTML-001.md) — HTML parsing; 32/32 fixtures match Chromium 152.
- [CSS-001](specifications/CSS-001.md) — CSS parsing, cascade, computed values; 11/11 fixtures match Chromium 152.
- [LAYOUT-001](specifications/LAYOUT-001.md) — Block and inline layout; 9/9 fixtures within 0.032px of Chromium 152.
- [IMAGE-001](specifications/IMAGE-001.md) — Images and replaced sizing; 2/2 fixtures match Chromium 152.
- [NET-001](specifications/NET-001.md) — HTTP(S), navigation, subresources; local-server tests, example.com, badssl.com.
