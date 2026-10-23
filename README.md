# Octave

A native desktop music client in Rust. The default app is the **desktop beta
preview**: live search, supported unsigned AAC playback, a persistent queue,
Windows media/tray integration and an experimental account library connection.
The earlier silent native experience prototype is available with `--prototype`.
Broader playback feasibility and authenticated playback remain deferred.

The listening UI follows YouTube Music's sidebar, search and persistent-player
layout while keeping Octave's original charcoal and sage palette. Narrow windows
use a compact navigation header. Navigation stays fixed while page content scrolls,
and each destination retains its own scroll position during the session.
Search, Queue, Account, Playlists and Settings remain
available, alongside Dark, Light and High contrast appearances. Visible songs and the current player load public YouTube thumbnails in the
background, cropped to square covers with a neutral fallback. Artwork uses a
bounded memory-only cache and never sends account cookies. The player keeps
previous/play/next and volume visible; its options menu holds stop, repeat and
shuffle. Installed Windows fonts are loaded locally and never redistributed.

Search shows a top-result card and an artwork-led Songs list. **Queue top result**
adds a song without starting playback; **Clear search** cancels pending results
and the obsolete in-flight request.
You can also launch with `--search "artist or song"` to open a search without autoplay.

## Expansion: local playlists

Open **Playlists**, enter a name, and **Save queue as new playlist**.
Browse cover cards to preview a playlist and its songs without changing the queue.
Select a saved playlist and **Load into queue** to replace the queue without
autoplay. Reorder, remove or add songs with the existing Queue controls, then
**Edit playlist → Replace with queue** to save the edits. Rename and delete
also live in **Edit playlist**. Rename uses the name field; deletion
has a separate confirmation. Up to 20 playlists of 10,000 entries are stored.

These playlists stay on this computer and do not modify your YouTube Music
account. A separate `listening.playlists.sqlite3` database sits beside the listening
database in the selected data directory. Writes run on a worker and commit
atomically; the UI only accepts changes after saving succeeds. Duplicate names
(ASCII case-insensitive) are rejected without losing the previous snapshot.
Repeated recordings use distinct queue/playlist occurrence IDs. Backup this database along with your listening database.

## Expansion: audio output selection

Open **Settings → Audio output**, choose **Refresh outputs**, select a device, then **Use
selected output**. Playback stops, the queue and current position remain, and
**Resume** starts on the selected output. Selection lasts until Quit; startup
uses **System default**. This does not change Windows settings. Enumeration runs
on a background worker and is capped at 64 devices. Refresh resets the dropdown
to System default but does not apply it until you click the apply button.

An explicitly selected device is never silently replaced with a different device.
If it disconnects, refresh/select another output and resume. Automatic hot-plug
handoff and remembered device preferences are not implemented yet.

## Expansion: sound and appearance

**Settings → Sound and appearance → Track transitions** defaults to gapless playback.
Set Crossfade to 1–12 seconds and choose **Apply transition** to save it. Applying
stops playback and retains position; Resume uses the new setting. Automatic queue
advance and repeat use prepared audio on the same output device. Manual skip and
seek remain immediate cancellation/restart actions.

A second worker prepares only the upcoming track. Each transition segment is
bounded to 2,097,152 float samples (8 MiB), plus one 100ms decode block. At 48kHz
stereo this permits a full 12-second linear crossfade; larger formats may shorten it. Short tracks overlap by at most half their length. Gains sum to
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
schema 8, preserving earlier sound/theme settings and queue occurrence identity. Back up the profile before
upgrading if you may need an older binary: older versions cannot read schema 8
and database downgrade is unsupported.

The diagnostic example supports `--list-outputs` and
`VIDEO_ID --muted-output --output-index 0` for a muted pause/seek/resume and
selected-output-to-default switch check. Indices apply to the current device
enumeration, not stable saved device identifiers.

## Account and desktop beta preview

Open **Account → Connection options** to connect a YouTube Music browser session.
The signed-out overview links to Search and local playlists; opening Account
does not start a connection. Settings groups desktop behavior, output and sound
preferences into expandable cards, with appearance controls first. The on-screen
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

**Lyrics → Load lyrics** fetches optional lyrics/source from YouTube Music
on a separate worker. Missing lyrics or request failure never controls playback.
The unsigned Rust provider requests mobile line timings, with a plain-text web
fallback on failure. Lyrics are not saved. Valid timed lines follow the consumed-audio clock.

## Listening experience

**Focus** opens large artwork and clock-driven lyrics, with a lyrics-only option.
The Queue / Lyrics / Related companion panel resizes and collapses while browsing.
Timed lines highlight only during their actual cue range and seek when clicked;
manual scrolling suspends Follow until **Resume follow**. Untimed lyrics remain
readable. Reduced motion and high contrast are saved locally.

**Home**, **Explore**, Charts, Moods & genres and catalog search browse public data
without signing in. Collections expose loaded counts and explicit paging. Pin up
to 50 collections; History retains the 200 most recent recordings actually heard
on this computer and can be cleared. These are local conveniences. Private
library and Premium playback acceptance remain separate account work.

A complete finite public playlist can be imported with cancellation and a
10,000-entry bound. Whole-playlist **True random** uses OS randomness across every
loaded occurrence. **Service shuffle batch** and **Start mix** are service-chosen
batches and do not claim to retrieve every playlist entry.

Translation stays off until you request it. Configure a LibreTranslate-compatible
HTTPS endpoint (loopback HTTP is allowed) in Settings, optionally enter an API key
for that run, then select a language and Translate in Lyrics. Original,
Translation and Both share the original cue identities. **Check server languages**
fetches only public language metadata, then the menu uses that server's choices.
Common choices are labeled until checked; custom codes remain available in Settings.
Requests batch at most 2,000 characters and preserve line order. Keys and translated
lyrics are memory-only. No default public translation service is contacted.

Discord Rich Presence is off by default. Supply your public Discord application
ID and explicitly enable it in Settings. It publishes actual playing metadata
through local Discord IPC, clears on pause/stop and reconnects in the background.
A bot token is neither needed nor accepted. Real Discord publishing has not yet
been verified with an Octave application ID.

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
  rejected by ordinary Add; use Add duplicate for another occurrence. Queue menus
  move or remove tracks, with guarded undo and drag handles in the companion panel.
- Resume, pause, seek, change volume, advance, shuffle upcoming songs, or repeat
  one song/the queue. Continuous playback controls automatic advance; preload is
  independent. Start mix explicitly requests a service recommendation batch.
  Removing the current song stops playback.
- Queue, current position, volume and repeat mode save atomically to SQLite every
  two seconds and on clean exit. Restart restores them **without autoplay**.
  Shuffle order is saved exactly as displayed. Maximum queue size: 10,000 entries.
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
add bounded outgoing tail, incoming head and mixed PCM buffers, each at most
8 MiB (plus a 100 ms block), supporting up to 12 seconds at 48kHz stereo.
Whole tracks are never buffered. The audio
callback reads PCM only. New commands replace pending work; generation checks
reject stale audio and results. Provider HTTP uses one shared Rust/Tokio runtime,
with four media and eight metadata request slots. Each complete request/body has
an absolute 12-second budget (artwork: five seconds). Playback generation changes
abort pending resolver/range futures at a 20ms cancellation poll; Stop also
silences queued PCM immediately. Player snapshots read the output's consumed-sample
counters even while decoding/network work waits. An empty output queue shows
**Buffering**, leaves the playhead/lyrics at consumed time, and does not add an
unheard seek offset to listening history. Recovery keeps the final consumed
position and a pause made during the stall. DNS has at most two blocking runtime threads;
an OS lookup may finish later, without holding the caller or creating a runtime
per retry. Account requests retain explicit headers and revocation checks;
no cookie jar or default credentials are installed on the shared client.

`cargo run -p octave-player --example transitions --release --locked -- VIDEO_ID`
runs an opt-in muted gapless/crossfade probe by repeating an accessible track from
near its end. It also checks pause, paused seek and stop after transition. Offline
tests verify exact PCM joins, fade curves, short-track bounds, format fallback,
missing preparation, queue edits, repeat state and preference migration.

## Playback limitations

During playback, the transport shows **AAC**, the service-reported bitrate (or
“bitrate not reported”), and the decoder's sample rate/channel count. These values
follow the active track across gapless/crossfade transitions, and clear during a
new load or Stop. They describe the selected unsigned stream; they do not imply
Premium access, lossless audio, a measured bitrate, or the hardware output format.
The resolver selects the highest reported bitrate among direct audio/MP4 formats
that declare the supported AAC-LC codec. Unexpected codecs are unsupported.

Only supported direct AAC/MP4 streams are playable. No authenticated playback, Premium
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

For a repeatable account-free pipeline workload, build optimized player tests
with `cargo test -p octave-player --release --no-run --locked`, then pass the
reported test executable to `packaging/Measure-Playback.ps1 -TestExecutable PATH
-OutputDir EXTERNAL_DIRECTORY`. This runs the ignored `one_hour_pipeline_soak`
test with generated 48 kHz stereo PCM, EQ, a five-second retained tail, and the
real playback worker/output queue. It records process memory and CPU. Execution
is accelerated and emits no audio; real-time network AAC acceptance remains separate.

## Now playing and lyrics

Use **Now playing** in the bottom player for larger artwork and song details.
The listening view has **Now playing**, **Up next**, and **Lyrics** tabs; switching
tabs does not start playback or change the queue. Queue navigation opens Up next.
Lyrics load only when requested and show their source. Timed recordings highlight
the active line using the same consumed-audio clock as the player.
The provider retains service-supplied line cues when valid, along with exact track
identity and attribution. Missing, overlapping, reversed, or malformed timings
remain plain text; no timestamps are estimated. Click a timed line during playback or pause to seek without changing play intent.
Follow centers new active lines; wheel/drag, paging and menus suspend it. Use
**Resume follow** to return. Plain recordings retain the ordinary reading view.
Unavailable lyrics can be retried; changing songs discards the old song's text.
The lyrics page scrolls as one reading surface, with attribution beside the header
and responsive text sizes. Reloading retains the current text if the request fails.
Listening tabs stay visible while reading, with **Back to top** available in Lyrics.
Reading positions are separate for each song; a new song starts at the beginning.
Track changes cancel pending lyrics/Related requests even while those views are
hidden. Catalog navigation, playlist cancellation and window cleanup also abort
their operation-scoped HTTP work without canceling another operation.
Compact lyrics use a smaller song header to leave more room above the fixed player.

To explicitly verify the unsigned lyrics worker and panel without audio, run
`cargo test -p octave-desktop --locked live_unsigned_lyrics_reach_the_panel -- --ignored --nocapture`.
This opt-in check depends on live song availability and prints counts, not lyrics.

To probe actual timestamp availability, run
`cargo test -p octave-provider --locked live_unsigned_timed_lyrics -- --ignored --nocapture`.
This stricter feasibility check requires real cues on at least two of seven public
recordings. On 22 October 2026 it passed: `hpSrLjc5SMs` returned 40 timed lines
(0–222950 ms), and `6Zt4P1scOwE` returned 51 (250–271640 ms), with attribution.
The five original music-video examples returned text without cue ranges. Coverage
depends on the exact recording; Octave preserves that identity and never substitutes
timings from a different version of a song. Extra visitor/mobile context did not
change those results in the tested requests. No sign-in or audio playback was used.
The deterministic fixture/fallback tests pass independently of this service gate.
To verify actual cues reach the desktop worker and its timing-status label, run
`cargo test -p octave-desktop --locked live_unsigned_timed_lyrics_reach_the_panel -- --ignored --nocapture`.
For native visual verification, use an isolated restored queue profile with
`--data-dir EXTERNAL_PROFILE --page lyrics --capture EXTERNAL_IMAGE.png --capture-load-lyrics`.
The last flag explicitly requests lyrics once for the current song; it only applies
in capture mode. Normal navigation fetches lyrics only when the optional auto-load preference is enabled.


### Focus and listening panel

**Focus** opens a large-artwork lyrics scene; **Lyrics only** prioritizes reading.
Compact windows keep a small song header and the same bottom player. Escape or
**Leave Focus** restores browsing without changing playback. **Side panel** opens
Queue / Lyrics / Related alongside browsing in wide windows; narrow windows use
queue/lyrics destinations. Drag the divider or use **Narrower/Wider panel**.
Back/Forward store destinations without replaying listening actions.

Panel width/tab/visibility, lyrics-only and Reduce motion preferences are saved
in schema 8. Older binaries reject this newer schema instead of overwriting it.
High contrast disables motion. Artwork has separate bounded 160/512px tiers
(96 small and eight large textures); no artwork work runs on the audio thread.
Lyrics have an eight-document, 15-minute memory cache. Related has eight
memory-only result sets; **Load related songs** explicitly requests unsigned
recommendations and offers queue actions. Neither failure interrupts audio.


### Complete playlists and queue entries

Public playlist import in Playlists retrieves every page before replacing the
queue. Progress distinguishes playable/unavailable entries; cancel or failure
keeps the old queue. Finite PL/OLAK playlists are supported, up to 10,000 entries;
oversized sources fail explicitly. **True random** randomizes the complete list
with OS entropy and unbiased Fisher–Yates, choosing a random opener. Local
playlists offer the same mode. Upcoming shuffle preserves played/current entries;
**Restore queue order** restores surviving upcoming originals, then later additions.
This is separate from YouTube's service shuffle and recommendations.

Each queue occurrence has its own durable ID. Use **Add duplicate** for intentional
repeats; ordinary Add to queue still prevents accidental duplicates. The companion
queue supports a drag handle/drop marker, keyboard menu actions, Play next and
removal/undo. Undo is available until the next queue edit. Neither undo nor view
navigation starts playback. Local playlist schema 2 preserves duplicates and adds
filter/sort presentation without changing saved source order.


### Transition status and continuous playback

Settings separates Continuous playback, Prepare the next song, and Gapless /
Standard crossfade. Blend length supports 1–12 seconds; existing gapless defaults
survive migration. At unusual rates/channel counts, an 8 MiB per-segment PCM ceiling
can shorten the fade. Short songs retain at least half their ordinary audio.
Preparing / Ready / Mixing / Natural transition / preparation failure comes from
player state. Mixing progress uses actual consumed incoming samples, freezes on
pause, and is invalidated on seek/skip/output changes. Format mismatches keep a
natural boundary. No adaptive mixing is implied by the visual status.

**Load lyrics when the song changes** is optional and off by default. It reuses
exact-ID memory results or requests the new document while a lyrics scene is open.
Translation will still require its own explicit request.


### Optional translation and Discord

Settings → Optional integrations accepts a LibreTranslate-compatible HTTPS endpoint
(or loopback HTTP for a local server). No service is selected automatically. API
keys are masked and kept only in memory; the endpoint and target language are
saved. **Translate via LibreTranslate** explicitly sends lyric text to that service.
Original / Translation / Both keep original cue identity; unexpected line counts,
old-song responses and failures preserve the original text. Batches contain at
most 24 lines / 4,000 characters, with bounded response size and deadlines. Service
language coverage, terms and limits are those of your configured endpoint.
See the [LibreTranslate API](https://docs.libretranslate.com/api/operations/translate/).

Discord Rich Presence is off by default and needs Octave's public application ID
and a running Discord desktop client. No bot token is used. Enabling shares current
track/artist/public artwork and elapsed time over local IPC; pause/stop clears it.
IPC work is separate from playback, bounded by time/packet limits, coalesced and
retried after disconnect. Configuration without an application ID never opens IPC.
Only fixture transport acceptance has run; real Discord/endpoint acceptance remains
pending configuration. Idle UI repainting is reduced; artwork/lyrics/motion do not
force continuous idle animation.

### Adaptive analysis experiment

`cargo run -p octave-player --example adaptive_analysis --release --locked`
benchmarks generated PCM silently. With two local file paths it analyzes up to
60 seconds of outgoing tail and incoming intro through the Rust decoder. No
network, audio output, new runtime or model is needed. It keeps at most 6,000
10ms energy bins per window and supports cancellation. Periodic onset estimates
propose a confidence-scored tempo/energy candidate; uncertain/incompatible pairs
return a natural transition decision. This research module does not change the
production player or enable an Adaptive mode. Its source-time mapping fixtures
cover skipped intros, repeated occurrences and stretching. Real music, vocals,
phrases, musical downbeats, bass handoff and controlled listening remain future
validation/DSP work; a synthetic pulse result does not prove those capabilities.

Native video is deferred until a supported video resolver/decoder and a shared
clock/audio owner are proven. The listening release remains audio-first, with no
embedded browser, FFmpeg runtime or Visual Studio requirement added by this work.
