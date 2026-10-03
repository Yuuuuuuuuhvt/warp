# WarpOss self build

This branch is the default branch of the fork and holds only what the daily build needs. It does
not contain Warp's source code.

## Purpose

The official Warp selects every tab opened through a `warp://tab_config/<name>` link, which moves
keyboard focus away from the tab the user is typing in. The patches here add `activate=false` and
`placement` link parameters ([warpdotdev/warp#16256](https://github.com/warpdotdev/warp/issues/16256))
so agent sessions can open visible tabs in the background.

## Daily build

`.github/workflows/warposs-daily.yml` runs at 02:00 Asia/Shanghai and can be started manually.
It checks out upstream `master`, applies `patches/*.patch` with `git am`, builds the OSS channel
app, signs it with a self-signed code signing identity and publishes
`WarpOss.zip` with its SHA-256 as a release tagged `warposs-<date>-<upstream sha>`. A run is skipped
when the upstream commit already has a release.

Signing needs two repository secrets, `WARP_CODESIGN_P12_BASE64` and `WARP_CODESIGN_P12_PASSWORD`.
Every build must use the same certificate: macOS ties granted permissions to the certificate in the
designated requirement, so a new certificate makes the app look like a different application.

## Updating the patches

When `git am` fails, upstream changed code the patches touch. Rebase the patch branch
`feat/tab-config-background-open` onto upstream `master`, run the affected tests, then regenerate
the patch files from it:

```sh
git format-patch upstream/master..feat/tab-config-background-open -o patches
```
