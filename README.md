# WarpOss self build

This branch is the default branch of the fork and holds only the build workflow. It does not
contain Warp's source code.

## Purpose

The official Warp selects every tab opened through a `warp://tab_config/<name>` link, which moves
keyboard focus away from the tab the user is typing in. This fork adds `activate=false` and
`placement` link parameters ([warpdotdev/warp#16256](https://github.com/warpdotdev/warp/issues/16256))
so agent sessions can open visible tabs in the background, and fixes the hover-revealed tab bar that
makes the window jump in fullscreen.

## Branches

| Branch | Role |
| --- | --- |
| `master` | Mirror of upstream. Not used by the build; do not push to it. |
| `main` | Upstream plus every change of this fork. Builds always use it. |
| `feat/*`, `fix/*` | One change each, based on `main` and also usable for upstream pull requests. |
| `self-build` | This branch: the workflow that keeps `main` in sync and builds it. |

## Daily build

`.github/workflows/warposs-daily.yml` runs at 02:00 Asia/Shanghai and can be started manually.
It checks out `main`, merges upstream `master`, runs the workspace and URI tests, builds the OSS
channel app and signs it with a self-signed code signing identity. Only then does it push the
merged `main` and publish `WarpOss.zip` with its SHA-256 as a release tagged
`warposs-<date>-<main sha>`. A run is skipped when the merged `main` already has a release.

A merge conflict or a failing test or build stops the run before anything is pushed; GitHub emails
the failure and machines keep the previous release. Resolve a conflict by merging upstream `master`
into `main` locally, running the tests and pushing `main`, then start the workflow manually.

## Developing a change

Branch from the latest `main`. When the change is ready, rebase it onto the current `main`, since
the daily merge moves `main` forward, run the tests again, then merge it into `main` and push. The
next daily build includes it, or start the workflow manually.

## Secrets

| Secret | Use |
| --- | --- |
| `WARP_CODESIGN_P12_BASE64`, `WARP_CODESIGN_P12_PASSWORD` | Signing identity. Every build must use the same certificate: macOS ties granted permissions to the certificate in the designated requirement. |
| `WARPOSS_SYNC_TOKEN` | Fine-grained token limited to this repository with Contents and Workflows write access. Merging upstream brings in upstream's workflow file changes, which the built-in `GITHUB_TOKEN` may never push. It is used only by the push step. |

The input classifier models are Git LFS objects fetched from upstream's LFS storage; the fork does
not store copies, so its LFS bandwidth quota is not spent.
