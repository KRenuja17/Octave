# Account and credential safety

Octave is an unofficial, experimental client. It cannot guarantee that use of
unofficial service endpoints will never trigger an account restriction. No
signed-in acceptance result or assurance of Google approval is claimed.

The existing advanced Account connection imports a YouTube Music browser Cookie
header. It is not Google OAuth with narrowly granted scopes. Treat the cookie
as a login credential: do not paste it into chat, issue reports, source files,
terminal commands, HTTP recordings or screenshots. The current account feature
does not enable authenticated or Premium playback.

The account adapter sends credentials only to its fixed HTTPS YouTube Music
origin. Account redirects are disabled, request headers are marked sensitive,
and application diagnostics record fixed categories/numbers rather than request
headers, URLs or response bodies. Account credentials are memory-only and are
zeroized when their owning buffers are released. Disconnect revokes Octave's
shared handles; an in-flight request can retain a copy until cancellation or its
bounded timeout. It does not revoke the browser session at Google. Memory-only
handling cannot prevent exposure through clipboard history, OS memory/paging,
crash dumps, malware or a compromised process.

Private library pages are held in memory. If a user deliberately queues, plays
or saves library songs, song IDs/titles/artists can enter the local listening,
history or playlist databases. Credential secrecy and listening-data privacy are
separate concerns. Keep any signed-in test profile outside the repository.

Git ignore rules exclude common credential files, databases, HAR exports, crash
dumps and private-key formats. These rules reduce accidental additions; they
do not stop forced additions, secrets embedded in source code, or files already
tracked. Check staged changes before every commit. Current tests use authored
synthetic credentials; no live account is needed in CI. Never use real account
data as a regression fixture.

For the current no-account-risk requirement, remain signed out. Before enabling
personal-account tests, evaluate official Google desktop OAuth with least-access
scopes for supported YouTube Data API operations. That is a separate adapter,
not permission to reuse OAuth tokens against unofficial Music endpoints or a
promise of native Premium playback. User consent, provider policy and live
security review remain necessary.

Relevant provider/security references:

- [YouTube terms](https://www.youtube.com/t/terms)
- [Google desktop OAuth for the YouTube Data API](https://developers.google.com/youtube/data/guides/auth/installed-apps)
- [OWASP session management](https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html)

When reporting a security problem, provide a redacted description and synthetic
reproduction. Do not post session cookies, tokens, private library exports or
personal databases to the public repository.
