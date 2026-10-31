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

These references inform behavior and architecture. No third-party source or
assets were imported. Future code reuse requires a separate license review.
Discord remains excluded, paid translation remains optional, and native video
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

## Proposed Octave account direction

Follow Limusic's user-facing Google login and account lifecycle, with
Orchard-style OS-protected persistence. Prefer this to asking users to copy
cookies manually. The next implementation pass should:

- Keep the native Rust UI. Evaluate an isolated, app-owned Windows WebView2
  sign-in host with a visible Google origin and no general application bridge.
  This host runs Google's web JavaScript; it is a runtime addition to evaluate,
  not a Tauri/Svelte migration already adopted by Octave.
- Keep session capture, requests and identity selection in Rust. Capture only
  the necessary YouTube-domain cookies; never read personal browser profiles or
  collect passwords through an Octave form, clipboard or chat.
- Default to a temporary session, including a temporary browser profile.
  Optional Remember account must use the OS credential vault with no plaintext
  fallback. Keep secrets out of SQLite, renderer messages, diagnostics and Git.
- Validate the selected account/channel before activation. Serialize auth
  transitions and reject stale refresh results after disconnect or account
  changes. Expiry and cancelled sign-in must leave a coherent signed-out state.
- Distinguish Disconnect from Forget account. Forget must clear saved secrets
  and the app-owned browser session and report cleanup failures. Test retries,
  crashes, persistence and refresh races with synthetic credentials first.
- Treat a later read-only live account check and Premium playback qualification
  as separate acceptance steps. This research accessed no account or credentials
  and does not implement or claim either result.

See [account and credential safety](SECURITY.md) for the current implementation
and limits. Detailed source notes, hashes and the updated working plans stay in
`D:\Octave-Research\signin-references-2026-10-31`, outside the project checkout.
