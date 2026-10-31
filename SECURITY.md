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

For the current no-account-risk requirement, remain signed out. The desktop
reference review below identifies a practical browser-session design for future
work; it does not establish zero account risk or authorize personal-account
testing. Official Google desktop OAuth is a separate option for supported
YouTube Data API operations with least-access scopes. It does not replace a
Music web session or establish native Premium playback. User consent, provider
policy and implementation review remain necessary for either path.

Relevant provider/security references:

- [YouTube terms](https://www.youtube.com/t/terms)
- [Google desktop OAuth for the YouTube Data API](https://developers.google.com/youtube/data/guides/auth/installed-apps)
- [OWASP session management](https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html)

## Desktop application references

[Limusic is the primary Octave reference](REFERENCES.md). The reviewed Limusic,
Orchard and sndell sign-in paths open Google/YouTube pages in an app-owned browser
and capture a web session for internal Music API calls. This avoids manual cookie
copying, but is not an official scoped OAuth grant. A Google-hosted page still
runs inside an application-controlled browser; its appearance alone is not proof
of host isolation or credential safety.

At the pinned revisions in REFERENCES.md, Limusic saves cookie headers in SQLite
text fields and retains accounts on Sign out; Remove account is distinct.
Its backend settings whitelist withholds auth material from the renderer, and
refresh checks the expected account. sndell saves browser headers to JSON and
clears browser data and its auth file on sign-out. Orchard uses QKeychain with
insecure fallback disabled, and transfers helper credentials through a dedicated
local socket rather than command arguments or stdout. OS-vault storage does not
eliminate the separate browser-profile copy or protect a compromised process.

The proposed Octave direction combines Limusic's account lifecycle with
OS-protected, opt-in persistence and explicit browser-profile cleanup. This is
documented design work; the existing memory-only manual adapter is unchanged.
No account credentials, browser profiles or private library data were accessed
in this public-source review. Community adoption is useful experience, but is
not evidence of an audited ban rate or a guarantee of provider approval.

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
