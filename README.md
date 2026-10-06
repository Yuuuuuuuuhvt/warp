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

`.github/workflows/warposs-daily.yml` runs every six hours, at 00:00, 06:00, 12:00 and 18:00 Asia/Shanghai, and can be started manually. A scheduled run skips when a release holding both platforms was created since 00:00 Asia/Shanghai that day, so the 00:00 slot always builds and the later slots only retry a failed day.
It has four jobs:

1. `prepare` pins the current `main` and upstream `master` commits, merges them, and decides which
   platforms still lack an artifact in the release of the merged commit.
2. `macos` rebuilds the same merge, runs the workspace and URI tests, builds the OSS channel app and
   signs it with a self-signed code signing identity.
3. `linux` rebuilds the same merge on Ubuntu with upstream's Linux build script, then assembles an
   Arch package `warp-terminal-oss-<date>.<main commit count>-1-x86_64.pkg.tar.zst` in an
   `archlinux:base-devel` container, because `makepkg` only exists on Arch. The package is not
   signed and the Linux job does not run the tests, which `macos` already ran on the same commit.
4. `publish` runs when at least one platform built. It pushes the merged `main`, then creates the
   release `warposs-<date>-<main sha>` or adds the missing artifacts to it: `WarpOss.zip`, the
   Arch package and a SHA-256 file for each.

The platforms are independent. A failure on one does not stop the other from being published, and
the next run builds the missing platform for the same `main` commit and adds it to the existing
release. A merge conflict stops the run in `prepare`, before anything is built or pushed. Only
`publish` pushes `main`, so `main` always points at a commit that has a release.

Every job rebuilds the merge commit from the pinned parents and dates and fails if its hash differs
from the one `prepare` produced, so both platforms are built from one commit even though the merge
is pushed only after the builds.

A failing test or build fails the run; GitHub emails the failure and machines keep the previous
release. Resolve a conflict by merging upstream `master` into `main` locally, running the tests and
pushing `main`, then start the workflow manually. The `force` input rebuilds both platforms and
replaces the artifacts of an existing release.

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
