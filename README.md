# mira

Mira is a macOS desktop automation client for AdventureQuest Worlds, built with GPUI and an embedded Ruffle player.

## Packaging for macOS

On an Apple silicon Mac, run `pnpm install --frozen-lockfile`, then `scripts/package-app`. It builds the version in the root `Cargo.toml`, signs it ad hoc, and writes `Mira.dmg` and `Mira.zip` to `target/releases/<version>/`. You don't need an Apple Developer account.

To ship a version, follow Releasing in `AGENTS.md`: `pnpm release changes` lists the commits since the last release, and `pnpm release publish <version> --notes <file>` bumps the version, packages the app, and publishes both files with the condensed notes and the full changelog.

People install from the DMG by dragging Mira to Applications. macOS asks them to approve it in Privacy & Security on the first launch.

Mira checks the latest release at launch and every 4 hours unless Check automatically is off in Settings. A newer version shows up in the Settings footer and at the bottom of the client and Manager sidebars. Nothing downloads until you click it. Mira then downloads `Mira.zip`, compares it against the SHA-256 that GitHub reports for the asset, and unpacks it next to the installed app. Restart to update swaps the app in place and opens the new version. Updates need Mira to sit in a folder you can write to, so they're off for a copy that macOS runs from a translocated path.
