# WinGet packaging

Flux Launcher is prepared for submission to the [Windows Package Manager Community Repository](https://github.com/microsoft/winget-pkgs). The Community Repository requires a multi-file manifest set, a stable version-specific installer URL, an installer SHA256, and unattended installation support.

The submission seed for the latest stable package is under `manifests/m/M1nuzz/FluxLauncher/0.1.54/`. It is intentionally kept separate from the repository root because a WinGet pull request must contain manifest files only. The seed targets the stable `v0.1.54` release, not a beta release.

To generate a manifest set for a future stable release on a Windows runner:

```powershell
$hash = (Get-FileHash .\FluxLauncher-Setup.exe -Algorithm SHA256).Hash
.\scripts\generate-winget-manifest.ps1 `
  -Version 0.1.54 `
  -InstallerSha256 $hash
```

Before opening a pull request, validate the directory with `winget validate --manifest <manifest-directory>`, then test installation with `winget install --manifest <manifest-directory>` or the repository's Windows Sandbox test. The pull request should contain only the three manifest files for one package version.

## Release signing

The stable release workflow can sign `FluxLauncher-Setup.exe`, `FluxLauncher-Portable.exe`, and the native fixture before publishing them. Signing is optional for beta workflow runs and required for stable workflow runs. Configure the following GitHub Actions secrets before dispatching a stable release:

| Name | Purpose |
| --- | --- |
| `WINDOWS_SIGNING_CERTIFICATE_BASE64` | Base64-encoded PFX/PKCS#12 certificate containing the release signing certificate and private key |
| `WINDOWS_SIGNING_CERTIFICATE_PASSWORD` | Password for the PFX certificate |

The optional repository variable `WINDOWS_TIMESTAMP_URL` overrides the default RFC 3161 timestamp service. The private key must never be committed to the repository or placed in release assets.

Authenticode signing identifies the publisher and helps SmartScreen build reputation across releases signed with the same identity. It does not guarantee that a new file will avoid an initial SmartScreen warning: reputation also depends on clean download history. EV certificates do not provide an automatic SmartScreen bypass.
