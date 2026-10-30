# Isolated attestation evaluation

The user authorized evaluating JavaScript for playback attestation on 30 October
2026. Octave's production resolver, UI and audio engine remain Rust. This directory
contains a permission regression gate, not a shipped attestation backend.

The evaluated runtime is portable Deno v2.9.8 Windows x64 from its official GitHub
release. ZIP SHA-256:
`cba84b26e0df5ae7f74fc66b86a95b866872a86292ebd84fa1e959927edd93bf`.
The gate pins the extracted executable separately. No global installation,
Visual Studio, browser profile, sign-in or account cookies are required.

```powershell
.\packaging\Test-AttestationSandbox.ps1 -Runtime <absolute-deno.exe> -OutputDir <external-evidence-directory>
```

The tested worker denies filesystem, network, environment, subprocess, FFI and
system-information access; disables configuration discovery, npm, remote imports
and interactive permission prompts. All seven denial checks passed. A separate
research host fetched fixed HTTPS protocol endpoints and passed challenge data
through stdin, with redacted results over stdout. Runtime/code remained in the
research directory. The research worker used a 192 MiB V8 heap setting; this is
not a hard process-memory limit or an OS security boundary.

The experiment used the previously audited rustypipe-botguard 0.1.2 bundle
(BgUtils 3.1.3 and jsdom-minimal 26.0.0), under the license inventory in that
archive. It is not redistributed in Octave. Production adoption would require
an updated dependency/advisory review, redistribution notices, a Rust host with
validated messages and hard lifecycle/resource bounds, and demonstrated benefit.
Permission checks cannot establish immunity to runtime exploits.

## Results

Challenge execution, integrity-token receipt and content/session token minting
succeeded. Both long recordings returned direct audio formats with content tokens.
However, matched requests **without** tokens also succeeded, and the unchanged
Rust resolver subsequently read media from both. There was no same-session failing
baseline before attestation. Neither token necessity nor a causal bot-check fix is
established; service state or other request differences may explain access.

Two 30-second default-device, zero-player-volume playback attempts then failed
the unchanged 15-second startup deadline before consuming audio. The first long
recording completed 110 successful range transactions; the second completed65.
These are startup failures after access succeeded, not BotCheckRequired failures.
No hour or full matrix acceptance is claimed. Investigate decoder initialization
and MP4 scanning next; do not silently extend the acceptance deadline.

An initial Python research host stalled in its network transport and was stopped
after exact process verification. A subsequent diagnostic used bounded curl
transactions and a separate Deno worker. Python/curl are research tooling only;
neither is an application dependency. No tokens or raw player URLs were retained.

Primary references:
- https://docs.deno.com/runtime/fundamentals/security/
- https://github.com/denoland/deno/releases/tag/v2.9.8
- https://docs.rs/rustypipe-botguard/0.1.2/rustypipe_botguard/
