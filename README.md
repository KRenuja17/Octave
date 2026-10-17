# Octave

A native desktop music client in Rust. The default app is the **desktop beta
preview**: live search, supported unsigned AAC playback, a persistent queue,
Windows media/tray integration and an experimental account library connection.
The earlier silent native experience prototype is available with `--prototype`.
Broader playback feasibility and authenticated playback remain deferred.

## Expansion: local playlists

Open **Local playlists**, enter a name, and **Save queue as new playlist**.
Select a saved playlist and **Load into queue** to replace the queue without
autoplay. Reorder, remove or add songs with the existing Queue controls, then
**Replace with queue** to save the edits. Rename uses the name field; deletion
has a separate confirmation. Up to 20 playlists of 500 unique tracks are stored.

These playlists stay on this computer and do not modify your YouTube Music
account. A separate `listening.playlists.sqlite3` database sits beside the listening
database in the selected data directory. Writes run on a worker and commit
atomically; the UI only accepts changes after saving succeeds. Duplicate names
(ASCII case-insensitive) and duplicate track IDs are rejected without losing the
previous snapshot. Backup this database along with your listening database.

## Expansion: audio output selection

Open **Audio output**, choose **Refresh outputs**, select a device, then **Use
selected output**. Playback stops, the queue and current position remain, and
**Resume** starts on the selected output. Selection lasts until Quit; startup
uses **System default**. This does not change Windows settings. Enumeration runs
on a background worker and is capped at 64 devices. Refresh resets the dropdown
to System default but does not apply it until you click the apply button.

An explicitly selected device is never silently replaced with a different device.
If it disconnects, refresh/select another output and resume. Automatic hot-plug
handoff and remembered device preferences are not implemented yet.

## Expansion: sound and appearance

**Sound and appearance → Track transitions** defaults to gapless playback.
Set Crossfade to 1–5 seconds and choose **Apply transition** to save it. Applying
stops playback and retains position; Resume uses the new setting. Automatic queue
advance and repeat use prepared audio on the same output device. Manual skip and
seek remain immediate cancellation/restart actions.

A second worker prepares only the upcoming track, including at most 5.1 seconds
of PCM. The playback worker retains at most five seconds of outgoing audio for a
linear crossfade. Short tracks overlap by at most half their length. Gains sum to
one to avoid boosting identical signals. The incoming track becomes current when
its first samples are consumed, including the overlap; its position includes that
overlap. Different sample rates/channel counts use a continuous queued join
without a fade. If preparation is unavailable or fails, the outgoing track plays
fully and normal queue advance/retry handles the next track. Source-encoded silence
and network underruns can still be audible; this is not silence removal.

Queue edits invalidate obsolete preparation. If a transition was already queued
but has not become current, changing its target restarts the current track at the
retained position to retract obsolete audio. Pausing freezes both sides of a fade;
Stop, seek, output changes and EQ changes cancel pending transitions.

Open **Sound and appearance** to adjust the 100 Hz, 1 kHz and 10 kHz EQ bands
between -12 and +12 dB. **Apply equalizer** retains the position and stops
playback; Resume applies the new settings. Positive gains automatically reduce
input gain for headroom. Flat EQ bypasses processing. Filtering runs on the
decode worker, with independent state for each channel.

Choose Dark, Light or High contrast in the same panel. EQ and appearance persist
alongside listening state. Transition settings upgrade the listening database to
schema 3, preserving earlier sound/theme settings. Back up the profile before
upgrading if you may need an older binary: older versions cannot read schema 3
and database downgrade is unsupported.

The diagnostic example supports `--list-outputs` and
`VIDEO_ID --muted-output --output-index 0` for a muted pause/seek/resume and
selected-output-to-default switch check. Indices apply to the current device
enumeration, not stable saved device identifiers.

## Account and desktop beta preview

Open **Account** to connect a YouTube Music browser session. The on-screen
instructions use an explicitly pasted Cookie header value and account index;
Octave never reads browser databases or asks for a Google password. This advanced
connection is memory-only: quit or Disconnect forgets the session. You must
connect again after restarting. No credential is saved to SQLite or a file.
An already-running HTTP request can take up to its timeout to release its copy;
late results after disconnect are ignored. Your browser stays signed in.

Library songs, liked songs, albums and playlists have explicit Load more controls,
deduplication and repeated-cursor protection. Open a collection to load its songs.
Account Like/Unlike buttons change the actual account only when clicked; visible
state changes only after YouTube confirms success. Refresh after an uncertain
network outcome. Expired/rejected sessions prompt reconnection. These unofficial
account endpoints have fixture coverage but still need validation with a real
user-initiated session. Account connection does **not** enable authenticated or
Premium playback; that feasibility work remains deferred.

On Windows, media keys/system media controls share the same player commands as
the window. Track title, artist, duration and playback progress are published to
Windows. The tray offers Show, Play/pause, Previous, Next, Stop and Quit. In
**Desktop settings**, opt into closing the window to the tray for that run;
otherwise closing quits normally. Tray failure always leaves normal close-to-quit
behavior. Quit stops playback and saves listening state.

**Lyrics → Load lyrics** fetches optional plain lyrics/source from YouTube Music
on a separate worker. Missing lyrics or request failure never controls playback.
Lyrics are not saved and are not synchronized to playback in this preview.

## Local Windows package

Using the configured GNU environment, build outside the repo:

```powershell
. 'D:\Octave-Research\Enter-Octave.ps1'
.\packaging\Build-Package.ps1 -OutputDir D:\Octave-Research\packages
```

The ZIP contains a portable executable, any required GNU runtime DLLs, dependency inventory,
checksums, and per-user `Install.ps1`/`Uninstall.ps1` scripts. Extract it, then run
the executable directly or run `powershell -NoProfile -ExecutionPolicy Bypass -File .\Install.ps1` from
that directory. Installation targets `%LOCALAPPDATA%\Programs\Octave`, adds a
Start menu shortcut and a Windows uninstall entry, and requires no administrator
rights. Uninstall removes only listed package files and preserves listening data.
Quit before upgrading/uninstalling. `-NoRegistration` and an explicit
`-InstallDir ...\Octave` support isolated packaging QA without registry changes.

Packages are unsigned local evaluation builds. A fresh-machine installation,
live-account acceptance, distribution licensing/notices and signing remain open
release gates. The checksum manifest detects accidental damage, not an untrusted
publisher. Signed update verification groundwork and recovery instructions are documented in [RELEASE.md](RELEASE.md); automatic updates are not enabled.

## Listening alpha

- Search automatically after a 350 ms pause in typing. A single worker replaces
  pending searches and discards obsolete results immediately when the query changes.
- Play a result to replace the queue; use its menu to append or play next.
  Play/add all search results with the batch controls. Duplicate video IDs are
  rejected. Queue menus move or remove tracks.
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
On macOS the default directory is `~/Library/Application Support/Octave`; on
Linux it is `$XDG_DATA_HOME/octave` (when absolute), or `~/.local/share/octave`.
Linux/macOS CI build jobs are prepared; builds and native integration on those
platforms still require verification.

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

The opt-in full-track alpha test searches a reference song, plays briefly, closes
and restores its isolated database, verifies no autoplay, explicitly resumes and
waits for completion. It uses muted output, still requires an audio device, and
takes about four minutes. Set `OCTAVE_TEST_DIR` to an external QA directory to
keep its temporary database there (otherwise it uses the OS temporary directory):

```text
cargo test -p octave-desktop --locked live_full_track_restart_and_intentional_resume -- --ignored --nocapture
```

Run this test separately from rebuilds on Windows, where a running executable
cannot be replaced. Deterministic tests cover restart restoration, transactional
rollback, queue/repeat/shuffle behavior, stale search results, bounded retry
scheduling and reconnecting HTTP ranges. A real network-loss session remains a
separate manual acceptance check.

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

The decoder probe also accepts `--eq-test` for the muted output checks and
`--full-decode` for unpaced decoding of an accessible full track. Run
`cargo run -p octave-player --example long_stream --release --locked` for an
account-free synthetic one-hour PCM decode. It generates input on demand and
checks sample count and bounded read requests; it does not validate one hour of
network AAC playback or physical output. `packaging/Measure-Release.ps1 -Active`
rotates fixture pages and scrolls for repeatable artwork/browsing measurements.

## Code boundaries

- `octave-core`: application models and redacted diagnostics.
- `octave-provider`: unsigned search/resolution and seekable HTTP ranges.
- `octave-player`: playback and next-track preparation workers with bounded PCM feeding rodio.
- `octave-desktop`: native listening UI, queue state, SQLite worker, debounced
  search worker, and the separate fixture prototype and feasibility preview.

Network and decoding work stays off the UI and audio callback. Each of the active
and upcoming transports retains one 256 KiB payload block, independent of track
duration. Ordinary playback queues three roughly 100 ms PCM blocks. Transitions
add bounded outgoing tail, incoming head and mixed PCM buffers of up to five
seconds each (plus a 100 ms block), never whole tracks. The audio
callback reads PCM only. New commands replace pending work; generation checks
reject stale audio and results. In-flight HTTP work may take up to its 12-second
timeout to relinquish the worker, while stop silences queued audio immediately.

`cargo run -p octave-player --example transitions --release --locked -- VIDEO_ID`
runs an opt-in muted gapless/crossfade probe by repeating an accessible track from
near its end. It also checks pause, paused seek and stop after transition. Offline
tests verify exact PCM joins, fade curves, short-track bounds, format fallback,
missing preparation, queue edits, repeat state and preference migration.

## Playback limitations

Only direct AAC/MP4 streams (itag 140) are supported. No account login, Premium
validation, cipher/JavaScript resolver, live-stream support or automatic
device-switch recovery is implemented. Retry re-resolves the
stream; seeking currently also re-resolves it, which adds latency. Output failure
is reported and needs a manual retry. Brief network stalls can exhaust the small
PCM queue and cause silence before recovery. An in-flight search cannot be
interrupted until its bounded HTTP request returns. Release hardening does not
remove these limitations. The original `--live-preview` remains a minimal diagnostic
window without the alpha's persistence or automatic recovery.

The unsigned client profile is a compatibility experiment informed by
[Tubefast](https://github.com/yigitbozyaka/tubefast); it is not an official API
contract. Provider changes remain isolated here. Sensitive values redact Debug
output, but explicit secret access must never be passed to a logger.

Windows x64 is the initial target. Octave is MIT licensed. Third-party distribution
obligations and any JavaScript resolver remain decisions to settle before release.
Research, planning, benchmarks, toolchains and scratch files belong outside this
repository in `D:\Octave-Research`.
