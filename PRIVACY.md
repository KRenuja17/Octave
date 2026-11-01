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
See [Discord setup and verification](DISCORD.md). Automatic update hosting is
not configured and automatic updating remains disabled.

## Local data and account connection

Queue, listening history, local playlists and preferences are stored in the
selected local profile. Song IDs, titles and artists can be retained when queued,
played or saved, including songs loaded from a connected private library. Files
are not encrypted merely because they are local. History can be cleared in the
app; uninstall preserves user profiles. Users control backups and profile removal.

The current advanced Account adapter accepts an explicitly provided browser
Cookie header, retains it only in memory and sends it to the fixed HTTPS
YouTube Music origin. It never reads personal browser profiles or asks for a
Google password. Disconnect/quit forgets Octave's session; it does not revoke
Google's browser session. A bounded in-flight request may retain a copy until
it completes or times out. This does not enable native Premium playback.
Proposed embedded sign-in and optional credential-vault persistence are not
implemented by the current reference review.

Private library responses and fetched lyrics are memory-only unless the user
performs an action that persists song metadata as described above. Local OS media
controls display playing metadata. Clipboard use, OS paging/crash dumps, malware
and manual diagnostic sharing have privacy implications outside that storage
policy; do not include account cookies or personal profiles in reports.

Use [credential safety guidance](SECURITY.md) for redacted reporting. Package
builds include this policy. Any future telemetry, credential persistence or
update service requires an updated policy and appropriate user controls.
