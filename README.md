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

## 每日构建与发布

TX 的定时任务每六小时触发 `.github/workflows/warposs-daily.yml`，时间为北京时间 00:00、06:00、12:00、18:00。工作流本身没有 schedule 触发器，也可手动执行 workflow_dispatch。当天已有全部平台的包与校验文件时跳过；force 输入强制重建。

1. prepare 固定 main 与 upstream master 的提交及合并时间，所有构建重建同一合并提交。它读取 main 中的 `script/linux/oss/platforms.json`，为尚缺产物或校验文件的发行版生成 Linux 构建矩阵。
2. macos 在既有 macOS 自托管执行器测试并构建 OSS 应用，再使用固定证书签名。
3. linux 在 TX 自托管执行器执行 `script/linux/bundle_oss <平台> <版本>`。Arch 在宿主原生编译、Arch 容器打包；Ubuntu 在 Ubuntu 24.04 容器内编译并生成 .deb，避免依赖 Arch 的 glibc。Ubuntu 与 Arch 的 Cargo 和 target 缓存相互独立，每个平台的 release-lto 超过 25 GiB 时才清理。构建复用执行器已有 HTTP 代理，不向容器传递发布凭据。
4. publish 下载成功产物，核验非空且全部 SHA-256 一致后，推送合并后的 main 并创建或补齐 `warposs-<日期>-<main 提交>` 发布。Linux 矩阵中部分失败不会阻止成功发行版的产物发布。

发布附件包括 `WarpOss.zip`、`warp-terminal-oss-<版本>-1-x86_64.pkg.tar.zst`、`warp-terminal-oss_<版本>_amd64.deb` 及每个包对应的 `.sha256`。包版本为 UTC 构建日期加 main 提交总数。Ubuntu 包在发布前检查包名、版本、架构、root 属主及动态库依赖，并从实际二进制生成最低共享库版本约束。OSS 的 .deb 不登记官方 Warp apt 源。

Linux 包没有独立签名，安装方通过 GitHub HTTPS 获取包并核对校验和。合并冲突在 prepare 阶段停止。缺少本平台产物时，设备保留已安装版本；下一次运行会补建缺失附件。源码修改直接合入 main，工作流修改更新 self-build，不能把 self-build 整体合入 main。

源码仓库现有 `script/linux/test_bundle_warpctrl` 在 Ubuntu 上同时检查 .deb 维护脚本和依赖元数据。完整发布验收还需要执行真实构建、在目标 Ubuntu 安装，并确认图形窗口与常驻开销；单独通过脚本测试不能替代实机验收。

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
