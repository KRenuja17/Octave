# Discord Rich Presence

Octave can show the current song on your Discord profile. It is **off by
default** and works without signing into YouTube or linking accounts in Octave.
The built-in public application ID is `1566363175241125928`; it is an identifier,
not a credential. Users do not need their own developer application.

## Enable sharing

1. Run the signed-in **Discord desktop client** on the same computer.
2. In Octave, open **Settings → Optional integrations**.
3. Leave **Custom Discord application ID** empty to use Octave's application.
4. Enable **Share current song on Discord**, then play a song.

The activity uses Discord's **Listening** type and includes the song title,
artist, public YouTube artwork, playback timestamps and a **Listen on YouTube
Music** link. Seeking updates the timestamps; a new recording updates the card.
Unknown duration omits the end time. A paused, buffering, stopped or mismatched
recording is not shared. Pause/stop/disable clear without waiting for the normal
five-second publishing interval. Ordinary updates are coalesced to avoid flooding
Discord; rapid changes or resuming may take up to five seconds.

Octave reconnects automatically if Discord starts later or restarts. Playback
does not wait for Discord. A normal quit gives the worker a bounded opportunity
to acknowledge a clear before exiting. A broken connection/process crash cannot
guarantee an acknowledged clear or control Discord's display/cache latency.

## Visibility and troubleshooting

**Sharing current song** means the local Discord client acknowledged the command;
it does not prove that another user can see your activity. Discord controls its
own activity/privacy settings and how each profile view renders the card.

- Check your status: **Invisible** makes you appear offline. If you want an
  attended visibility check, temporarily choose Online yourself.
- Check Discord **Settings → Activity Sharing** (also called **Activity Privacy**
  in some versions), including **Share my activity**, audience and application
  controls. Octave never changes these settings.
- The browser-only Discord client cannot supply this desktop IPC connection.
- A custom application must have a valid public numeric Application ID. Use the
  [Developer Portal](https://discord.com/developers/applications), **General
  Information → Application ID**. Do not enter an OAuth client secret, bot token
  or Discord user token. No bot, OAuth redirect, server invite or subscription is
  needed for this integration.
- Buttons and artwork rendering depend on Discord. Your own profile view may
  differ from what a friend sees; an accepted IPC command is separate evidence
  from visual acceptance.

Disable the sharing checkbox to stop publishing. **Reset Discord setup** also
removes a saved custom ID. Editing that ID turns sharing off until you explicitly
enable it again. Sharing preference and an optional custom public ID are saved
in Octave's local profile. Starting Octave never starts audio automatically.

## Data boundary

Only the selected recording's title/artist, timestamps, fixed public artwork
URL and fixed public song link are sent to the local Discord client. No queue,
lyrics, listening history, private playlist identifiers, Google cookies or
translation keys are included. Discord can publish the supplied metadata under
its own privacy controls. The native IPC handshake includes the application ID
and commands include Octave's process ID. Discord's READY response can contain
account information; Octave discards it without logging or persisting it.

The adapter never requests OAuth authorization, subscribes to chat/user events,
reads a Discord profile/token database or sends a bot/user token. IPC packets are
capped at 64 KiB, commands/handshakes have one-second deadlines, errors expose
fixed categories, and all work runs outside the UI/audio threads. Windows uses
named pipes; Linux/macOS use the documented Unix-socket paths. Neither a
JavaScript runtime nor Discord's native SDK is needed for this adapter.

## Verification

Normal workspace tests exercise metadata/privacy boundaries, stable clocks,
seek/repeat/pause, Unicode/grapheme clipping, invalid IDs, accessible Settings,
framing/nonce checks, fragmented reads, ping/pong, timeouts and errors. A real OS
transport fixture exercises restart/reconnect, disable, application-ID changes
and quit; it uses a dedicated `octave-discord-test-*` endpoint, never Discord's.

The explicitly ignored live test publishes **actual** unsigned playback on the
default audio device at zero app volume. Run it only when you intend to share
the two test songs through the running Discord desktop client:

```powershell
$env:OCTAVE_DISCORD_LIVE = '1'
$env:OCTAVE_DISCORD_HOLD_SECONDS = '90' # optional; 20–120 seconds
cargo test -p octave-desktop discord::tests::live_discord_with_muted_unsigned_playback --locked -- --exact --ignored --nocapture
```

It checks Rick Astley playback, pause, resume, a seek to 90 seconds, stop, Oasis
playback, disable/re-enable and bounded quit. It stores no application profile,
does not change Windows volume, and is excluded from ordinary CI. A successful
run proves local RPC acceptance, not universal catalog or public visibility.

On 1 November 2026, the live lifecycle passed against the running Windows Discord
desktop. Discord returned matching application/type/title/artist fields and
artwork/timing data. An initial invisible-card report was traced to Discord's
top-level **Share my activity** switch being off. The user enabled it themselves
and confirmed the Listening card's song, artist, artwork and timing were correct.
This attended visual result is separate from the automated acknowledgements.

Protocol references checked 1 November 2026:
[Discord RPC](https://docs.discord.com/developers/topics/rpc),
[Rich Presence without authentication](https://docs.discord.com/developers/discord-social-sdk/development-guides/setting-rich-presence#rich-presence-without-authentication),
[activity sharing](https://support.discord.com/hc/en-us/articles/7931156448919-Activity-Sharing-on-Discord-FAQ),
[online status](https://support.discord.com/hc/en-us/articles/227779547-Changing-Online-Status).
