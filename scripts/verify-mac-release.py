#!/usr/bin/env python3
"""Verify matching signed macOS ZIP and DMG release artifacts."""

from __future__ import annotations

import argparse
import hashlib
import json
import plistlib
import subprocess
import sys
import tempfile
from pathlib import Path


APP_NAME = "U-King.app"
COMMAND_TIMEOUT_SECONDS = 120


class VerificationError(RuntimeError):
    pass


def run(command: list[str]) -> bytes:
    try:
        return subprocess.run(
            command,
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=COMMAND_TIMEOUT_SECONDS,
        ).stdout
    except FileNotFoundError as error:
        raise VerificationError(f"required tool is unavailable: {command[0]}") from error
    except subprocess.TimeoutExpired as error:
        raise VerificationError(f"command timed out after {COMMAND_TIMEOUT_SECONDS}s: {' '.join(command[:2])}") from error
    except subprocess.CalledProcessError as error:
        raise VerificationError(f"command failed ({error.returncode}): {' '.join(command[:2])}") from error


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def app_at(root: Path) -> Path:
    candidates = [path for path in root.iterdir() if path.name == APP_NAME and path.is_dir() and not path.is_symlink()]
    if len(candidates) != 1:
        raise VerificationError(f"expected exactly one top-level {APP_NAME}")
    return candidates[0]


def app_metadata(app: Path, expected_version: str) -> tuple[str, str]:
    plist_path = app / "Contents" / "Info.plist"
    try:
        with plist_path.open("rb") as source:
            plist = plistlib.load(source)
    except (FileNotFoundError, plistlib.InvalidFileException) as error:
        raise VerificationError(f"invalid {APP_NAME} Info.plist") from error
    version = plist.get("CFBundleShortVersionString")
    executable = plist.get("CFBundleExecutable")
    if not isinstance(version, str) or version != expected_version:
        raise VerificationError(f"{APP_NAME} version does not equal {expected_version}")
    if not isinstance(executable, str) or not executable or Path(executable).name != executable:
        raise VerificationError(f"invalid {APP_NAME} executable name")
    executable_path = app / "Contents" / "MacOS" / executable
    if not executable_path.is_file() or executable_path.is_symlink():
        raise VerificationError(f"missing {APP_NAME} executable")
    return version, sha256(executable_path)


def attach_readonly(dmg: Path) -> Path:
    raw = run(["hdiutil", "attach", "-readonly", "-nobrowse", "-plist", str(dmg)])
    try:
        entities = plistlib.loads(raw).get("system-entities", [])
        mount_points = [Path(entity["mount-point"]) for entity in entities if isinstance(entity, dict) and "mount-point" in entity]
    except (plistlib.InvalidFileException, KeyError, TypeError) as error:
        raise VerificationError("hdiutil did not return a mount point") from error
    if len(mount_points) != 1:
        raise VerificationError("DMG must mount exactly one volume")
    return mount_points[0]


def artifact(path: Path, app_version: str, executable_sha256: str) -> dict[str, object]:
    if not path.is_file():
        raise VerificationError(f"missing artifact: {path.name}")
    return {
        "file": path.name,
        "sha256": sha256(path),
        "bytes": path.stat().st_size,
        "appVersion": app_version,
        "executableSha256": executable_sha256,
    }


def verify_zip(zip_path: Path, expected_version: str) -> tuple[str, str]:
    with tempfile.TemporaryDirectory(prefix="uking-mac-zip-") as temporary:
        root = Path(temporary)
        run(["ditto", "-x", "-k", str(zip_path), str(root)])
        app = app_at(root)
        run(["codesign", "--verify", "--deep", "--strict", str(app)])
        return app_metadata(app, expected_version)


def verify_dmg(dmg_path: Path, expected_version: str) -> tuple[str, str]:
    mount_point = attach_readonly(dmg_path)
    try:
        app = app_at(mount_point)
        run(["codesign", "--verify", "--deep", "--strict", str(app)])
        return app_metadata(app, expected_version)
    finally:
        try:
            run(["hdiutil", "detach", str(mount_point)])
        except VerificationError:
            run(["hdiutil", "detach", "-force", str(mount_point)])


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--zip", required=True, type=Path)
    parser.add_argument("--dmg", required=True, type=Path)
    parser.add_argument("--version", required=True)
    parser.add_argument("--source-commit", required=True)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()

    if not args.version.strip() or not args.source_commit or any(char not in "0123456789abcdef" for char in args.source_commit.lower()) or len(args.source_commit) != 40:
        raise VerificationError("version or source commit is invalid")

    zip_version, zip_executable_sha256 = verify_zip(args.zip, args.version)
    dmg_version, dmg_executable_sha256 = verify_dmg(args.dmg, args.version)
    if zip_version != dmg_version or zip_executable_sha256 != dmg_executable_sha256:
        raise VerificationError("ZIP and DMG app contents do not match")

    proof = {
        "schema": 1,
        "version": args.version,
        "sourceCommit": args.source_commit,
        "zip": artifact(args.zip, zip_version, zip_executable_sha256),
        "dmg": artifact(args.dmg, dmg_version, dmg_executable_sha256),
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    temporary = args.output.with_suffix(args.output.suffix + ".tmp")
    temporary.write_text(json.dumps(proof, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    temporary.replace(args.output)
    print(json.dumps(proof, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except VerificationError as error:
        print(f"mac release verification failed: {error}", file=sys.stderr)
        raise SystemExit(1)
