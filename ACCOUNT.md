# Account sign-in and Premium playback

Octave follows Limusic's browser-session/account lifecycle, with its own Rust
implementation and OS-protected optional storage. This is an unofficial YouTube
Music integration, not a scoped Google OAuth grant or a Google-approved client.
Provider compatibility and account enforcement cannot be guaranteed.

## Connect

Open **Account**, choose an installed Brave, Edge or Chrome browser, then select
**Sign in with Google**. It opens with a new, app-owned temporary profile;
your personal browser's existing sign-in is not imported. Enter credentials only on Google's
page in that browser. Octave does not read your personal browser profile, collect
passwords through an Octave form, or ask you to copy cookies. Never send session
cookies, passwords or tokens in chat or bug reports.

Wait for YouTube Music to finish loading after sign-in, then **close that isolated
browser window**. During authentication there is no debugging connection or
browser automation. After it exits, Octave briefly reopens only its owned profile
for native session capture. The worker reads only YouTube-domain cookies through
a bounded loopback connection, then closes the browser and removes its temporary profile.
It validates the account before enabling the session. Cancellation closes and
cleans the owned browser; cleanup failures are reported and can be retried with
**Forget account**. A crash may leave a temporary profile until the next sign-in
or explicit cleanup. No personal browser profiles are removed.

Choose a server-listed account/channel in Account. **Refresh session** verifies
the expected identity and processes scoped cookie rotation; unexpected identity
changes are rejected. An expired session requires a new sign-in. Account changes
stop audio at the retained position: press **Resume** to use the selected account.

## Remember, disconnect and forget

**Remember account** is off by default. On Windows and macOS it saves an
AES-256-GCM encrypted session outside the checkout; its random encryption key is
held separately by Windows Credential Manager or macOS Keychain. There is no
plaintext fallback. The file also contains the encrypted account selector needed
to reject restoration into the wrong identity. **Restore saved account** explicitly
restores and revalidates it. Linux protected persistence is not implemented.

**Disconnect** discards the active in-memory session and stops account playback.
**Forget account** additionally removes the sealed file, pending write, OS-vault
key and remaining owned sign-in profiles. A removal failure remains visible;
retry it rather than assuming cleanup completed. Neither action remotely revokes
Google sessions or signs your personal browser out. Quit discards the active
session, while an explicitly remembered session remains until Forget.

Account files live under `account` in Octave's selected data directory. Browser
profiles and remembered secrets are refused inside Git checkouts. Keep test and
production profiles outside source folders. Uninstall preserves local user data;
use Forget before removing a profile or uninstalling if you want its vault key
removed too. OS-protected storage does not protect a compromised user account or
process, and file deletion is not a forensic secure-erase guarantee.

## Library and audio

Library songs, liked songs, albums and playlists load only when requested.
**Like/Unlike** changes the actual account only when clicked. Private responses
stay in memory, but deliberately queued, played or saved song metadata can enter
the ordinary listening/history/playlist databases. Those databases are not
encrypted by Remember account.

Verified browser sessions are passed to the native audio resolver. Account
credentials go only to the fixed HTTPS Music API origin, with redirects disabled;
media and artwork hosts receive no account cookies. The player uses supported,
offered AAC streams. A Premium subscription does not guarantee that the service
offers a supported full-quality stream for every request. There is no silent
anonymous fallback after an authenticated failure. Live Premium qualification
is a separate check from successful sign-in.

On Windows, ciphered AAC URLs use the packaged `octave-signature` helper. It
evaluates public player JavaScript in Boa, a Rust engine, in a separate one-shot
process. No Node/Deno installation is needed. Only public script text and
signature/throttling values enter that process; account cookies, library
responses and complete media URLs do not. Imports and host APIs are disabled.
Windows limits it to one process and 512 MiB committed memory; Octave enforces a
deadline and kills it on cancellation. This is a capability-restricted engine
with resource limits, not an OS AppContainer sandbox or a guarantee against
engine vulnerabilities. Other platforms fail closed for cipher decoding until
equivalent containment is qualified. DRM and manifest-only formats remain
unsupported. Public scripts are cached in memory only, without account headers.

The collapsed **Connection options** form remains an advanced, memory-only manual
library adapter. Its unverified pasted session is not used for authenticated
playback. Prefer browser sign-in.

## Validation and reporting

Ordinary CI uses synthetic credentials only. Native opt-in tests check isolated
browser startup/cleanup and a synthetic vault round trip. The attended live probe
checks read-only library loading, identity refresh, available channel selection,
optional Remember/restore/Forget and muted native audio. It prints fixed stage
results and audio quality measurements, never account names or response bodies.
Current acceptance results are recorded in [release notes](RELEASE.md).

Source builds need both binaries beside each other: run
`cargo build -p octave-desktop -p octave-signature --release --locked`.
The Windows packaging script includes the helper and its license inventory.
The resolver never searches PATH or downloads an executable decoder.

See [privacy](PRIVACY.md), [credential safety](SECURITY.md) and the pinned
[Limusic/Orchard source comparison](REFERENCES.md).
