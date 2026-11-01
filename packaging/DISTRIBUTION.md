# Windows distribution

The current artifact is an unsigned evaluation package. Attended Windows
Account/Premium scope is described in README.md. Translation is optional and off;
no paid translation service is required. A signature alone does not complete
listening, fresh-machine or accessibility acceptance.

## Build and independent runtime checks

`Build-Setup.ps1` creates `Octave-<version>-Setup.exe` from a clean optimized
package after verifying every manifest entry, required notice/source bundle and
both executable version resources. Extra files, links, duplicate/unsafe names,
tampering and dirty/debug payloads are rejected. Output stays outside source and
the immutable portable package. The SHA256 sidecar and build receipt bind the
payload manifest, payload commit, installer commit and compiler hash.

`Get-SetupCompiler.ps1` downloads the pinned official Inno Setup 7.1.1 release,
verifies its published SHA256 and trusted Pyrsys B.V. Authenticode signature, then
extracts it in the vendor's portable/current-user mode without IDE registration.
The app remains Rust and needs neither this compiler nor Visual Studio to run.
Sources: [official release](https://github.com/jrsoftware/issrc/releases/tag/is-7_1_1),
[portable mode](https://github.com/jrsoftware/issrc/blob/is-7_1_1/isportable.iss),
[compiler license](https://github.com/jrsoftware/issrc/blob/is-7_1_1/license.txt).

Setup installs for the current user under `%LOCALAPPDATA%\Programs\Octave`,
creates a Start menu shortcut and native Windows uninstall registration, and
offers an optional desktop shortcut. The installed program is independent of the
checkout/build directories. Its guard refuses source checkouts, data folders,
unowned nonempty destinations and reparse points. It asks the user to quit an
existing installed Octave rather than closing it. Only explicitly installed files
are removed, leaving profile/account data and unrelated files. A matching old
PowerShell uninstall registration is migrated only after successful installation.
Use native Setup/Windows uninstall for a Setup-managed installation; don't mix
the portable PowerShell installer with it.

`Test-SetupGuards.ps1` exercises refusal cases without personal data or registry
changes. `Test-SetupInput.ps1` checks payload policy with synthetic damaged copies.
`Test-Setup.ps1` verifies actual native install/repair, legacy upgrade, Start menu,
Installed apps, restricted-PATH launch, uninstall and profile preservation. It
refuses existing Octave registrations and uses an explicit isolated profile.

The native Setup container and generated uninstaller are **not** covered by the
existing four-file portable SignPath configuration. They remain unsigned; before
production the approved provider must sign/timestamp the contained executables,
scripts, native uninstaller and final Setup, with corresponding same-byte checks.
Do not publish an unsigned Setup as a signed release or treat the compiler's own
signature as an Octave publisher signature.

The manually triggered **Windows evaluation package** workflow builds from source
with Rust GNU, executes workspace/renderer/demuxer and packaging-policy checks,
and uploads portable and native Setup evaluation artifacts with seven-day retention.
Separate Windows runners test portable registration and native Setup lifecycle.
They have no publication or
signing credentials and cannot create a GitHub release. Configure/dispatch it
only after the committed workflow is available on GitHub.

For a local build use `Build-Package.ps1` with an external `OutputDir` and
`CARGO_TARGET_DIR`. Avoid keeping multiple target trees. Incremental compilation
and debug information can be disabled with `CARGO_INCREMENTAL=0`,
`CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0` for smaller QA caches.

Windows builds embed product, description, contributor, original-filename and
version resources. `Build-Package.ps1 -Version <version>` sets
`OCTAVE_BUILD_VERSION` for Cargo and restores the previous environment value on
exit. Ordinary Cargo builds use the workspace version unless explicitly
overridden. Numeric Windows versions use major/minor/patch/0 (16 bits each);
the string version preserves the prerelease suffix and sets its Windows flag.
The build uses `winresource` with the existing GNU `windres` toolchain locally;
no Visual Studio installation is needed. MSVC CI uses its installed SDK tools.

`Test-ExecutableMetadata.ps1 -Executable <exe> -Version <version>` verifies the
actual desktop PE resource, including prerelease/debug flags and both numeric/string
versions. Use `-Kind signature` for the contained playback worker. Both share
product/version generation and retain their own description/internal/original
filename. Packaging and CI reject either executable with missing or mismatched
metadata, including stale `-SkipBuild` input. Repeat this check on the final
provider-signed executable as well as the signature and package-integrity checks.

Run `Test-RegisteredPackage.ps1 -PackageDir <extracted-package> -WorkDir <external-dir>`
to check the real Start-menu shortcut, uninstall registry entry, restricted-PATH
launch, unregister/uninstall and test-data preservation. It refuses an existing
Octave registration. It uses an explicit isolated profile and does not delete
normal user data. Its result records `freshMachine=false`: running it on a
developer machine does not prove consumer-machine compatibility.

The Windows Server CI image's system OpenGL implementation cannot initialize
egui_glow (the captured error requires OpenGL 2.0+). Independent registration
therefore uses checksum-pinned Mesa 26.2.5 llvmpipe in the isolated test
installation only. The setup refuses non-GitHub environments, extracts two x64
DLLs, changes no system driver and leaves the uploaded package unchanged.
The result records the graphics environment and DLL hashes. Normal host checks
omit `-SoftwareOpenGlDir` and use the installed graphics driver. This CI pass
is not hardware-GPU, unmodified-Windows or fresh consumer-machine acceptance.
Upstream [release](https://github.com/pal1000/mesa-dist-win/releases/tag/26.2.5)
and [per-application deployment documentation](https://github.com/pal1000/mesa-dist-win/blob/26.2.5/readme.md).

Before production distribution, run the same package in a fresh Windows VM or
another clean physical machine, recording OS version, GPU/driver, launch result,
registration, uninstall, hashes and any required runtimes. A separate hosted CI
runner is useful independent evidence but still has development tools installed.
Windows Sandbox is an alternative on supported Windows editions; it is unavailable
on this machine's Windows 11 Home. Do not disable security protections or import
a test root certificate to make acceptance pass.

## Code signing policy

Only reviewed builds from a clean source commit may be signed. Record the exact
source commit and final ZIP hash; retain dependency notices and corresponding
modified MPL source. A maintainer approves each signing and public release.
Keep private keys and service credentials outside this repository and artifacts.
Provider identities and timestamp URLs must come from the approved provider.

`Sign-Package.ps1` supports an existing current-user Windows certificate with an
accessible private key (including a provider-backed key where supported). It
creates a separate artifact, signs Octave's executable and PowerShell installers
with SHA256, requires a valid timestamp, regenerates hashes and verifies the
expected publisher. It refuses a self-signed certificate and dirty/debug builds.
It leaves the unsigned input unchanged. A signing failure leaves an incomplete
directory for diagnosis and produces no release ZIP; do not distribute it.

The Windows API behind `Set-AuthenticodeSignature` requires the provider's HTTP
timestamp endpoint. Microsoft documents that HTTPS can leave a signature without
a timestamp. For HTTPS/RFC3161 or cloud providers, use that provider's supported
signing client instead, then regenerate package hashes and run
`Test-ReleaseSignatures.ps1` against the independently configured publisher
thumbprint. No provider client or certificate is installed by these scripts.

Example for an already approved certificate:

```powershell
.\packaging\Sign-Package.ps1 -PackageDir <evaluation-directory> `
  -OutputDir <external-output> -CertificateThumbprint <public-thumbprint> `
  -TimestampServer <provider-http-timestamp-url>
```

`Test-SigningPolicy.ps1` checks unsigned rejection and synthetic policy cases.
It does not establish successful trusted signing. The live signature gate must
pass on the final bytes after provider signing; timestamp, publisher, code-signing
usage, required files and every manifest checksum are checked.

A possible no-cost route is [SignPath Foundation](https://signpath.org/), which
requires application and approval. Its [conditions](https://signpath.org/terms)
include released/maintained open source, verifiable builds, project reputation,
review and signing roles, MFA, binary product/version metadata and a public
privacy/signing policy. Octave has not been accepted, and no SignPath sponsorship
or certificate is claimed. Do not add their attribution as an existing sponsor
before approval. The owner must review these requirements and submit any account
application; no accounts or keys are created automatically.

### Prepared SignPath route

`signpath-artifact.xml` is a **provider-review candidate**, based on the provider's
[artifact examples](https://docs.signpath.io/artifact-configuration/examples).
It restricts product/version/company/original-filename metadata on the two
Octave executables and signs only those executables and the two installer
scripts. Other payloads, including upstream GNU DLLs and notices, pass through
unchanged. The provider must approve the exact configuration, source dependency
arrangement and project eligibility before use.

The optional `sign=true` distribution job uses the pinned official SignPath v3
action and the artifact ID from the same reviewed main-branch build, after
independent registration passes. It follows the provider's
[GitHub trusted-build integration](https://docs.signpath.io/trusted-build-systems/github).
There is no token, project configuration or environment created automatically.
Before enabling, the owner must create **release-signing** with required review
and configure its protected values from the approved provider:

| Name | Type |
| --- | --- |
| `SIGNPATH_API_TOKEN` | Environment secret, submitter access only |
| `SIGNPATH_ORGANIZATION_ID` | Environment variable |
| `SIGNPATH_PROJECT_SLUG` | Environment variable |
| `SIGNPATH_SIGNING_POLICY_SLUG` | Environment variable, manual approval policy |
| `SIGNPATH_ARTIFACT_CONFIGURATION_SLUG` | Environment variable, approved configuration |
| `SIGNPATH_PUBLISHER_THUMBPRINT` | Environment variable, independently verified public certificate thumbprint |

Never paste service credentials into chat/source or commit an environment export.
The SignPath account and GitHub account require owner-controlled MFA. Application
acceptance, required-reviewer protection and provider manual approval are owner/
provider prerequisites, not conditions inferred from variable names. No production
signing request has been submitted or verified yet.

`Complete-SignedPackage.ps1` imports the returned artifact into a separate output
directory. It checks the original input manifest, clean optimized source/version,
exact file set and unchanged non-signing payloads, retains BUILD-INFO, regenerates
hashes and requires publisher/timestamps and both PE metadata resources. It emits
a ZIP and SHA256 sidecar only after qualification. It relies on SignPath's trusted
build connector/manual approval for the provenance of changed signed executables;
signature validity alone does not prove their correspondence to unsigned code.
`Test-SignedPackageImport.ps1` verifies real unsigned rejection and synthetic
import-policy cases; its mock valid signatures are not evidence of live signing.

The workflow repeats registration on the final signed files before uploading
`windows-signed-review`. This is still CI with isolated Mesa, not consumer-GPU or
SmartScreen acceptance. Use [consumer acceptance](CONSUMER_ACCEPTANCE.md) and
`Test-ConsumerPackage.ps1` on the reviewed final signed manifest. The tool records
non-identifying OS/GPU/RAM/input-method facts and bounded install/uninstall results;
attended checks stay pending. It does not collect account sessions or machine/
user names, and does not install runtimes, IMEs or alter security protections.

Application entry: [SignPath Foundation](https://signpath.org/apply.html).
Its current form needs maintainer contact information, reputation evidence and
agreement to its conditions/data processing. The owner supplies those directly
to SignPath. A public evaluation release may be needed for the already-released
condition. An unpublished draft is not a public release, and another project's
reputation is not evidence of Octave adoption. No acceptance is guaranteed.

## Production release checkpoint

1. Complete the remaining explicitly required listening/accessibility gates and
   record any agreed scope exclusions. Paid translation remains optional;
   attended Windows Account/Premium scope is described in README.md.
2. Build a clean versioned artifact and run installer/registration checks.
3. Obtain provider approval, sign, verify the exact publisher and timestamps,
   regenerate checksums, and repeat runtime/registration checks on signed bytes.
4. Complete a fresh-machine test and review the dependency/source inventory.
5. Review the final ZIP, source commit, SHA256 and release notes before publishing.

Production updater hosting and a pinned Ed25519 metadata key are not configured.
Keep automatic updating disabled until its key management, signed metadata,
expiry/sequence policy, HTTPS feed and recovery have been qualified. Authenticode
signing and updater metadata signing are separate trust mechanisms. No unverified
update feed is activated by the packaging workflow.

Primary references:
[GitHub context availability](https://docs.github.com/en/actions/reference/workflows-and-actions/contexts#context-availability),
[Windows Sandbox editions](https://learn.microsoft.com/en-us/windows/security/application-security/application-isolation/windows-sandbox),
[PowerShell Authenticode timestamps](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.security/set-authenticodesignature#-timestampserver).
