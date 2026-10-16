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
- No feature specifications yet (first: HTML-001, Phase 2).
