# Release candidate evaluation

Octave is MIT licensed. This Windows x64 GNU build is an unsigned evaluation
candidate. It is not yet approved for public distribution. No Visual Studio is
required. This candidate keeps the audio-first scope; native video and production
Adaptive mixing are deferred. Extract the ZIP and run `octave-desktop.exe`, or run the included
`Install.ps1` with PowerShell for a per-user installation. Quit before upgrading.
The installer verifies package checksums before staging the replacement.
Checksums alone do not authenticate a publisher; obtain packages from a trusted
source. `BUILD-INFO.json` records the source commit, dirty state, compiler, lockfile
hash and whether the package script invoked a build. A skipped build cannot be
attributed to that source. These records do not assert byte-for-byte reproducibility.

## Recovery

Upgrades retain the old installation in a sibling `.Octave-backup-...` directory.
A caught activation failure restores it. Old package-owned files are removed from
the new installation; unrelated files and the separate listening profile remain.
Power loss, registration errors and application startup failure are not covered by
automatic rollback. To recover after quitting Octave, move the failed installation
aside and rename the retained backup to `Octave`; rerun its installer from a
separate extracted copy if shortcut/uninstall registration needs repair. Keep
backups until the upgraded application has been tested. Never remove your listening
profile as part of application rollback. Database downgrades are not implemented.
The listening update migrates older profiles transactionally to schema 8, retaining
queue order/occurrences, position and sound settings, and adding view preferences,
history and pins. Back up the profile before upgrading if an older binary may be
needed; older binaries reject schema 8. Local playlists use a separate database
and migrate to schema 2 to preserve repeated recordings. Downgrades are unsupported.

## Signed update groundwork

`cargo run -p octave-update --locked -- PUBLIC_KEY_HEX_FILE METADATA_JSON SIGNATURE_HEX_FILE INSTALLED_SEQUENCE [DOWNLOAD_DIRECTORY]`
verifies an Ed25519 signature over the exact metadata bytes before parsing them.
The key is 32 bytes of hex; the detached signature is 64 bytes of hex. Metadata:

```json
{"schema":1,"product":"Octave","target":"x86_64-pc-windows-gnu","version":"0.1.0-rc.1","sequence":1,"expires_unix":1790000000,"url":"https://example.invalid/Octave.zip","length":1234,"sha256":"64 lowercase hexadecimal characters"}
```

The example is illustrative, not a usable feed. Expiry is Unix seconds, must be
in the future and at most 30 days away. Sequences must exceed the installed floor.
Provision the public key independently of the downloaded metadata. The caller must
maintain the installed sequence securely and supply a trusted, private download
directory with one download at a time. The helper does not persist that floor,
rotate keys, install or execute payloads. No production key/feed is configured and
the desktop does not automatically check for updates. Interrupted payloads resume;
only exact length and SHA-256 verified files are promoted from partial downloads.

## Limitations and remaining release gates

The implemented playback contract is deliberately narrow: unsigned supported direct AAC-LC/MP4
streams, choosing the highest reported supported offered bitrate, with typed failures for restricted/ciphered/unavailable
responses. Reported quality is shown in the transport. No embedded JavaScript
resolver has been adopted; broader service compatibility is an unresolved
feasibility decision, not an implied capability of the Rust-only implementation.

Account-free evidence includes controlled range failures, queue recovery, exact
transition PCM tests, generated long-media checks, and active fixture browsing.
Injected transport/device failures verify error handling without disconnecting
the host network or hardware. These checks complement the physical/live gates below.

- Authenticated playback and broader playback feasibility remain deferred. Account
  endpoints need a real user-initiated session test; credentials stay in memory.
- Physical media keys/tray interaction, output-device changes, actual network
  interruption, screen-reader tasks and a fresh Windows machine need acceptance.
- One repeated-recording live hour now passes; representative catalog and
  broader listening reliability remain open.
  A real-time hour on the default output at zero player volume completed from
  source `54d9405`, with sixteen gapless repeats of one accessible recording.
  It failed the predeclared consumption threshold: 3,580.859 seconds observed
  during a 3,600.020-second window, with 17.916 seconds of sampled buffering.
  Stop's Idle check passed before the failure was reported. There was no
  mid-run terminal playback error or automatic retry. Sampled peak private memory
  was 22.19 MiB for the standalone player process; this excludes the desktop UI.
  The refill regression is fixed in `8794d2b`: sample-counted output targets two
  seconds of ordinary unconsumed PCM, capped at 8 MiB, with bounded decoder and
  current-block overhead. Four real loopback range refills delayed 500 ms caused
  122 empty callback reads before the fix and zero afterward. Queue bounds,
  cancellation, consumed clocks and the full 12-second fade pass offline.
  Two matched live retests on that clean source retained the same recording,
  default output, zero player volume, no sign-in/retries and original two-second
  allowance. Neither reached an hour: the first stopped after 1,286.055 seconds
  of process time on next-track preparation Timeout; one manual confirmation
  stopped after 299.129 seconds on an active-stream Timeout. Their last minute
  checkpoints, at 1,260 and 240 seconds respectively, recorded no buffering.
  Those partial checkpoints do not establish zero buffering over the complete
  attempts or a successful live hour. Both failed results are retained; no
  timeout extension, recording substitution or retry-until-pass was used.
  `4208617` adds one fresh-connection replay for unsigned visitor/player reads
  and strict media ranges after Network/Timeout. A one-second initial attempt
  and its reconnect share the original 12-second whole-transaction deadline.
  Partial bodies are discarded. HTTP errors, invalid responses/ranges,
  cancellation, credentials, account and optional-service traffic are not
  replayed. Real socket fixtures verify recovery, cancellation and the shared
  deadline; 141 offline all-target tests and strict checks pass.
  A subsequent matched run from clean `4208617` passed: 3,600.013 seconds wall
  time, 3,599.847 seconds conservatively observed, zero sampled buffering,
  sixteen gapless handoffs, stable reported AAC 129 kb/s / 44.1 kHz stereo,
  and Stop returning to Idle. It used default output at zero player volume,
  no sign-in, no playback restarts and the unchanged two-second allowance.
  Peak private memory was 22.72 MiB for the standalone player, excluding UI.
  Opt-in private diagnostics recorded 489 successful HTTP transactions and no
  reconnect attempts. The earlier remote timeout cause remains unproven; this
  healthy hour did not exercise recovery, which is covered by socket fixtures.
  A later predeclared 13-recording breadth matrix from clean `25aa803` resolved
  only the control. Its three 30-second default-output, zero-volume windows
  at 0, 106 and 178 seconds passed with no buffering, stable AAC quality and
  Stop returning to Idle. Twelve other entries returned BotCheckRequired before
  audio was offered, including both finite long classical recordings. Therefore
  no continuous long-recording hour ran and catalog acceptance remains open.
  No failed row was replaced or retried to pass. The completed investigation
  is not a successful capability matrix, a population estimate or proof that
  the recordings inherently require an account. Natural EOF and full-recording
  playback were not tested by the short seek windows.
  A single hour-long recording, broader access and physical sound/lyric latency
  remain unverified.
  A subsequent Rust-only investigation tested the newer TVHTML5/Samsung device
  description with bounded, memory-only public guest state. It did not unlock
  any of five blocked references, including both long recordings. Its initial
  homepage redirect is retained; one Web-bootstrap/TV-player follow-up completed
  without following redirects or submitting consent/challenges. No TV fallback
  or production access fix was adopted. A fresh visionOS comparison still
  offered direct AAC only for the control. These are metadata observations,
  not accepted playback or proof of an exact missing attestation token.
  A second run of the unchanged 13-row matrix from clean `1ea6798` completed
  with the same twelve BotCheckRequired results, including both long sources.
  The control passed three default-output, zero-volume 30s windows at offsets
  0/106/178s: 90.021s total wall time, stable AAC quality and Stop-to-Idle.
  Start/late each reported one 20ms buffering event; middle reported none.
  The original consumption allowance passed, but this is not zero buffering.
  All 89 HTTP transactions succeeded, with no reconnect; service access denial
  remains distinct from network failure. No long hour could start, no recording
  was substituted, and catalog/long-source acceptance stays open. Diagnostic
  fixtures bring validation to 145 offline tests with strict Clippy/format checks.
  Further test-only investigation at `97e3d2e` compared ordinary content
  acknowledgement fields and HTML5 playback context against matched minimal
  visionOS requests. All fifteen requests for five blocked references still
  returned bot checks with zero audio formats; three control responses offered
  AAC. Both long references stayed blocked under all three variants. Public
  embed pages provided logged-out configuration but no supported inline player
  shape; those initial parser observations remain separate. Six follow-up
  embedded-player requests using page-supplied context/host flags returned
  Unavailable/ERROR with no audio, including the control. No fabricated embed
  host or script execution was used. No working path emerged, so the requested
  conditional playback matrix was not rerun and no output device was opened.
  The audited rustypipe-botguard 0.1.2 helper embeds Deno/V8 JavaScript; it was
  neither installed nor executed. No runtime/dependency/license change was
  adopted. 146 offline tests and strict checks pass; production unsigned access,
  catalog acceptance and the continuous long-recording hour remain unresolved.
  A subsequent pure-Rust attestation audit pinned eight repositories and verified
  71 selected source files. The relevant native reCAPTCHA parser omits token
  generation; the newer RustyPipe fork still embeds Deno/V8. Other matching
  projects use external helpers or address unrelated bot protection. Cold-start
  placeholders do not provide completed attestation or sustained playback proof.
  No working native generator was identified or integrated. The conditional
  zero-volume matrix was not triggered; no token/player request or output device
  was used in this source-only audit. Documentation records the requirement for
  fresh signed-out proof and real media validation before playback acceptance.
  Production code/dependencies are unchanged; existing bot-check and long-source
  gates remain open. Source/hash verification and diff checks passed; application
  tests were not rerun for these documentation-only changes.
  The silent accelerated pipeline workload now uses the maximum 12-second tail;
  a separate output-queue fixture verifies the full 12-second Standard overlap.
  These generated checks do not establish an hour of live AAC playback. A bounded
  device-free network workload additionally repeats one real unsigned recording
  through AAC decoding, preparation, EQ, the full fade and consumed-clock queue.
  Its accelerated sample-hour does not establish a real-time hour, a long single
  recording, representative catalog access, or physical audio quality.
  A bounded five-recording unsigned probe decoded and sought one recording successfully;
  four initially returned SignInRequired. A four-client follow-up identified
  bot-verification reasons on all four, with no formats offered. A broader ten-ID
  decoder sample passed one recording; nine returned BotCheckRequired. Octave now
  distinguishes those checks from ordinary sign-in requirements and does not
  automatically retry either. Matching endpoint Origin and User-Agent also left
  access unchanged. No client fallback or extra runtime was adopted. Unsigned
  lyrics availability does not establish audio access; broader unsigned resolver
  compatibility remains a feasibility gate on this network.
  Fresh anonymous page bootstrap also failed to broaden access: three isolated
  guest sessions kept page-issued cookies and server visitor updates coherent,
  but all four blocked IDs still returned bot verification. The tested Topic's
  watch-page JSON was blocked too. Those probes are test-only; production has no
  page bootstrap or automatic cookie handling. No sign-in or page-script execution
  was used. Anonymous session initialization alone has no demonstrated coverage
  benefit here.
- Production signing, update-key custody, hosting, license obligations (including
  bundled font and MPL dependency requirements), and tagged-source reproduction
  must be settled before distribution. The generated dependency inventory includes
  a workspace superset. Packaging now requires notice texts for every listed crate,
  with exact-revision, hash-checked supplements where archives omit them.
  `MPL-SOURCES.zip` contains the unmodified locked MPL crate sources and license.
  This closes the missing-text gap; it does not replace final distribution review.
- Installer rollback covers caught file activation errors, not every interruption.

## Sanitized bug reports

Maintainer package QA can be repeated with
`packaging/Test-Package.ps1 -PackageDir CANDIDATE -PreviousPackageDir BETA -WorkDir EXTERNAL_DIRECTORY`.
It verifies manifests, installs/upgrades in a unique isolated directory, launches
with only Windows on PATH, and checks uninstall/profile preservation. It uses
`-NoRegistration`; fresh-machine, shortcut and registry acceptance remain separate.

Include package version, source commit from BUILD-INFO, Windows version, UI action,
expected/actual behavior, and the visible error category. Reproduce with an isolated
`--data-dir ABSOLUTE_DIRECTORY` when possible. Never attach cookies, authorization
headers, signed media URLs, raw HTTP responses, browser data, or your listening
database. Review screenshots for account names and private library contents.
Disconnect and quit to release the memory-only account session. An in-flight
request can retain its copy until its bounded timeout expires.

## Listening-experience acceptance

### Remaining acceptance checklist (25 October 2026)

Signed-out implementation is complete for the current audio-first scope, but
release acceptance is incomplete. The following gates consolidate the outstanding
items; they do not replace or weaken the recorded criteria and failed results.

| Gate | Required input or prerequisite | Evidence required to close |
| --- | --- | --- |
| Unsigned catalog and long recordings | A demonstrated Rust-only access improvement without a JavaScript runtime, or an observed service-side change | Reuse the frozen 13-recording matrix, preserve every outcome, and pass its continuous long-source hour on default output at zero player volume. A repeated short recording does not close this gate. |
| Live lyric translation | User-selected LibreTranslate endpoint; enter any required key in the masked field | Explicit translation of timed lyrics preserves cue correspondence; target changes, cancellation and service failure leave playback usable. Review language quality separately from HTTP success. |
| Discord publishing | User-owned public application ID and an available Discord desktop session | Explicit opt-in publishes the correct recording and elapsed time; pause, stop, disable and disconnect clear or update presence according to policy. |
| Account and Premium | A separately authorized user-initiated session | Verify private library operations, offered supported audio quality and disconnect/cache isolation. Signed-out fixtures cannot close this gate. |
| Physical listening and desktop controls | An attended listening session and relevant hardware | Verify lyric/output latency, fades, pause/seek, media keys, tray and output-device changes. Zero-volume runs cannot establish audible quality. |
| Accessibility and multilingual layout | Screen reader and native visual review | Complete keyboard/screen-reader tasks and inspect RTL/mixed scripts, wrapping and focus at supported sizes/scales; repair findings before acceptance. Widget names alone are insufficient. |
| Fresh-machine distribution | A separate supported Windows machine and representative GPU/scaling | Verify launch, rendering, installation/registration, upgrade and uninstall with profile preservation. Isolated NoRegistration QA is partial evidence only. |
| Production distribution and updates | Publisher signing identity, key custody, hosting and release approval | Verify signed artifacts, trusted update metadata and sequence handling, tagged-source packaging and notices before publication. No production key or feed is configured. |

Service-specific Workout/Party/Focus tuner variants remain unsupported by the
verified unsigned contract. Native video and production Adaptive mixing remain
explicitly deferred beyond this candidate. These are not completed features.

The latest catalog evidence remains one playable control and twelve
BotCheckRequired results, including both long sources. The native attestation
audit found no complete qualifying generator. Do not rerun the conditional matrix
or mark catalog acceptance complete solely because a source audit finished.

For every subsequent check, retain the source commit, actual executable/package
provenance, environment, predefined criteria and full outcome in the external
research directory. Record failures as failures; promote individual gates only
with their required evidence. No public release is approved by this checklist.

### Implemented behavior and verification limits

Clock-driven line lyrics, Follow/manual scrolling, Focus, companion panels,
complete public playlists, local history/pins, unsigned catalog and longer
standard fades have offline fixture coverage. Translation HTTP and Discord IPC
fixtures pass; real configured LibreTranslate and Discord application/desktop
acceptance remain open. No default translation server or Discord application ID
is supplied. API keys/session credentials remain in memory.

Translation and language checks are cancellable in flight. Track/target changes
cancel obsolete translation work even when the lyrics view is hidden. Changing
the service clears its previous memory-only API key and cached translations.
Optional-service requests have separate capacity from playback/catalog requests.

The adaptive module is an isolated local analysis experiment, not production
adaptive audio. Native video is deferred. Account/Premium, screen-reader/RTL,
physical audio/device latency, fresh-machine/GPU/scaling and signing acceptance
must remain explicit before a public listening-experience release. Headless
frame measurements and silent native captures do not establish those outcomes.
