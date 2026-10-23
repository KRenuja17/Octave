# Release candidate evaluation

Octave is MIT licensed. This Windows x64 GNU build is an unsigned evaluation
candidate. It is not yet approved for public distribution. No Visual Studio is
required. Extract the ZIP and run `octave-desktop.exe`, or run the included
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
- A one-hour playback/memory workload and representative track matrix remain open.
  The silent accelerated pipeline workload now uses the maximum 12-second tail;
  a separate output-queue fixture verifies the full 12-second Standard overlap.
  These generated checks do not establish an hour of live AAC playback. A bounded
  five-recording unsigned probe decoded and sought one recording successfully;
  four initially returned SignInRequired. A four-client follow-up identified
  bot-verification reasons on all four, with no formats offered. A broader ten-ID
  decoder sample passed one recording; nine returned BotCheckRequired. Octave now
  distinguishes those checks from ordinary sign-in requirements and does not
  automatically retry either. Matching endpoint Origin and User-Agent also left
  access unchanged. No client fallback or extra runtime was adopted. Unsigned
  lyrics availability does not establish audio access; broader unsigned resolver
  compatibility remains a feasibility gate on this network.
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

Clock-driven line lyrics, Follow/manual scrolling, Focus, companion panels,
complete public playlists, local history/pins, unsigned catalog and longer
standard fades have offline fixture coverage. Translation HTTP and Discord IPC
fixtures pass; real configured LibreTranslate and Discord application/desktop
acceptance remain open. No default translation server or Discord application ID
is supplied. API keys/session credentials remain in memory.

The adaptive module is an isolated local analysis experiment, not production
adaptive audio. Native video is deferred. Account/Premium, screen-reader/RTL,
physical audio/device latency, fresh-machine/GPU/scaling and signing acceptance
must remain explicit before a public listening-experience release. Headless
frame measurements and silent native captures do not establish those outcomes.
