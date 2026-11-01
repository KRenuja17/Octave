# Code signing policy

Octave's signing provider is not yet approved or configured. Current evaluation
packages are unsigned. No SignPath sponsorship or certificate is claimed.

The repository owner, [KRenuja17](https://github.com/KRenuja17), reviews and
approves production signing and publication. Signing access and provider roles
must be confirmed by the owner before enrollment; all accounts with release or
signing access must use MFA. External contributions require maintainer review.

Only a clean, reviewed source commit and its qualified release build may be
submitted for signing. Signing credentials/private keys must stay in the
approved provider or protected signing environment, outside Git, packages and
diagnostic output. Evaluation CI has read-only repository permissions and no
signing credentials. Each signing request requires maintainer approval.

The final artifact must pass publisher/timestamp verification, executable product
and version checks, regenerated SHA256 manifests, installer/registration and
runtime checks. Record the source commit and final signed ZIP checksum. Do not
publish an unsigned artifact as a signed production release or treat a signature
as evidence of remaining accessibility/fresh-machine acceptance.

Automatic updates remain disabled. A future updater requires separately managed
Ed25519 metadata signing keys, qualified HTTPS hosting and recovery/rollback
policy; an Authenticode certificate does not supply those controls.

See the [privacy policy](PRIVACY.md), [credential safety](SECURITY.md) and
[distribution procedure](packaging/DISTRIBUTION.md). Approved provider identity,
attribution and public signing details will be added only after acceptance.
