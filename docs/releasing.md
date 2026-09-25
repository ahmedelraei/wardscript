# Releasing

## Cutting a release

1. Set the version in `Cargo.toml` (`[workspace.package]`), `crates/ward_runtime/py/pyproject.toml`
   (PEP 440 spelling: `0.1.0b2` for `0.1.0-beta.2`) and `crates/ward_runtime/ts/package.json`, and
   merge that to `main`.
2. Tag the merge commit and push the tag:

   ```bash
   git tag v0.1.0-beta.2 && git push origin v0.1.0-beta.2
   ```

`.github/workflows/release.yml` then builds `ward` for each platform, tests the installers
against the new archives, and publishes a GitHub release with the archives, their
`.sha256` files, `SHA256SUMS`, both installers and a build provenance attestation per
archive. A version with a suffix (`-beta.2`, `-rc.1`) is published as a pre-release. The
release job stops if the tag doesn't match the version in `Cargo.toml`.

## Verifying a download

```bash
sha256sum -c --ignore-missing SHA256SUMS                          # the checksum (Linux)
shasum -a 256 -c ward-v0.1.0-beta.1-aarch64-apple-darwin.tar.gz.sha256   # the checksum (macOS)
gh attestation verify ward-v0.1.0-beta.1-x86_64-unknown-linux-musl.tar.gz --repo ahmedelraei/wardscript
codesign --verify --strict --verbose=2 ward                       # macOS, once signing is set up
```

On Windows, once signing is set up: `Get-AuthenticodeSignature ward.exe`.

## Code signing

Attestations work without setup. They prove where an archive came from, but macOS
Gatekeeper and Windows SmartScreen only trust signed binaries. The install scripts
aren't affected (downloads through `curl` and PowerShell aren't quarantined), but a
binary downloaded in a browser gets a warning until it is signed.

The signing steps run only when their secrets are set (repository **Settings → Secrets
and variables → Actions**), and never on pull requests. Until then, releases are
unsigned.

### macOS: Developer ID and notarization

Needs an [Apple Developer Program](https://developer.apple.com/programs/) membership.

1. Create a **Developer ID Application** certificate (Xcode → Settings → Accounts →
   Manage Certificates, or developer.apple.com → Certificates), and export it from
   Keychain Access as a `.p12` with a password.
2. In App Store Connect → Users and Access → Integrations → **App Store Connect API**,
   create a key with the Developer role and download the `.p8` (it downloads only once).

| Secret | Value |
|---|---|
| `MACOS_CERTIFICATE` | `base64 -i certificate.p12` |
| `MACOS_CERTIFICATE_PASSWORD` | the `.p12`'s password |
| `MACOS_SIGNING_IDENTITY` | e.g. `Developer ID Application: Your Name (TEAMID)` (`security find-identity -v -p codesigning`) |
| `APPLE_API_KEY` | `base64 -i AuthKey_XXXX.p8` |
| `APPLE_API_KEY_ID` | the key's ID |
| `APPLE_API_ISSUER` | the issuer ID shown above the keys |

Both macOS binaries are signed with the hardened runtime and a secure timestamp, then
notarized with `notarytool`. A bare executable can't be stapled, so Gatekeeper checks the
notarization online the first time it runs.

### Windows: Azure Artifact Signing

Since 2023, code-signing keys must live in hardware or a cloud HSM, so a `.pfx` file in CI
is no longer an option for new certificates. Azure
[Artifact Signing](https://learn.microsoft.com/azure/artifact-signing/) (formerly Trusted
Signing) is a cloud-HSM service built for CI. Check its identity-validation requirements
for individuals and organizations in your country first.

1. Create an Artifact Signing account and complete identity validation, then create a
   **Public Trust** certificate profile.
2. Create an app registration (service principal) with a client secret, and give it the
   Certificate Profile Signer role on the account ("Artifact Signing Certificate Profile
   Signer", or "Trusted Signing Certificate Profile Signer" on accounts from before the rename).

| Secret | Value |
|---|---|
| `AZURE_TENANT_ID` | the directory (tenant) ID |
| `AZURE_CLIENT_ID` | the app registration's client ID |
| `AZURE_CLIENT_SECRET` | its client secret |
| `AZURE_SIGNING_ENDPOINT` | the account's region endpoint, e.g. `https://eus.codesigning.azure.net/` |
| `AZURE_SIGNING_ACCOUNT` | the account name |
| `AZURE_CERTIFICATE_PROFILE` | the certificate profile name |

`ward.exe` is signed with SHA-256 and timestamped before it's packaged.
