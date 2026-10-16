# Security

**RustLite now loads untrusted content from the network, but it is a
single-process prototype without sandboxing. It is not safe for browsing
untrusted sites with sensitive data on the machine.** It runs no JavaScript,
which removes most of the attack surface browsers usually have, but parsers,
decoders and layout still process attacker-controlled input in the main
process.

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

## Input hardening

- `unsafe_code = "forbid"` across the workspace (dependencies such as
  rustls/ring and the image decoders do contain `unsafe`).
- HTML parsing and layout are iterative or depth-capped (block depth 200), so
  deep nesting cannot overflow the stack; parsing is linear in input size.
- Image decoding has dimension (8192 px) and allocation limits; glyph
  rasterization caps glyph size (2048 px).
- Rasterizer tolerates malformed display lists (unbalanced clips are counted).
- The error page escapes URLs and messages.

## Not yet done (required before RustLite is suitable for general browsing)

1. Process isolation: a sandboxed renderer process per site, with the network
   and UI in a separate privileged process. An ADR is needed before Phase 9
   (JavaScript).
2. Same-origin policy for subresources and, later, scripts; CORS; CSP.
3. Fuzzing of the HTML/CSS parsers and decoders (cargo-fuzz targets).
4. Download handling and safe file saving (downloads are refused today).
5. Mixed-content rules (HTTPS pages currently may load HTTP subresources).
6. Dependency auditing in CI (`cargo audit` / `cargo deny`).
