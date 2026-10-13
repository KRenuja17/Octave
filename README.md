# Octave

A native desktop music client in Rust. This early playback preview supports
YouTube Music search and experimental unsigned playback of direct AAC streams.
It includes play, pause, resume, seek, stop and retry. Track coverage varies;
requests requiring sign-in or a cipher resolver show an explicit error.

## Build

Rust 1.99.0 is pinned. Windows builds can use the GNU Rust toolchain and portable
MinGW-w64 GCC without installing Visual Studio. In the configured workspace:

```powershell
. 'D:\Octave-Research\Enter-Octave.ps1'
cargo build --workspace --locked
cargo run -p octave-desktop --locked
```

For another machine, install the GNU toolchain, put GCC's `bin` directory on the
process PATH, select `1.99.0-x86_64-pc-windows-gnu`, and set `CARGO_TARGET_DIR` to
an external build directory. No global PATH changes are required.

## Verify

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo run -p octave-desktop --locked -- --smoke-test
```

Tests use synthetic data and local HTTP fixtures; CI needs no YouTube account.
The graphical smoke test opens a native window and closes after three frames.
Startup requires no network connection, account or audio device.

Optional live probes (service responses can change):

```text
cargo run -p octave-provider --example probe --locked
cargo run -p octave-provider --example probe --locked -- VIDEO_ID
cargo run -p octave-provider --example probe --locked -- --search "SEARCH QUERY"
cargo run -p octave-player --example decode --locked -- VIDEO_ID
cargo run -p octave-player --example decode --locked -- VIDEO_ID --output
```

The provider probe tests search/resolution and a byte-range read. The decoder
probe checks AAC decoding and forward/backward seeks, including 20 minutes when
the track is long enough. `--output` additionally emits a brief audio sample and
checks pause, paused seeking, resume and stop on the default output device.
Probes do not save media, signed URLs, credentials or raw service responses.

## Code boundaries

- `octave-core`: application models and redacted diagnostics.
- `octave-provider`: unsigned search/resolution and seekable HTTP ranges.
- `octave-player`: one decode worker and a bounded PCM queue feeding rodio.
- `octave-desktop`: egui search results and playback controls.

Network and decoding work stays off the UI and audio callback. The transport
retains one 256 KiB payload block, independent of track duration. Playback queues
at most three roughly 100 ms PCM blocks plus one block being decoded. The audio
callback reads PCM only. New commands replace pending work; generation checks
reject stale audio and results. In-flight HTTP work may take up to its 12-second
timeout to relinquish the worker, while stop silences queued audio immediately.

## Preview limitations

Only direct AAC/MP4 streams (itag 140) are supported. No account login, Premium
validation, cipher/JavaScript resolver, live-stream support, automatic retries,
queue persistence or device-switch recovery is implemented. Retry re-resolves the
stream; seeking currently also re-resolves it, which adds latency. Output failure
is reported and needs a manual retry. Brief network stalls can exhaust the small
PCM queue and cause silence. This is a feasibility build, not a release candidate.

The unsigned client profile is a compatibility experiment informed by
[Tubefast](https://github.com/yigitbozyaka/tubefast); it is not an official API
contract. Provider changes remain isolated here. Sensitive values redact Debug
output, but explicit secret access must never be passed to a logger.

Windows x64 is the initial target. Licensing and any JavaScript resolver remain
decisions to settle before adopting those dependencies or distributing a release.
Research, planning, benchmarks, toolchains and scratch files belong outside this
repository in `D:\Octave-Research`.
