# Octave reference direction

Decision: 31 October 2026. **Limusic is the primary application reference** for
future Octave design and listening/account work, as requested by the user.
Octave keeps its Rust application, native egui UI, and original charcoal/sage
palette. Reference priority does not change the accepted audio-first scope.

## Reference order

1. **[Limusic](https://github.com/SimoHypers/limusic)** and its
   [product site](https://simohypers.github.io/limusic/): overall listening flow,
   artwork/player/lyrics hierarchy, compact and theater experiences, queue
   continuity, and account/channel lifecycle. Use these behaviors as the first
   comparison when proposing refinements; adapt them to Octave's design.
2. **[Orchard](https://github.com/SFG5453/Orchard)** and its
   [product site](https://sfg545.dev/orchard/): supplementary reference for
   OS-protected session persistence, isolated sign-in helpers, account switching
   and listening transitions.
3. **[sndell/yt-music-app](https://github.com/sndell/yt-music-app)**:
   supplementary reference for queue, lyrics, player interactions and automated
   browser-session capture. Earlier project references remain background research.

These application references inform behavior and architecture; none of their
code or assets were imported. Separately, the MIT-licensed ytdlp-ejs preprocessor
subset is pinned in `vendor/octave-ejs`, with attribution and a restricted native
worker; see its `README-Octave.md`. Google player scripts are not redistributed.
Discord was reopened by the user on 1 November 2026; see [the integration guide](DISCORD.md).
Paid translation remains optional, and native video
and production Adaptive mixing remain deferred. Other reference features are
comparison material, not new release requirements.

## Verified sign-in comparison

Public source snapshots reviewed on 31 October 2026:

| Project / revision | Google sign-in and session use | Credential persistence |
| --- | --- | --- |
| Limusic `d82372c` | Tauri login WebView; native capture of YouTube-domain cookies; Rust internal-API client. Windows uses WebView2's default user agent. | Full cookie header in ordinary SQLite text fields; separate persistent WebView profile. Sign out retains saved accounts; Remove account deletes the selected database row. |
| Orchard `54a49f3` | Separate sign-in helper; native WebView2 on Windows; browser-cookie adapter and local session IPC. | QKeychain with insecure fallback disabled. Sign out clears live state and requests deletion of the stored secret. Browser-profile erasure is a separate concern. |
| sndell `ab0be0b` | pywebview/WebView2 login; captures Music API headers/cookies; validates with ytmusicapi. | Readable `browser.json` plus persistent WebView data. Sign out clears profile data, removes auth files and clears its cache. |

These reviewed paths reuse Google/YouTube web sessions; they are not scoped
desktop OAuth grants. Limusic's refresh handles rotated cookies and refuses to
silently adopt a different account. Its settings interface withholds session
secrets from the renderer. Those are useful patterns, while Orchard offers the
stronger persistence model to follow. A successful browser login alone does not
verify native Premium playback or establish a zero account-restriction risk.

Sources: Limusic [login host](https://github.com/SimoHypers/limusic/blob/d82372cdd3016e3c5e28ea8022596b7fefdf681a/src-tauri/src/session.rs),
[database](https://github.com/SimoHypers/limusic/blob/d82372cdd3016e3c5e28ea8022596b7fefdf681a/src-tauri/src/db.rs),
[account lifecycle](https://github.com/SimoHypers/limusic/blob/d82372cdd3016e3c5e28ea8022596b7fefdf681a/src-tauri/src/state.rs),
[settings boundary](https://github.com/SimoHypers/limusic/blob/d82372cdd3016e3c5e28ea8022596b7fefdf681a/src-tauri/src/commands.rs);
Orchard [session/keychain lifecycle](https://github.com/SFG5453/Orchard/blob/54a49f3c1ca58ea8251868b89730f2f9f9d52f49/app/src/auth/auth_manager.cpp),
[local IPC](https://github.com/SFG5453/Orchard/blob/54a49f3c1ca58ea8251868b89730f2f9f9d52f49/app/src/auth/auth_session_channel.cpp);
sndell [capture, validation and logout](https://github.com/sndell/yt-music-app/blob/ab0be0bd9d979a4db46e5b011044a79c3cc83389/app/features/music_api.py).

## Implemented Octave account direction — 1 November 2026

Follow Limusic's user-facing Google login and account lifecycle, with
Orchard-style OS-protected persistence. Prefer this to asking users to copy
cookies manually. The native implementation now:

- Keeps the native Rust UI. Uses a selected installed Brave/Edge/Chrome browser with a separate,
  temporary app-owned profile on Windows, with no application page bridge or
  script injection. Human authentication runs without debugging. After the user
  closes that window, a separate bounded native loopback CDP phase captures only
  YouTube cookies. No WebView2 SDK, Tauri migration or new browser install.
- Keeps session capture, requests and identity selection in Rust. Captures only
  the necessary YouTube-domain cookies; never read personal browser profiles or
  collect passwords through an Octave form, clipboard or chat.
- Defaults to a temporary session, including a temporary browser profile.
  Optional Remember account must use the OS credential vault with no plaintext
  fallback. Keep secrets out of SQLite, renderer messages, diagnostics and Git.
- Validates the selected account/channel before activation. Serializes auth
  transitions and reject stale refresh results after disconnect or account
  changes. Expiry and cancelled sign-in must leave a coherent signed-out state.
- Distinguishes Disconnect from Forget account. Forget clears saved secrets
  and the app-owned browser session and report cleanup failures. Test retries,
  crashes, persistence and refresh races with synthetic credentials first.
- Treats read-only live account checks and Premium playback qualification as
  separate acceptance steps. Synthetic tests and native browser/vault checks pass;
  attended live results are tracked in [release notes](RELEASE.md). The original
  public-source review itself accessed no account or credentials.

The installed-browser choice also accounts for Google's restrictions on embedded
sign-in. See Microsoft's [WebView2 Google-authentication notes](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/browser-features#google-authentication)
and Chromium's [separate-profile debugging guidance](https://developer.chrome.com/blog/remote-debugging-port).
It does not guarantee that Google will accept every browser or native API request.
Google's [supported-browser guidance](https://support.google.com/accounts/answer/7675428?hl=en)
explicitly identifies embedded/software-automated sign-in as potentially blocked.

See [account and credential safety](SECURITY.md) for the current implementation
and limits. Detailed source notes, hashes and the updated working plans stay in
`D:\Octave-Research\signin-references-2026-10-31`, outside the project checkout.
