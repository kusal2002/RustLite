# Security

**RustLite loads untrusted content from the network and runs its
JavaScript. Pages run in separate, sandboxed renderer processes that can
open no files, make no network connections and start no processes
(verified on Windows 11 and in CI on Ubuntu and macOS). It is a prototype: do not rely on it for browsing untrusted
sites with sensitive data on the machine.** `--disable-javascript` turns scripting off. Design:
[ADR-0005](../research/decisions/ADR-0005-process-isolation.md) (process
split) and [ADR-0006](../research/decisions/ADR-0006-sandbox-crate-scoped-unsafe.md)
(sandbox).

## Process isolation (PROC-001)

- Each tab's page (HTML/CSS parsing, style, layout, paint, JavaScript,
  image decoding) runs in a renderer process, `rustlite --renderer`, one per
  tab per site; cross-site navigation starts a new process. The browser
  process keeps the window, UI, navigation decisions, history, network
  stack, HTTP cache and clipboard.
- IPC is stdin/stdout with length-prefixed frames (≤64 MiB). The browser
  decodes every renderer message as hostile: bounds-checked lengths, finite
  floats, display-list fonts accepted only as positions in the face list
  the browser sent, image sizes capped at 16384 px a side.
- **Brokered resources:** renderers open no files. The browser sends its
  list of installed font faces at start; while handling a request a
  renderer may ask (an *upcall*) for a face's data, a fallback face for a
  character, or a local document's stylesheet, script or image by its
  reference text. The browser answers only for faces it listed and only
  for files inside the current local document's directory (the renderer
  never learns the path); malformed upcalls kill the renderer. Network
  loads were already done by the browser.
- **Watchdog:** a renderer that does not reply within 10 s, exits or sends
  a malformed frame is killed; the tab shows a local "Page not available"
  page and the next navigation gets a new renderer. This is also the total
  time scripts can run per request (the in-process gap of Phase 9).
- `--single-process` puts pages back in the browser process (debugging
  only; no isolation).

## OS sandbox (SANDBOX-001, SANDBOX-002)

`apps/desktop/tests/renderer.rs` runs the real binary in its sandbox and
checks, after lockdown, that writing a file, reading a directory, starting
a process and connecting to a local TCP listener all fail.

**Windows** (verified locally, Windows 11):

- **Restricted primary token:** the browser creates each renderer
  suspended (`CreateProcessAsUserW`) with a token whose user and groups are
  all deny-only, whose only restricting SID is the NULL SID, with every
  privilege removed and low integrity. Access checks against files, the
  registry, devices or other processes grant nothing; Winsock cannot even
  start. A same-access warm-up impersonation token lets the loader map the
  executable and DLLs; the renderer drops it (`RevertToSelf`) before
  reading any input. Only its stdin, stdout and stderr handles are
  inherited (explicit handle list).
- **Job object per renderer** (assigned before the renderer runs): 1 GiB
  committed-memory cap, no child processes (`ActiveProcessLimit = 1`),
  killed when the browser exits or crashes, immediate exit on unhandled
  exceptions, no clipboard, desktop switching, global atoms, other
  processes' window handles, system parameters or display settings.
- **Mitigations:** no dynamic code; no DLLs from remote or low-integrity
  locations.

**Linux** (verified in CI, ubuntu-latest, CI run 39049665123): `no_new_privs`; a
Landlock ruleset that handles every filesystem right (and TCP
bind/connect on ABI 4+) and grants none; a default-deny seccomp-bpf filter
(all threads, x86_64 and aarch64) that kills the process for a foreign
architecture and allows only what a renderer uses, returning `EPERM` (not
a kill, so a missing entry shows up as a failed call) for everything else,
including every x32 call:

- stdio pipe I/O and logging: `read`, `readv`, `write`, `writev`, `close`,
  `fstat`, `poll`/`ppoll`; `fcntl` only `F_GETFD`/`F_SETFD`/`F_GETFL`/
  `F_SETFL`; `ioctl` only `TCGETS` (`isatty`);
- memory: `brk`, `munmap`, `mremap`, `madvise`, and `mmap`/`mprotect` only
  without `PROT_EXEC` (no code is loaded after lockdown; Boa has no JIT);
- threads: `clone` only with glibc's exact thread flags (no fork, `vfork`
  or namespaces); `clone3` fails with `ENOSYS` so glibc falls back to
  `clone`; `futex`, `set_robust_list`, `rseq`, `sched_yield`,
  `sched_getaffinity`, `gettid`, `getpid`, `exit`, `exit_group`; `prctl`
  only `PR_SET_NAME`/`PR_GET_NAME`;
- signals within the process: `rt_sigreturn`, `rt_sigprocmask`,
  `rt_sigaction`, `sigaltstack`, `restart_syscall`, and `tgkill` only to the
  renderer's own process id;
- clocks, sleep and randomness: `clock_gettime`, `clock_getres`,
  `gettimeofday`, `time` (x86_64), `nanosleep`, `clock_nanosleep`,
  `getrandom`.

Path-based calls (`openat`, `newfstatat`/`statx`, `unlink`, `getcwd`),
sockets, `exec`, `kill`, `ptrace` and the rest of the kernel surface are
refused by the filter, so Landlock is now the second layer for files. CI
runs the renderer tests under strace (`scripts/renderer-syscalls.sh`): in
run 39049665123 the 6 traced renderers made only `read`, `write`, `brk`, `mmap`,
`munmap`, `sigaltstack` and `exit_group` after lockdown, apart from the
self-test's probes (`openat`, `getcwd`, `unlink`, `socket`, `getppid`),
which all got `EPERM`.

**macOS** (verified in CI, macos-latest, CI run 38986502575): a
`sandbox_init` profile that denies all network operations, `exec` and
`fork`, and reading or writing any path except system library locations
(`/System`, `/usr/lib`, `/usr/share`, dyld and time-zone data,
`/dev/null`, `/dev/urandom`).

The `unsafe` code for these calls lives only in `crates/sandbox`
(`windows.rs`, `linux.rs`, `macos.rs`), one documented call per block; all
other crates forbid `unsafe` (ADR-0006).

**Gaps:**

- **Linux:** the allowed calls (notably `futex`, `mmap`, `madvise`) are
  still kernel attack surface; the allowlist runs on x86_64 in CI only
  (aarch64 is compiled and clippy-checked, not run); no memory limit; Landlock needs kernel 5.13+ with Landlock enabled, otherwise the
  renderer logs an incomplete sandbox and file access stays open.
- **macOS:** `(allow default)` with targeted denials: Mach services and
  IPC stay reachable; no memory limit.
- **Windows:** no AppContainer; the renderer can still use objects that
  grant access to everyone by null DACL, and window messages within its
  desktop are limited only by the job's UI restrictions.
- No per-renderer heap limit inside Boa: the 1 GiB job cap ends the
  renderer instead (Windows only).

## JavaScript (SCRIPT-001)

- Engine: Boa 0.22 (Rust), without `Intl`/`Temporal`
  ([ADR-0004](../research/decisions/ADR-0004-javascript-engine-boa.md)).
- Scripts see only `window`, `document`, `console`, `location.href`,
  timers, animation frames, `performance.now()` and a DOM subset. No network
  (`fetch`/XHR), storage, cookies, clipboard, file or browser-UI APIs exist
  for pages.
- Timers are bounded (10,000 active per page; at most 1,000 callbacks per
  event-loop turn) and run on the UI thread between events; `performance.now()`
  is coarsened to 0.1 ms.
- Forms submit by GET only and the resulting navigation is a page-initiated
  navigation, so the usual URL policy applies (no `file:`, `javascript:` or
  `data:` targets).
- DOM wrappers cannot be forged: the node behind a wrapper is looked up by
  object identity in Rust; objects made from our prototypes are rejected.
- The page's DOM is lent to the engine only during a call; no shared
  mutable state, no `unsafe` in our bindings.
- Limits: 10,000,000 iterations per loop and call depth 400; a stopped
  script reports an error. The renderer watchdog (10 s per request) bounds
  total script time and the job object bounds memory on Windows; in
  `--single-process` mode neither applies.
- Scripts inserted through `innerHTML` do not run; modules are not run;
  `javascript:` URLs stay blocked.
- External scripts load only over `http(s)` (or from a local document's own
  directory), are limited to 4 MiB, and are refused when served as an image,
  audio, video or CSV type.

## Network

- TLS via rustls with the operating system's trust store
  (`rustls-platform-verifier`). Certificate validation is always on; there is
  no flag to disable it, and tests never disable it. Expired, self-signed and
  wrong-host certificates are refused (verified against badssl.com, NET-001).
- Only `http:`/`https:` are fetched. Redirects are followed manually so every
  hop is scheme-checked: a server cannot redirect into `file:` or other
  schemes. At most 10 redirects.
- Timeouts (30 s total, 10 s connect), per-destination body limits (16 / 4 /
  32 MiB), a bounded loader queue (64) and a fixed worker pool (4 threads), so
  a page cannot make RustLite allocate or queue without bound.
- Stale-result protection: superseded navigations are cancelled and their late
  results ignored by id; subresources are cancelled when the document changes.
- No cookies, no credentials, no cache, no `Referer`: nothing is persisted or
  sent beyond the request line, `Host`, `Accept` and a `User-Agent`.

## Local files

- Only the user can open `file:` URLs (command line, address bar, history).
  Page links to `file:`, `javascript:`, `data:` and unknown schemes are
  blocked.
- A local document can load images and stylesheets only from its own
  directory tree (canonicalized; `..`, absolute paths and symlinks pointing
  outside are refused).
- Network documents load subresources only over `http(s)`.
- **Mixed content is blocked:** an `https:` document loads no `http:`
  subresources at all (stylesheets, images, scripts). This is stricter than
  browsers, which upgrade or allow some passive content.

## Input hardening

- `unsafe_code = "forbid"` across the workspace (dependencies such as
  rustls/ring and the image decoders do contain `unsafe`).
- HTML parsing and layout are iterative or depth-capped (block depth 200), so
  deep nesting cannot overflow the stack; parsing is linear in input size.
- Image decoding has dimension (8192 px) and allocation limits; glyph
  rasterization caps glyph size (2048 px).
- Rasterizer tolerates malformed display lists (unbalanced clips are counted).
- The error page escapes URLs and messages.

## Fuzzing

`crates/browser-core/tests/fuzz.rs` mutates the HTML fixtures (tricky tokens,
deletions, duplication, splicing, truncation) and checks that parsing yields
a consistent tree and the whole pipeline (style, layout, paint, find,
resize) never panics; it also runs random sequences of DOM binding calls.
CI runs a fixed seed (150 cases per target); 36,000 release-mode cases with
three other seeds found no failures (2026-10-16). Decoders (PNG/JPEG/GIF)
and the CSS parser alone are not yet fuzzed separately.

## Not yet done (required before RustLite is suitable for general browsing)

1. Narrowing the macOS sandbox (deny-default profile) and memory limits
   outside Windows. (The Linux seccomp filter is an allowlist since
   2026-10-17.)
2. A graceful script timeout (today a hung script costs the whole
   renderer, after 10 s) and a heap limit outside Windows.
3. CORS and CSP. Same-origin rules matter once scripts can fetch or read
   cross-origin data; today scripts cannot fetch, and cross-origin images,
   stylesheets and scripts load as they do in browsers.
4. Coverage-guided fuzzing (cargo-fuzz on nightly) and decoder fuzzing.
5. Download handling and safe file saving (downloads are refused today).
6. ~~Dependency auditing in CI~~: `cargo deny` checks advisories,
   licenses, bans and sources in the `audit` CI job (`deny.toml`). Its
   first run flagged `rustybuzz` (RUSTSEC-2026-0206) and `ttf-parser`
   (RUSTSEC-2026-0192) as unmaintained; the font stack moved to
   `harfrust`/`skrifa` (ADR-0007) and `deny.toml` has no ignores left.
