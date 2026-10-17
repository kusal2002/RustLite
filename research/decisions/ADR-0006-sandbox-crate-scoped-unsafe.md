# ADR-0006: A sandbox crate with scoped `unsafe` (windows-sys)

- Status: Accepted and implemented. Windows: restricted token + job,
  verified locally. Linux (Landlock + seccomp) and macOS (`sandbox_init`):
  verified in CI (run 38986502575).
- Date: 2026-10-17 (revised the same day: restricted token, Unix sandboxes)

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
- Dependencies: `windows-sys 0.61` (`Win32_Foundation`, `Win32_Security`,
  `Win32_System_Console`, `Win32_System_JobObjects`, `Win32_System_Pipes`,
  `Win32_System_Threading`; Windows only) and `libc 0.2` (Unix only; asked
  and decided with the user on 2026-10-17 over the `landlock`/`seccompiler`
  crates, to keep the policy small and in one place). Both MIT/Apache-2.0.
- The public API is platform-neutral: `launch(exe, args, memory_limit)`
  returns the process and its stdin/stdout pipe ends; `lockdown()` reports
  a `Lockdown { filesystem, network, processes, privileges, integrity }`.
- **Windows** (the model of Chromium's sandbox):
  - The browser creates the renderer **suspended** with
    `CreateProcessAsUserW` and a **lockdown token**: user and all groups
    deny-only, the NULL SID as the only restricting SID, privileges
    removed, low integrity. Access checks grant nothing, so files, the
    registry, devices and sockets (Winsock fails to start) are closed.
  - Only the renderer's stdin/stdout/stderr handles are inherited
    (`PROC_THREAD_ATTRIBUTE_HANDLE_LIST`).
  - It is assigned to its **job object** before it runs: committed memory
    cap (1 GiB), `ActiveProcessLimit = 1`, kill on job close, die on
    unhandled exception, UI restrictions.
  - Its first thread gets a **warm-up impersonation token** (restricted to
    the user's own SIDs, low integrity) so the loader can map the
    executable and DLLs; `lockdown()` turns on the dynamic-code and
    image-load mitigations and calls `RevertToSelf`, after which only the
    lockdown token applies.
- **Linux** (`lockdown()`): `PR_SET_NO_NEW_PRIVS`; Landlock with every
  filesystem right handled (and TCP on ABI 4+) and no rule granting any;
  a seccomp-bpf denylist (synchronized to all threads) returning `EPERM`
  for sockets, `exec`, `ptrace`, cross-process memory, mounts and
  namespaces, BPF, perf, `userfaultfd`, keyrings, modules, kexec,
  `io_uring` and handle-based opens; a foreign architecture kills the
  process. No user namespaces: unprivileged ones are disabled on common
  distributions (Ubuntu 24.04).
- **macOS** (`lockdown()`): `sandbox_init` with a profile denying network,
  `exec`/`fork`, and file reads and writes outside system library paths.

## Why not AppContainer

AppContainer would also remove file and network access, but the renderer
executable (and its directory) would have to grant read and execute to the
AppContainer SIDs — changing ACLs on the user's files — and it brings
capability and profile management. A restricted token gives the same
denials with no changes outside the process. AppContainer can be layered
on later.

## Consequences

- Verified on Windows 11 by an automated test that runs the real binary in
  its sandbox: low integrity (0x1000); writing a file, reading a
  directory, starting a process and connecting to a local TCP port all
  denied; real pages (fonts, scripts, local subresources) render through
  the broker of ADR-0005.
- Linux and macOS: the same self-test passes in CI on ubuntu-latest and
  macos-latest (run 38986502575), and pages render there through the
  broker with the sandbox on.
- **Remaining gaps** (also in `docs/security.md`): the Linux filter is a
  denylist; the macOS profile allows by default and denies specific
  operations; no memory limits outside Windows.
- Reviewing `unsafe` means reading `crates/sandbox/src/{windows,linux,macos}.rs`.
