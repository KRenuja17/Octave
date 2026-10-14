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

- Authenticated playback and broader playback feasibility remain deferred. Account
  endpoints need a real user-initiated session test; credentials stay in memory.
- Physical media keys/tray interaction, output-device changes, actual network
  interruption, screen-reader tasks and a fresh Windows machine need acceptance.
- A one-hour playback/memory workload and representative track matrix remain open.
- Production signing, update-key custody, hosting, license obligations (including
  bundled font and MPL dependency requirements), and tagged-source reproduction
  must be settled before distribution. The generated dependency inventory includes
  a workspace superset; missing license texts are listed in `BUILD-INFO.json`.
- Installer rollback covers caught file activation errors, not every interruption.

## Sanitized bug reports

Include package version, source commit from BUILD-INFO, Windows version, UI action,
expected/actual behavior, and the visible error category. Reproduce with an isolated
`--data-dir ABSOLUTE_DIRECTORY` when possible. Never attach cookies, authorization
headers, signed media URLs, raw HTTP responses, browser data, or your listening
database. Review screenshots for account names and private library contents.
Disconnect and quit to release the memory-only account session. An in-flight
request can retain its copy until its bounded timeout expires.
