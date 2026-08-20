# WinGet packaging

Flux Launcher is prepared for submission to the [Windows Package Manager Community Repository](https://github.com/microsoft/winget-pkgs). The Community Repository requires a multi-file manifest set, a stable version-specific installer URL, an installer SHA256, and unattended installation support.

The seed manifest for the first stable package is under `manifests/m/m1nuzz/FluxLauncher/0.1.54/`. It uses the lowercase package identity `m1nuzz.FluxLauncher` consistently in the directory, filenames, YAML identifiers, and publisher metadata. The seed targets stable `v0.1.54`, not a beta release. A WinGet submission PR should contain only the three manifest files for one package version.

To generate a manifest set for a future stable release on a Windows runner:

```powershell
$hash = (Get-FileHash .\FluxLauncher-Setup.exe -Algorithm SHA256).Hash
.\scripts\generate-winget-manifest.ps1 `
  -Version 0.1.54 `
  -InstallerSha256 $hash
```

Before opening a pull request, validate the directory with `winget validate --manifest <manifest-directory>`, then test installation with `winget install --manifest <manifest-directory>` or the repository's Windows Sandbox test. The installer URL must be a stable, version-specific publisher release URL; beta assets are not suitable for a Community Repository manifest.

## Automatic stable-release submission

The `Submit stable release to WinGet` workflow listens for GitHub `release.published` events. It runs only when the release has `prerelease: false`; beta releases are rejected and never submitted. The workflow can also be started manually with a stable release tag for recovery or the first submission.

The workflow downloads `FluxLauncher-Setup.exe` from the selected stable release, calculates the SHA256 instead of trusting release metadata, generates the three manifests, upgrades the Windows runner's App Installer when necessary, runs `winget validate`, pushes a versioned branch to `m1nuzz/winget-pkgs`, and opens or reuses an official pull request in `microsoft/winget-pkgs`. It exits without creating a duplicate when an official PR for the same branch already exists.

Configure a dedicated GitHub Actions secret named `WINGET_GITHUB_TOKEN`. The token must be allowed to push branches to the `m1nuzz/winget-pkgs` fork and create pull requests against the public `microsoft/winget-pkgs` repository. Prefer a dedicated fine-grained token with only the required repository access, repository metadata read access, Contents read/write for the fork, and Pull requests read/write for PR creation. If the selected GitHub token type cannot grant those permissions to a public upstream PR, use a narrowly scoped classic token with `public_repo`; never use an account password or commit a token to the repository.

The first submission may require completing Microsoft's Contributor License Agreement in the official PR. The workflow automates branch and PR creation; it does not bypass Microsoft review, CLA checks, repository validation, installer policy checks, or SmartScreen reputation requirements.

## Release signing

The stable release workflow can sign `FluxLauncher-Setup.exe`, `FluxLauncher-Portable.exe`, and the native fixture before publishing them. Signing is optional for beta workflow runs and required for stable workflow runs. Configure the following GitHub Actions secrets before dispatching a stable release:

| Name | Purpose |
| --- | --- |
| `WINDOWS_SIGNING_CERTIFICATE_BASE64` | Base64-encoded PFX/PKCS#12 certificate containing the release signing certificate and private key |
| `WINDOWS_SIGNING_CERTIFICATE_PASSWORD` | Password for the PFX certificate |

The optional repository variable `WINDOWS_TIMESTAMP_URL` overrides the default RFC 3161 timestamp service. The private key must never be committed to the repository or placed in release assets.

Authenticode signing identifies the publisher and helps SmartScreen build reputation across releases signed with the same identity. It does not guarantee that a new file will avoid an initial SmartScreen warning: reputation also depends on clean download history. EV certificates do not provide an automatic SmartScreen bypass.
