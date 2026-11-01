# Consumer Windows acceptance

Run this on the final **signed bytes**, using an ordinary Windows account and
the computer's installed graphics/audio drivers. The Acer i5/MX130/6 GB laptop
is a useful separate hardware target when available. Its OS, driver support,
input methods and performance still need measurement. Do not reset that laptop
or remove development tools to create a test environment. Record what is there.

Keep security protections enabled. Do not install Rust, Visual Studio, GNU tools,
Mesa, a test root certificate or an IME just to make this test pass. Never use a
personal Google/YouTube session for this acceptance. This procedure needs no
sign-in. Keep screenshots and reports outside the source repository, and omit
desktop notifications, usernames and account information from shared evidence.

## Identify the reviewed artifact

The release review must supply a source commit, signed ZIP SHA256, extracted
manifest SHA256 and independently confirmed publisher-certificate thumbprint.
Use the source/test scripts from that same reviewed commit. Download the ZIP
through a browser from the review/download URL and record the browser/Windows
security messages before extracting. A USB copy, CI download or developer
command-line launch alone does not verify consumer SmartScreen behavior.

Verify the ZIP with `Get-FileHash -Algorithm SHA256`, then the signatures and
manifest using `Test-ConsumerPackage.ps1`. A new signed application can still
show a reputation warning; record its exact wording and publisher. Do not claim
that signing guarantees warning-free launch. If Windows blocks execution, stop
that test and report it; do not disable protections or unblock files to record a
pass. See Microsoft's [SmartScreen reputation documentation](https://learn.microsoft.com/windows/apps/package-and-deploy/smartscreen-reputation).

```powershell
# Replace placeholders with the approved review's public values and local paths.
.\packaging\Test-ConsumerPackage.ps1 -PackageDir <extracted-signed-package> `
  -WorkDir <external-evidence-directory> `
  -PublisherThumbprint <approved-public-thumbprint> `
  -ExpectedManifestSha256 <reviewed-manifest-sha256> `
  -CleanConsumerEnvironment -RunRegistrationCheck
```

Use `-CleanConsumerEnvironment` only after confirming the machine is a consumer
environment without project build dependencies. The report records this as an
attestation; it cannot prove the machine's history. The optional registration
test refuses an existing Octave installation, uses an isolated profile, checks
the actual shortcut/registry entry and host-driver launch, then uninstalls while
checking preserved data. It adds no graphics/runtime dependencies. An automated
pass leaves attended checks pending. Do not run it over an existing installation.

## Attended checks

- Install with the signed `Install.ps1` using a normal user account. Launch from
  Start, inspect the version and verify Home, Search, Playlists, Settings, Account
  and the compact/expanded listening panel render without clipping or missing
  glyphs. Keep the existing Octave palette. Measure startup time and idle memory
  on the Acer; exercise resize/minimize/restore and the installed GPU driver.
- Create only local test queue/playlists/preferences. Quit, reinstall the same
  signed version and confirm preservation (repair/reinstall). For **upgrade**,
  start from a separate earlier reviewed build and install the new signed build;
  verify the version changes and data survives. Same-version reinstall and
  synthetic installer fixtures do not establish version-to-version acceptance.
  If no earlier signed build exists, record that limitation rather than a pass.
- Check keyboard focus and Narrator, mixed Hebrew/Arabic/Latin editing, combining
  marks, emoji/flags, selection, deletion and undo. A real composition IME check
  needs an already installed composition input method; English direct input and
  synthetic IME events do not qualify. Preserve the installed-IME-only scope.
- With one display, inspect supported app text sizes/window sizes. Physical DPI
  acceptance requires two actual displays with different Windows scale settings:
  move the window both directions while editing and while lyrics scroll, and
  inspect focus/caret, artwork and bottom-player hit targets. Software scaling
  and changing a single monitor's scale are distinct evidence.
- Quit and uninstall normally. Check Start and Installed apps registration is
  removed, binaries are removed, and user profile/unrelated files are preserved.
  Record any running-app, access-denied or security warning findings.

Record each case as passed, failed, not available or not applicable, with the
artifact hash and evidence. A reviewer must resolve failures and explicit scope
decisions before publication. A second laptop does not supply two-display
testing unless a second physical display is actually connected.
