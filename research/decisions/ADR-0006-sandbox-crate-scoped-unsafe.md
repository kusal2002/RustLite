# ADR-0006: A sandbox crate with scoped `unsafe` (windows-sys)

- Status: Accepted (implemented on Windows, Phase 10)
- Date: 2026-10-17

## Context

ADR-0005 splits pages into renderer processes. A separate process only
limits damage if the operating system confines it, and every OS sandbox API
is a C API: job objects and token changes on Windows, seccomp/namespaces on
Linux, `sandbox_init` on macOS. None of them is reachable from safe Rust
alone. The workspace has `unsafe_code = "forbid"`, and the master
specification asks before major dependency changes.

Options considered (asked and decided with the user, 2026-10-17):

1. **Allow scoped `unsafe` in one small crate** and call the Win32 API
   through `windows-sys` (Microsoft's raw bindings, no runtime).
2. **A third-party sandbox crate** (e.g. `gaol`, `birdcage`): unmaintained
   or Unix-only, and it would hide the exact policy from review.
3. **Process split without an OS sandbox**: crash and hang containment only;
   a compromised renderer would still have the user's full rights.

## Decision

- New crate `crates/sandbox` (`rustlite-sandbox`) is the **only** crate
  where `unsafe` may appear. Its lints are `unsafe_code = "deny"` with a
  per-function `#[allow(unsafe_code)]`, and
  `clippy::undocumented_unsafe_blocks = "deny"`: every block calls one Win32
  function and carries a `SAFETY:` note on its arguments and handles.
  Handles are owned by an RAII `Handle` that closes them once.
- Every other crate keeps `unsafe_code = "forbid"`.
- Dependency: `windows-sys 0.61` with only `Win32_Foundation`,
  `Win32_Security`, `Win32_System_JobObjects`, `Win32_System_Threading`.
  Windows-only (`cfg(windows)`), MIT/Apache-2.0, no build scripts beyond
  import libraries.
- Policy on Windows:
  - **Browser side**: one job object per renderer — committed memory cap
    (1 GiB default), `ActiveProcessLimit = 1` (no child processes),
    kill-on-job-close (renderers die with the browser, even on a crash),
    die on unhandled exception, and UI restrictions (clipboard read/write,
    desktop, global atoms, user handles, system parameters, display
    settings, exit windows). The child is assigned right after spawn,
    before it receives any input; if assignment fails the child is killed.
  - **Renderer side** (`lockdown()`, before reading the first message):
    disable all token privileges, lower the token to **low integrity**
    (no writes to the user's files, registry or medium-integrity
    processes), forbid dynamic code (no new executable memory), and
    restrict image loads (no DLLs from remote or low-integrity locations,
    prefer System32).
- The public API is platform-neutral (`RendererJob`, `lockdown`,
  `integrity_level`, `in_job`, `SUPPORTED`); on Linux and macOS it is a
  no-op that reports "not sandboxed".

## Why not AppContainer or a restricted token now

Both need the renderer to be *started* with a new token
(`CreateProcessAsUser` / a `PROC_THREAD_ATTRIBUTE_LIST`), which
`std::process::Command` cannot do; we would have to replace process
spawning and stdio piping with our own `unsafe` code. Self-lowering to low
integrity plus the job object gets most of the write and process
protection with a far smaller `unsafe` surface. AppContainer (which also
removes read and network access) is the next step.

## Consequences

- Verified on Windows 11 by an automated test that runs the real binary in
  its sandbox: low integrity (0x1000), inside a job, writing a file
  denied, starting a process denied, reading allowed.
- **Remaining gaps** (also in `docs/security.md`): renderers can still
  read files the user can read and open network connections (our code does
  neither; the OS does not stop it); no sandbox on Linux or macOS yet.
- Reviewing `unsafe` means reading one file
  (`crates/sandbox/src/windows.rs`).
