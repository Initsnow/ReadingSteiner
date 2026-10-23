#!/usr/bin/env python3
"""
Generate Nix SRI hashes for ReadingSteiner GitHub Release assets.

Usage:
    python scripts/generate-nix-hashes.py [TAG] [--update] [--repo OWNER/REPO]

Examples:
    python scripts/generate-nix-hashes.py v0.1.0
    python scripts/generate-nix-hashes.py v0.1.0 --update
"""

import argparse
import base64
import hashlib
import json
import os
import re
import sys
import urllib.request
import urllib.error

TARGET_MAP = {
    "x86_64-linux": "x86_64-unknown-linux-musl",
    "aarch64-linux": "aarch64-unknown-linux-musl",
    "x86_64-darwin": "x86_64-apple-darwin",
    "aarch64-darwin": "aarch64-apple-darwin",
}


def get_version_from_cargo():
    cargo_path = os.path.join(os.path.dirname(__file__), "..", "Cargo.toml")
    with open(cargo_path, "r", encoding="utf-8") as f:
        for line in f:
            m = re.match(r'^\s*version\s*=\s*"([^"]+)"', line)
            if m:
                return m.group(1)
    return "0.1.0"


def fetch_url(url, token=None):
    headers = {"User-Agent": "ReadingSteiner-Nix-Hash-Generator"}
    if token:
        headers["Authorization"] = f"token {token}"
    req = urllib.request.Request(url, headers=headers)
    with urllib.request.urlopen(req) as response:
        return response.read()


def hex_to_sri(hex_str: str) -> str:
    cleaned = hex_str.strip().split()[0]
    raw_bytes = bytes.fromhex(cleaned)
    b64 = base64.b64encode(raw_bytes).decode("ascii")
    return f"sha256-{b64}"


def main():
    parser = argparse.ArgumentParser(description="Generate Nix SRI hashes for release assets")
    parser.add_argument("tag", nargs="?", default=None, help="Release tag (e.g. v0.1.0)")
    parser.add_argument("--repo", default=os.environ.get("GITHUB_REPOSITORY", "Initsnow/ReadingSteiner"), help="GitHub repo")
    parser.add_argument("--update", action="store_true", help="Update nix/binary.nix defaultHashes directly")
    parser.add_argument("--token", default=os.environ.get("GITHUB_TOKEN"), help="GitHub token for API access")
    args = parser.parse_args()

    tag = args.tag or os.environ.get("TAG")
    if not tag:
        v = get_version_from_cargo()
        tag = f"v{v}" if not v.startswith("v") else v

    print(f"Fetching release metadata for {args.repo} @ {tag}...")

    api_url = f"https://api.github.com/repos/{args.repo}/releases/tags/{tag}"
    try:
        data = json.loads(fetch_url(api_url, args.token).decode("utf-8"))
    except urllib.error.HTTPError as e:
        print(f"Error fetching release: HTTP {e.code} ({e.reason})", file=sys.stderr)
        if e.code == 404:
            print(f"Release tag '{tag}' does not exist on GitHub yet.", file=sys.stderr)
        sys.exit(1)

    assets = {a["name"]: a for a in data.get("assets", [])}

    hashes = {}
    clean_version = tag.lstrip("v")

    for system, target in TARGET_MAP.items():
        archive_name = f"reading-steiner-{tag}-{target}.tar.gz"
        sha_name = f"{archive_name}.sha256"

        if sha_name in assets:
            sha_url = assets[sha_name]["browser_download_url"]
            print(f"Downloading checksum for {system} ({target})...")
            content = fetch_url(sha_url, args.token).decode("utf-8")
            hashes[system] = hex_to_sri(content)
        elif archive_name in assets:
            archive_url = assets[archive_name]["browser_download_url"]
            print(f"Downloading archive for {system} ({target}) to calculate sha256...")
            archive_data = fetch_url(archive_url, args.token)
            h = hashlib.sha256(archive_data).hexdigest()
            hashes[system] = hex_to_sri(h)
        else:
            print(f"Warning: Neither '{archive_name}' nor '{sha_name}' found in release assets for {system}.", file=sys.stderr)

    print("\nCalculated Nix SRI hashes:")
    for sys_name, sri in hashes.items():
        print(f"  {sys_name} = \"{sri}\";")

    # Generate snippet
    nix_snippet = "  defaultHashes = {\n"
    for sys_name in ["x86_64-linux", "aarch64-linux", "x86_64-darwin", "aarch64-darwin"]:
        h = hashes.get(sys_name, "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=")
        nix_snippet += f'    {sys_name} = "{h}";\n'
    nix_snippet += "  };"

    # Update nix/binary.nix if requested
    if args.update and hashes:
        binary_nix_path = os.path.join(os.path.dirname(__file__), "..", "nix", "binary.nix")
        if os.path.exists(binary_nix_path):
            with open(binary_nix_path, "r", encoding="utf-8") as f:
                content = f.read()

            # Update version ? "..."
            content = re.sub(
                r'version \? "[^"]*"',
                f'version ? "{clean_version}"',
                content
            )

            # Update defaultHashes = { ... };
            content = re.sub(
                r'defaultHashes = \{[^}]*\};',
                nix_snippet.strip(),
                content,
                flags=re.DOTALL
            )

            with open(binary_nix_path, "w", encoding="utf-8") as f:
                f.write(content)
            print(f"\nSuccessfully updated {binary_nix_path} with new hashes for version {clean_version}!")

    # Write summary if GITHUB_STEP_SUMMARY is available
    summary_path = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary_path and hashes:
        with open(summary_path, "a", encoding="utf-8") as f:
            f.write(f"\n### 🚀 Nix Flake Binary Hashes for `{tag}`\n\n")
            f.write("To use precompiled binaries without local compilation, update `nix/binary.nix` or configure:\n\n")
            f.write("```nix\n")
            f.write("services.reading-steiner = {\n")
            f.write("  enable = true;\n")
            f.write("  useBinary = true;\n")
            f.write("};\n")
            f.write("```\n\n")
            f.write("#### SRI Hashes:\n")
            f.write("```nix\n")
            f.write(nix_snippet + "\n")
            f.write("```\n")

    return 0


if __name__ == "__main__":
    sys.exit(main())
