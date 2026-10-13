# Octave

A native desktop music client in Rust. The foundation opens a local window;
YouTube connectivity and playback are planned for the next milestone.

## Build

Rust 1.99.0 is pinned. For Windows without Visual Studio, install the GNU Rust
toolchain and place a portable MinGW-w64 GCC `bin` directory on the process PATH.
Select it with `cargo +1.99.0-x86_64-pc-windows-gnu`.

Keep build artifacts outside this repository:

```powershell
$env:CARGO_TARGET_DIR = 'D:\Octave-Research\build\target'
cargo +1.99.0-x86_64-pc-windows-gnu build --workspace --locked
cargo +1.99.0-x86_64-pc-windows-gnu run -p octave-desktop --locked
```

## Verify

Using the same selected toolchain, run:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo run -p octave-desktop --locked -- --smoke-test
```

The smoke test opens a real native window and closes after three UI frames.
It requires an interactive desktop and graphics driver. Startup requires no
network connection, account or audio device. CI builds and checks on Windows;
the graphical smoke test is run locally.

## Code boundaries

- `octave-core`: application models and typed diagnostics.
- `octave-provider`: transient stream results; network implementation follows in M1.
- `octave-player`: playback state; audio implementation follows in M1.
- `octave-desktop`: egui shell and future desktop integration.

Sensitive values redact Debug output. Explicit credential access must never be
passed to a logger; this wrapper is not a general-purpose string sanitizer.

Windows x64 is the initial target. Licensing and any JavaScript resolver remain
decisions to settle before adopting those dependencies or distributing a release.
Research, planning, benchmarks and scratch files belong outside this repository.
