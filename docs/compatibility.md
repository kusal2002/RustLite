# Compatibility

## Platform matrix

| | Windows | Linux | macOS |
|---|---|---|---|
| Rust target | `x86_64-pc-windows-msvc` (verified) | `x86_64-unknown-linux-gnu` | `aarch64-apple-darwin`, `x86_64-apple-darwin` |
| Build prerequisites | MSVC Build Tools + Windows SDK | gcc/clang; X11 and/or Wayland libs at **runtime** (loaded via dlopen) | Xcode command-line tools |
| Windowing | winit (Win32) | winit (X11, Wayland) | winit (AppKit) |
| Presentation | softbuffer (GDI) | softbuffer (X11 SHM / Wayland SHM) | softbuffer (CoreGraphics) |
| GPU backend | Not used yet (planned wgpu: DX12/Vulkan) | Planned wgpu: Vulkan/GL | Planned wgpu: Metal |
| Font discovery | Not implemented | Not implemented | Not implemented |
| Certificate store | Not implemented (planned rustls + platform verifier) | Same | Same |
| Shortcut modifier | Ctrl | Ctrl | Cmd |
| Validation | ✅ build, tests, window | ❌ unverified (CI defined) | ❌ unverified (CI defined) |

## Web platform

Nothing yet: only `about:blank`. HTML, CSS, layout, text, networking and
JavaScript compatibility will be tracked here per feature, with links to tests.
