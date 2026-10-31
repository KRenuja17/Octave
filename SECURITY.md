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

## uYouPlus comparison

Reviewed uYouPlus archived commit `c6f7672e3ab17ddb5ed486a0b8b1e2022ce9ad2c`
and historical release `v18.08.1-2.3.1`. Its build injects tweaks into the
existing iOS YouTube app. The historical source patches Google SSO Keychain
access groups for the re-signed app; the archived source includes a configurable
bundle-identity sign-in patch. These are iOS integration workarounds, not a
portable independently authorized desktop OAuth client. The original YouTube
app and uYou binary are not fully covered by this public-source review.

The project documents repeated sign-in rejection and compatibility fixes.
Sign-in rejection is different from account suspension; neither those reports
nor successful user experiences establish a ban probability or a guarantee.
This review does not establish credential theft, either. Applicable lessons for
Octave are avoiding manual credential copying, using OS-protected storage if
persistence is added, and separating credential protection from provider-policy
acceptance. No uYouPlus login hooks or account credentials were imported.

Sources: [archived sign-in hook](https://github.com/qnblackcat/uYouPlus/blob/c6f7672e3ab17ddb5ed486a0b8b1e2022ce9ad2c/Sources/uYouPlusPatches.xm),
[historical SSO/Keychain hooks](https://github.com/qnblackcat/uYouPlus/blob/v18.08.1-2.3.1/uYouPlus.xm),
[build composition](https://github.com/qnblackcat/uYouPlus/blob/c6f7672e3ab17ddb5ed486a0b8b1e2022ce9ad2c/.github/workflows/buildapp.yml),
[project FAQ](https://github.com/qnblackcat/uYouPlus/wiki/FAQs).

When reporting a security problem, provide a redacted description and synthetic
reproduction. Do not post session cookies, tokens, private library exports or
personal databases to the public repository.
