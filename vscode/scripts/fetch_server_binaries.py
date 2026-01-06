#!/usr/bin/env python3
"""
Fetch prebuilt beancount-language-server binaries for packaging the VS Code extension.

Downloads release assets from GitHub, verifies SHA256 sums, and lays them out under
server/<triplet>/. All extraction is handled with Python libraries so no system tar
or unzip executables are required.

Environment Variables:
    BLS_VERSION: Specific release version to download (e.g., "1.4.1"). If not set,
                 fetches the latest release from GitHub.
    BLS_TARGETS: Comma-separated list of target triplets to download (e.g.,
                 "x86_64-pc-windows-msvc,aarch64-apple-darwin"). If not set, downloads
                 all default targets unless VSCE_TARGET or BLS_NO_BUNDLE is set.
    BLS_NO_BUNDLE: Set to any value to skip downloading binaries entirely.
    VSCE_TARGET: VS Code platform identifier (e.g., "win32-x64"). If set, only downloads
                 the binary for that platform.
    GITHUB_TOKEN: Optional GitHub API token for authenticated requests (increases rate limits).
"""

import gzip
import hashlib
import os
import shutil
import stat
import sys
import tarfile
import zipfile
from pathlib import Path
from typing import Dict, List

import requests

REPO = "polarmutex/beancount-language-server"

DEFAULT_TARGETS: Dict[str, Dict[str, str]] = {
    "win32-x64": {
        "triplet": "x86_64-pc-windows-msvc",
        "archive": "beancount-language-server-windows-x64.gz",
        "binary": "beancount-language-server.exe",
    },
    "darwin-arm64": {
        "triplet": "aarch64-apple-darwin",
        "archive": "beancount-language-server-macos-arm64.gz",
        "binary": "beancount-language-server",
    },
    "linux-x64": {
        "triplet": "x86_64-unknown-linux-gnu",
        "archive": "beancount-language-server-linux-x64.gz",
        "binary": "beancount-language-server",
    },
    "linux-arm64": {
        "triplet": "aarch64-unknown-linux-gnu",
        "archive": "beancount-language-server-linux-arm64.gz",
        "binary": "beancount-language-server",
    },
}

TARGET_BY_TRIPLET: Dict[str, Dict[str, str]] = {
    cfg["triplet"]: cfg for cfg in DEFAULT_TARGETS.values()
}


def request_headers(token: str | None) -> Dict[str, str]:
    headers = {"User-Agent": "beancount-language-server-vscode-build"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    return headers


def fetch_latest_release_version(headers: Dict[str, str]) -> str:
    url = f"https://api.github.com/repos/{REPO}/releases/latest"
    resp = requests.get(url, headers=headers, timeout=60)
    resp.raise_for_status()
    data = resp.json()
    tag = data.get("tag_name")
    if not tag or not isinstance(tag, str):
        raise RuntimeError("Unable to read tag_name from latest release response")
    return tag[1:] if tag.startswith("v") else tag


def sha256_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def parse_checksum(text: str) -> str:
    for token in text.strip().split():
        if len(token) == 64 and all(c in "0123456789abcdefABCDEF" for c in token):
            return token.lower()
    raise ValueError("Unable to parse checksum file")


def read_cached_checksum(path: Path) -> str | None:
    if not path.exists():
        return None
    try:
        return parse_checksum(path.read_text(encoding="utf-8"))
    except Exception as err:
        print(f"Cached checksum invalid at {path}: {err}", file=sys.stderr)
        return None


def download_checksum(url: str, headers: Dict[str, str]) -> str:
    try:
        resp = requests.get(url, headers=headers, timeout=60)
        resp.raise_for_status()
    except requests.HTTPError as err:
        if err.response is not None and err.response.status_code == 404:
            raise RuntimeError(
                f"Checksum not found at {url}. The release may be missing this asset; set BLS_VERSION to a release with binaries for your target."
            ) from err
        raise
    return parse_checksum(resp.text)


def ensure_checksum(path: Path, url: str, headers: Dict[str, str]) -> str:
    cached = read_cached_checksum(path)
    if cached:
        return cached
    expected = download_checksum(url, headers)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(f"{expected}  {path.name}\n", encoding="utf-8")
    return expected


def download(url: str, destination: Path, headers: Dict[str, str]) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    try:
        resp = requests.get(url, headers=headers, timeout=120)
        resp.raise_for_status()
    except requests.HTTPError as err:
        if err.response is not None and err.response.status_code == 404:
            raise RuntimeError(
                f"Archive not found at {url}. The release may be missing this asset; set BLS_VERSION to a release with binaries for your target."
            ) from err
        raise
    destination.write_bytes(resp.content)


def extract_archive(archive_path: Path, destination: Path, binary_name: str) -> None:
    destination.mkdir(parents=True, exist_ok=True)

    if archive_path.suffix == ".zip":
        with zipfile.ZipFile(archive_path) as zf:
            zf.extractall(destination)
        return

    if archive_path.suffix == ".gz":
        # For .gz files, extract to the specified binary name
        output_path = destination / binary_name
        with gzip.open(archive_path, "rb") as gz_file:
            output_path.write_bytes(gz_file.read())
        return

    suffixes = archive_path.suffixes
    if suffixes[-2:] == [".tar", ".gz"]:
        mode = "r:gz"
    elif suffixes[-2:] == [".tar", ".xz"]:
        mode = "r:xz"
    elif archive_path.suffix == ".tar":
        mode = "r:"
    else:
        raise RuntimeError(f"Unsupported archive format: {archive_path.name}")

    with tarfile.open(archive_path, mode) as tf:
        tf.extractall(destination, filter="data")


def ensure_executable(binary_path: Path) -> None:
    try:
        current = binary_path.stat().st_mode
        binary_path.chmod(current | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
    except OSError:
        # Chmod is best effort on platforms that support POSIX permissions.
        pass


def locate_binary(target_dir: Path, binary_name: str) -> Path | None:
    direct = target_dir / binary_name
    if direct.exists():
        return direct

    for candidate in target_dir.rglob(binary_name):
        if candidate.is_file():
            return candidate
    return None


def resolve_targets() -> List[Dict[str, str]]:
    if os.environ.get("BLS_NO_BUNDLE"):
        return []

    explicit = os.environ.get("BLS_TARGETS")
    if explicit:
        triplets = [
            triplet.strip() for triplet in explicit.split(",") if triplet.strip()
        ]
        targets: List[Dict[str, str]] = []
        for triplet in triplets:
            cfg = TARGET_BY_TRIPLET.get(triplet)
            if not cfg:
                raise RuntimeError(f"Unsupported target triplet: {triplet}")
            targets.append(cfg)
        return targets

    vsce_target = os.environ.get("VSCE_TARGET")
    if vsce_target:
        cfg = DEFAULT_TARGETS.get(vsce_target)
        if not cfg:
            raise RuntimeError(f"Unsupported VSCE target: {vsce_target}")
        return [cfg]

    return list(DEFAULT_TARGETS.values())


def clean_server(root: Path) -> None:
    shutil.rmtree(root / "server", ignore_errors=True)


def download_and_extract(
    root: Path, version: str, cfg: Dict[str, str], headers: Dict[str, str]
) -> None:
    base_url = f"https://github.com/{REPO}/releases/download/{version}"
    archive_url = f"{base_url}/{cfg['archive']}"
    checksum_url = f"{archive_url}.sha256"

    target_dir = root / "server" / cfg["triplet"]
    cache_dir = root / ".cache" / "binaries" / version / cfg["triplet"]
    archive_path = cache_dir / cfg["archive"]
    checksum_path = archive_path.with_suffix(archive_path.suffix + ".sha256")

    expected = ensure_checksum(checksum_path, checksum_url, headers)

    needs_download = True
    if archive_path.exists():
        actual = sha256_file(archive_path)
        if actual == expected:
            needs_download = False
        else:
            print(
                f"Cached archive checksum mismatch for {cfg['archive']}, redownloading.",
                file=sys.stderr,
            )
            archive_path.unlink(missing_ok=True)

    if needs_download:
        print(f"Downloading {archive_url}")
        download(archive_url, archive_path, headers)
        actual = sha256_file(archive_path)
        if actual != expected:
            archive_path.unlink(missing_ok=True)
            raise RuntimeError(
                f"Checksum mismatch for {cfg['archive']}: expected {expected} got {actual}"
            )

    extract_archive(archive_path, target_dir, cfg["binary"])

    located = locate_binary(target_dir, cfg["binary"])
    if not located:
        raise RuntimeError(
            f"Binary {cfg['binary']} missing after extracting {cfg['archive']}"
        )

    binary_path = target_dir / cfg["binary"]
    if located != binary_path:
        target_dir.mkdir(parents=True, exist_ok=True)
        binary_path.write_bytes(located.read_bytes())

    ensure_executable(binary_path)
    print(f"Prepared {binary_path}")


def main() -> None:
    script_root = Path(__file__).resolve().parent
    root = (script_root / "..").resolve()

    token = os.environ.get("GITHUB_TOKEN")
    headers = request_headers(token)
    version = os.environ.get("BLS_VERSION") or fetch_latest_release_version(headers)

    targets = resolve_targets()
    if not targets:
        print("BLS_NO_BUNDLE set; skipping binary download")
        return

    clean_server(root)

    for cfg in targets:
        download_and_extract(root, version, cfg, headers)


if __name__ == "__main__":
    main()
