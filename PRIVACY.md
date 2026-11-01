# Octave privacy policy

Effective 1 November 2026. Octave is an unofficial local desktop music client.
The application has no Octave-operated account or analytics backend and does not
upload listening databases or crash reports to the developer automatically.

## Network services

Catalog browsing, search, playback, public artwork and lyrics use Google/YouTube
services and their media/image hosts. These requests expose normal connection
information such as IP address, plus the search terms or recording/collection
identifiers needed for the requested feature. Public Home content may load when
that view opens. Artwork requests do not carry account cookies.
On Windows, authenticated playback may fetch public YouTube player JavaScript
without account headers. A separate bounded decoder receives only that public
script and signature/throttling strings, never session cookies or private library
responses. Public scripts stay in memory and are not distributed with Octave.
The service's handling of those requests is governed by
[Google's privacy policy](https://policies.google.com/privacy) and
[YouTube's terms](https://www.youtube.com/t/terms).

Translation is off until requested. Configuring a LibreTranslate-compatible
endpoint and requesting translation sends the selected lyrics and language
choices to that endpoint, with the supplied API key if any. Choose a service
whose privacy policy you accept. Keys and translated lyrics are memory-only.

Optional Discord Rich Presence is off by default. If explicitly configured and
enabled, it sends the current song's title/artist, timestamps, public artwork URL
and song link through the local Discord client, which may publish it according
to [Discord's privacy policy](https://discord.com/privacy). Octave uses its public
application ID or an optional override; no Discord token or account linking is
required. Only the opt-in preference and custom public ID are saved locally.
Pause/buffering/stop/disable clear the activity; normal quit attempts a bounded
clear. Discord's READY account fields are discarded without logging/storage.
See [Discord setup](README.md#listening-experience). Automatic update hosting is
not configured and automatic updating remains disabled.

## Local data and account connection

Queue, listening history, local playlists and preferences are stored in the
selected local profile. Song IDs, titles and artists can be retained when queued,
played or saved, including songs loaded from a connected private library. Files
are not encrypted merely because they are local. History can be cleared in the
app; uninstall preserves user profiles. Users control backups and profile removal.

Account sign-in opens an installed browser with an app-owned temporary profile.
You enter Google credentials yourself without a debugging connection or browser
automation, then close that window. Octave briefly reopens only its owned profile
for capture. Native Rust captures only
YouTube-domain session cookies through a loopback connection, validates the
account and closes/deletes the temporary profile. Octave never reads personal
browser profiles or collects passwords in its UI. Cleanup failures are reported;
a crash can leave the owned profile until a later sign-in or explicit Forget.

Sessions stay in memory unless you enable Remember account. Windows/macOS
persistence uses an encrypted file in the selected profile and a separate key in
the OS credential store; no plaintext fallback is used. Linux persistence is
unavailable. Disconnect/quit removes the active session; Forget additionally
removes remembered files, the vault key and owned browser profiles. Neither
remotely revokes Google sessions. Uninstall preserves user data, so use Forget
first to remove the vault entry. Bounded in-flight requests may retain a copy
until cancelled or timed out. Credentials are sent only to the fixed HTTPS
YouTube Music API origin, never to media/artwork hosts. See
[account setup](README.md#account-and-desktop-beta-preview).

The first Liked Music page loads after a verified account connection or restore.
Other private library pages load when selected, refreshed or explicitly paged.
Up to eight inactive library pages are cached in memory during the connection;
changing account/channel or disconnecting clears that cache. Private library
responses and fetched lyrics are memory-only unless the user
performs an action that persists song metadata as described above. Local OS media
controls display playing metadata. Clipboard use, OS paging/crash dumps, malware
and manual diagnostic sharing have privacy implications outside that storage
policy; do not include account cookies or personal profiles in reports.

Reports must omit credentials, cookies, private library responses and personal
browser/profile files. Package builds include this policy. Any future telemetry, changes to credential storage or
update service requires an updated policy and appropriate user controls.
