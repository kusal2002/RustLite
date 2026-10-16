# REA workflow

REA (https://github.com/morluto/rea, https://rea.tools/) is the research and
evidence tool. It is **not** part of the browser engine.

## Readiness report (2026-10-16)

| Check | Result |
|---|---|
| Version | `rea-agents@6.5.0` via `npx` (not installed globally; `rea` is not on PATH) |
| MCP integration | Registered in Claude Code (`npx -y rea-agents@6.5.0 mcp`) and connected; browser tools available (`list_browser_targets`, `inspect_web_page`, `capture_web_screenshot`, `compare_web_screenshots`, `observe_web_session`, `capture_browser_scenario`, …) |
| `rea doctor` | `healthy: false` — Node 24.19.0 OK; native providers missing (Hopper, Ghidra, IDA, jadx); several *other* clients' registrations report `config_drift` (Claude Desktop, Codex, Cursor, Copilot CLI, Antigravity) |
| Reference browser | Google Chrome 155.0.8059.84 installed |
| Native analysis | Unavailable (no Ghidra/IDA/Hopper). Not needed for current milestones. |

The doctor failures do **not** block browser observation. Fixing the other
clients' registrations requires `rea setup`, which rewrites their configs —
not done; that is the user's call.

CLI usage without a global install:

```bash
npx -y rea-agents@6.5.0 <command> --help
```

## Browser observation procedure

1. Launch Chrome with an isolated temporary profile and loopback-only CDP:
   ```bash
   chrome --user-data-dir=<temp dir> --remote-debugging-address=127.0.0.1 --remote-debugging-port=9222 --no-first-run <fixture URL>
   ```
2. `rea list-browser-targets http://127.0.0.1:9222` → pick the exact target id.
3. `rea inspect-web-page` for DOM/resource structure.
4. `rea capture-web-screenshot http://127.0.0.1:9222 <targetId> --allowed-origins <origin>` for the baseline.
5. Perform one controlled action; `rea observe-web-session` to record it.
6. Capture the result; `rea compare-web-captures` / `compare-web-screenshots`.
7. Save raw output under `research/captures/raw/` (git-ignored) and the authored
   spec under `research/specifications/<ID>.md`.

Fixtures are served from a local static server (loopback only); no personal
browsing data or credentials are ever captured.

## Differential testing loop

Fixture → Chrome observation (REA) → RustLite output (`--headless`, DOM dump,
geometry dump) → structural comparison first, pixel comparison second →
fix → regression test. Font, DPI and rasterization differences are expected and
documented per test; screenshot similarity alone is never proof.

## Fallback

If REA is unavailable: continue from public specs and Chrome DevTools, record
"REA unavailable" in the spec, and never claim REA evidence.
