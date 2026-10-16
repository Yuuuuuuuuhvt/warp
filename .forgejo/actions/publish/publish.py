"""先核对完整平台产物，再发布 Forgejo，仅对保留回退的公开分叉发布 GitHub 附件。"""

import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import urllib.error
import urllib.parse
import urllib.request

ALLOWED = {"aria", "fiato", "warp", "solfeggio"}
# GitHub 原仓库归档后只读，只有仍在 GitHub 维护的公开分叉继续接收回退发布。
GITHUB_FALLBACK = {"warp"}
FORGEJO = "https://git.luonto.me"
GITHUB = "https://api.github.com"


class Redirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, response, code, message, headers, new_url):
        old = urllib.parse.urlsplit(request.full_url)
        new = urllib.parse.urlsplit(new_url)
        if new.scheme != "https":
            raise RuntimeError("Release redirect requires HTTPS")
        if old.hostname == "git.luonto.me" and new.hostname != old.hostname:
            raise RuntimeError("Forgejo release redirect changed origin")
        result = super().redirect_request(request, response, code, message, headers, new_url)
        if old.hostname != new.hostname:
            result.remove_header("Authorization")
        return result


def digest(path):
    h = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def verified_files(name, directory):
    """完整平台与校验和核对可在推送 main 之前执行，不依赖发布凭据。"""
    if name not in ALLOWED:
        raise RuntimeError("Repository is outside the release allowlist")
    directory = Path(directory)
    paths = [p for p in directory.rglob("*") if p.is_file()]
    if any(p.is_symlink() for p in paths) or len({p.name for p in paths}) != len(paths):
        raise RuntimeError("Release artifacts contain symbolic links or duplicate filenames")
    files = {p.name: p for p in paths}
    if any(not (n.endswith((".tar.gz", ".zip", ".deb", ".pkg.tar.zst", ".sha256")) or n == "SHA256SUMS") for n in files):
        raise RuntimeError("Unexpected file in release artifact directory")
    archives = [n for n in files if not n.endswith(".sha256") and n != "SHA256SUMS"]
    if not archives or any(n + ".sha256" not in files for n in archives):
        raise RuntimeError("Every release asset needs a checksum")
    for n in archives:
        if digest(files[n]) != files[n + ".sha256"].read_text().split()[0]:
            raise RuntimeError("Release checksum mismatch: " + n)
    expected = {"aria": ["aarch64-apple-darwin.tar.gz", "aarch64-unknown-linux-musl.tar.gz", "x86_64-unknown-linux-musl.tar.gz"],
                "fiato": ["x86_64-unknown-linux-musl.tar.gz"],
                "warp": [".zip", ".deb", ".pkg.tar.zst"],
                "solfeggio": [".zip", ".deb", ".pkg.tar.zst"]}[name]
    if any(not any(n.endswith(suffix) for n in archives) for suffix in expected):
        raise RuntimeError("Release is missing a required platform")
    return files


def main():
    name = os.environ["RELEASE_REPOSITORY"].rsplit("/", 1)[-1]
    tag = os.environ["RELEASE_TAG"]
    if not tag or "/" in tag:
        raise RuntimeError("Invalid release tag")
    files = verified_files(name, os.environ["RELEASE_DIRECTORY"])
    opener = urllib.request.build_opener(Redirect())
    providers = [(FORGEJO + "/api/v1/repos/yurun/" + name, os.environ["RELEASE_WRITE_TOKEN"], "token ")]
    if name in GITHUB_FALLBACK:
        providers.append((GITHUB + "/repos/Yuuuuuuuuhvt/" + name, os.environ["GITHUB_RELEASE_SYNC_TOKEN"], "Bearer "))
    releases = []
    for base, token, prefix in providers:
        def api(path, method="GET", body=None):
            req = urllib.request.Request(base + path, method=method,
                data=json.dumps(body).encode() if body is not None else None,
                headers={"Authorization": prefix + token, "Content-Type": "application/json", "Accept": "application/json"})
            with opener.open(req, timeout=60) as response:
                data = response.read()
                return json.loads(data) if data else None
        # 发布中断留下的草稿也必须复用，按标签查找接口可能只返回已发布条目。
        releases_in_repo = api("/releases?per_page=100&limit=100")
        release = next((r for r in releases_in_repo if r["tag_name"] == tag), None)
        if release is None:
            release = api("/releases", "POST", {"tag_name": tag, "target_commitish": os.environ["RELEASE_COMMIT"],
                "name": name + " " + tag, "body": "Verified multi-platform build " + os.environ["RELEASE_COMMIT"],
                "draft": True, "prerelease": os.environ.get("RELEASE_PRERELEASE") == "true"})
        for filename, path in files.items():
            existing = next((a for a in release.get("assets", []) if a["name"] == filename), None)
            if existing:
                if existing.get("digest"):
                    if existing["digest"] != "sha256:" + digest(path):
                        raise RuntimeError("Published asset differs: " + filename)
                else:
                    with opener.open(urllib.request.Request(existing["browser_download_url"],
                            headers={"Authorization": prefix + token}), timeout=600) as response:
                        h = hashlib.sha256()
                        for chunk in iter(lambda: response.read(1 << 20), b""):
                            h.update(chunk)
                    if h.hexdigest() != digest(path):
                        raise RuntimeError("Published asset differs: " + filename)
                continue
            with tempfile.NamedTemporaryFile(mode="w") as cfg:
                cfg.write('header = "Authorization: ' + prefix + token + '"\n');cfg.flush()
                if base.startswith(FORGEJO):
                    args = ["--form", "attachment=@" + str(path), base + "/releases/" + str(release["id"]) + "/assets"]
                else:
                    url = release["upload_url"].split("{", 1)[0] + "?name=" + urllib.parse.quote(filename, safe="")
                    args = ["--header", "Content-Type: application/octet-stream", "--data-binary", "@" + str(path), url]
                subprocess.run(["curl", "--silent", "--show-error", "--fail", "--max-time", "600", "--config", cfg.name, *args],
                               check=True, stdout=subprocess.DEVNULL)
        releases.append((base, token, prefix, release["id"], release.get("draft")))
    # 目标仓库所有附件完成后按逆序公开发布，存在回退时先公开回退再公开主仓，主仓 latest 不能先于回退可用。
    for base, token, prefix, release_id, draft in reversed(releases):
        if draft:
            req = urllib.request.Request(base + "/releases/" + str(release_id), method="PATCH", data=b'{"draft":false}',
                    headers={"Authorization": prefix + token, "Content-Type": "application/json"})
            with opener.open(req, timeout=60) as response:
                response.read()
    if name in GITHUB_FALLBACK:
        print("Verified assets published to Forgejo and GitHub fallback")
    else:
        print("Verified assets published to Forgejo")


if __name__ == "__main__":
    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    if args.check:
        files = verified_files(os.environ["RELEASE_REPOSITORY"], os.environ["RELEASE_DIRECTORY"])
        print("Verified complete platform set: " + str(len(files)) + " files")
    else:
        main()
