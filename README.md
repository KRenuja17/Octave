# Octave

A native desktop music client in Rust. The default app is the **listening alpha**:
live search, supported unsigned AAC playback, and a local persistent queue.
The earlier silent native experience prototype is available with `--prototype`.
Broader playback feasibility and account access remain deferred.

## Listening alpha

- Search automatically after a 350 ms pause in typing. A single worker replaces
  pending searches and discards obsolete results immediately when the query changes.
- Play a result to replace the queue; use its menu to append or play next.
  Duplicate video IDs are rejected. Queue menus move or remove tracks.
- Resume, pause, seek, change volume, advance, shuffle upcoming songs, or repeat
  one song/the queue. Completion advances through your queue; no recommendation
  radio is requested. Removing the current song stops playback.
- Queue, current position, volume and repeat mode save atomically to SQLite every
  two seconds and on clean exit. Restart restores them **without autoplay**.
  Shuffle order is saved exactly as displayed. Maximum queue size: 500 songs.
- Transient network/time-out/selected HTTP failures re-resolve the stream and
  resume from the retained position, with at most two retries per play action.
  Sign-in/unsupported-stream failures stop for manual action. Retry and Skip are
  available without losing the queue. Stop cancels pending recovery.
- Ctrl+K focuses search; Space controls playback outside text inputs;
  Alt+Left/Right select previous/next. Lists virtualize visible rows.

Data is stored in `%LOCALAPPDATA%\Octave\listening.sqlite3` on Windows. Override
the location with `--data-dir ABSOLUTE_DIRECTORY` (also useful for QA profiles).
The database stores track metadata and settings, never signed stream URLs,
credentials, or audio. Newer unknown database versions are left untouched.
Storage failures are shown in the window; do not assume changes were saved then.

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

## Explore the prototype

Launch with `--prototype`. This fixture UI simulates playback silently, has no
account/network access, and resets its sample state on each launch.

- Browse the six original collections, search 10,000 sample tracks, or filter likes.
- Use row menus to add songs, play next, reorder or remove queue entries.
- The player simulates play/pause, previous/next, seek, shuffle, repeat and volume.
  In compact windows, volume/shuffle/repeat are inside the **Demo** menu.
- Open **Preview states** for loading, empty, error, offline and signed-out scenes.
  Their recovery buttons restore fixture content; no network request is made.
- **Ctrl+K** focuses search. **Tab / Shift+Tab** move focus; **Enter** activates
  focused controls. **Space** toggles transport outside the search field;
  **Alt+Left / Alt+Right** move between tracks.
- Native display scaling is respected. Narrow/short windows use compact navigation;
  page content scrolls while the player stays visible.

```text
cargo run -p octave-desktop --locked -- --prototype --compact
cargo run -p octave-desktop --locked -- --prototype --page album --collection 5
cargo run -p octave-desktop --locked -- --live-preview
```

The last collection includes Japanese, Korean, Greek, Cyrillic and long-title
fixtures. Windows-installed Segoe UI, MS Gothic and Malgun Gothic are loaded as
available; OS fonts are never redistributed. Other systems use egui fallbacks.

For repeatable prototype visual QA, use `--prototype --page home|search|library|album|playlist|queue|lyrics`,
`--query TEXT`, `--state loading|empty|error|offline|signed-out`, `--scale 1|1.5|2`,
and `--capture ABSOLUTE_PATH.png`. Captures wait for all artwork, save outside the
repository, then close the window. `--capture-scroll` exercises a short page's
scrolling before capture. Scale overrides are for QA; omit them for native DPI.

Original PNG artwork is embedded in the binary. One background worker decodes
and resizes it, with a two-result channel and a six-texture cache (about 2.4 MiB
of RGBA payload). Regenerate the art with the Rust `generate_artwork` example.
Track lists virtualize rows instead of drawing all 10,000 songs each frame.

Keyboard, accessible labels and input flows have automated egui coverage. A full
Narrator/NVDA listening audit and full bidirectional/complex-script support are
release-level work; the prototype is not a claim of complete accessibility.

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
- `octave-desktop`: native listening UI, queue state, SQLite worker, debounced
  search worker, and the separate fixture prototype and feasibility preview.

Network and decoding work stays off the UI and audio callback. The transport
retains one 256 KiB payload block, independent of track duration. Playback queues
at most three roughly 100 ms PCM blocks plus one block being decoded. The audio
callback reads PCM only. New commands replace pending work; generation checks
reject stale audio and results. In-flight HTTP work may take up to its 12-second
timeout to relinquish the worker, while stop silences queued audio immediately.

## Playback limitations

Only direct AAC/MP4 streams (itag 140) are supported. No account login, Premium
validation, cipher/JavaScript resolver, live-stream support or automatic
device-switch recovery is implemented. Retry re-resolves the
stream; seeking currently also re-resolves it, which adds latency. Output failure
is reported and needs a manual retry. Brief network stalls can exhaust the small
PCM queue and cause silence before recovery. An in-flight search cannot be
interrupted until its bounded HTTP request returns. This is an alpha, not a
release candidate. The original `--live-preview` remains a minimal diagnostic
window without the alpha's persistence or automatic recovery.

The unsigned client profile is a compatibility experiment informed by
[Tubefast](https://github.com/yigitbozyaka/tubefast); it is not an official API
contract. Provider changes remain isolated here. Sensitive values redact Debug
output, but explicit secret access must never be passed to a logger.

Windows x64 is the initial target. Licensing and any JavaScript resolver remain
decisions to settle before adopting those dependencies or distributing a release.
Research, planning, benchmarks, toolchains and scratch files belong outside this
repository in `D:\Octave-Research`.
