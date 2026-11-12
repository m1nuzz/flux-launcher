# Flux Launcher Privacy Policy

Last updated: November 11, 2026

Flux Launcher is a Windows launcher published by m1nuzz. This policy describes what the application stores on your device and what can leave it.

## Summary

- Flux has no telemetry, analytics, advertising, accounts, or user tracking.
- Settings, search history, and usage counters are stored locally on your device.
- Flux checks GitHub for stable updates and downloads them when you ask it to.
- Google Search is an optional provider. It prepares a Google search result, and only opens that search in your default browser when you choose it. Google then receives the query.
- Everything and Obsidian providers work locally.
- Community plugins are separate third-party programs. Their own privacy practices apply.

## Data stored locally

Flux stores its configuration in `%APPDATA%\FluxLauncher\settings.json`. This includes your preferences, query history, saved launch counts, and other launcher state. This data stays on your device; Flux does not send it to the publisher. You can clear query history and launch counters in Settings, and you can remove the settings file at any time.

## Network connections

### Update checks and downloads

When update checks are enabled, Flux requests the latest stable release information from the GitHub API at `api.github.com`. If you choose to install an update, Flux downloads the installer from GitHub. GitHub receives standard request information such as your IP address and user agent, subject to GitHub's privacy statement. Update checks can be disabled in Settings > General > Updates.

### Google Search

The Google Search provider is enabled by default and can be disabled in Settings > Plugins. It does not send a request by itself: it prepares a result that opens `https://www.google.com/search?q=...` in your default browser. The request is made only when you choose that result. Google then receives the query and standard browser request information, subject to Google's privacy policy.

### Everything and Obsidian

The Everything file search provider communicates with Everything over local IPC. The Obsidian provider reads the vault you configure. Flux does not send this data to the publisher.

### Community plugins

Native Rust plugins and Flow plugins are separate programs installed by you. Flux forwards the current query to them so they can return results. A plugin may contact its own servers; that behavior and its privacy policy belong to the plugin author. Install plugins only from sources you trust.

## What Flux does not do

- It does not sell, rent, or share your data with advertisers or data brokers.
- It does not require an account or collect personal information for the publisher.
- It does not collect analytics or crash telemetry.

## Security

Flux stores its local settings using the normal Windows user profile protections. It does not operate a server that stores your launcher data.

## Changes

If this policy changes, the updated version will be published in this repository with a new date.

## Contact

Questions about this policy can be raised in the public issue tracker: https://github.com/m1nuzz/flux-launcher/issues
