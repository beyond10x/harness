#!/usr/bin/env python3
"""Verify each pinned provider-wire contract against its fixtures.

A contract that nothing checks is a description of what someone once intended. This makes the
fixtures load-bearing: change what the harness sends or accepts without re-pinning, and this fails
before the change reaches anyone.

The Rust side proves the other half -- that the harness actually produces these bytes. Neither
check is sufficient alone.
"""

from __future__ import annotations

import hashlib
import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
WIRES = ROOT / "contracts" / "provider-wires"

REQUIRED_KEYS = {
    "wire",
    "version",
    "transport",
    "endpoint",
    "streaming",
    "stateful",
    "request_fields",
    "stream_events",
    "output_items",
    "conformance",
    "files",
}

ALLOWED_CONFORMANCE = {"provider_emulated", "vendor_live"}


def check_version(directory: pathlib.Path, failures: list[str]) -> None:
    manifest_path = directory / "manifest.json"
    if not manifest_path.is_file():
        failures.append(f"{directory}: no manifest.json")
        return
    try:
        manifest = json.loads(manifest_path.read_text())
    except json.JSONDecodeError as error:
        # Named, not raised: a corrupted manifest is a contract failure like any other, and a
        # traceback reads as the checker breaking rather than the contract.
        failures.append(f"{manifest_path}: not JSON: {error}")
        return

    missing = REQUIRED_KEYS - manifest.keys()
    if missing:
        failures.append(f"{manifest_path}: missing keys {sorted(missing)}")
        return

    if manifest["wire"] != directory.parent.name:
        failures.append(
            f"{manifest_path}: wire `{manifest['wire']}` does not match its directory"
        )
    if manifest["version"] != directory.name:
        failures.append(
            f"{manifest_path}: version `{manifest['version']}` does not match its directory"
        )
    if manifest["conformance"] not in ALLOWED_CONFORMANCE:
        failures.append(
            f"{manifest_path}: conformance `{manifest['conformance']}` is not one of "
            f"{sorted(ALLOWED_CONFORMANCE)}"
        )

    recorded = {entry["path"] for entry in manifest["files"]}
    present = {
        f"fixtures/{path.name}" for path in sorted((directory / "fixtures").glob("*"))
    }
    for path in sorted(present - recorded):
        failures.append(f"{manifest_path}: `{path}` exists but is not recorded")
    for path in sorted(recorded - present):
        failures.append(f"{manifest_path}: `{path}` is recorded but missing")

    for entry in manifest["files"]:
        target = directory / entry["path"]
        if not target.is_file():
            continue
        body = target.read_bytes()
        if len(body) != entry["bytes"]:
            failures.append(
                f"{target}: {len(body)} bytes, manifest says {entry['bytes']}"
            )
        digest = hashlib.sha256(body).hexdigest()
        if digest != entry["sha256"]:
            failures.append(f"{target}: sha256 {digest}, manifest says {entry['sha256']}")

    check_stream_fixture(directory, manifest, failures)


def check_stream_fixture(
    directory: pathlib.Path, manifest: dict, failures: list[str]
) -> None:
    """Every event in the pinned stream must be one the manifest declares, and the reverse."""
    stream = directory / "fixtures" / "turn-stream.sse"
    if not stream.is_file():
        failures.append(f"{directory}: no fixtures/turn-stream.sse")
        return
    seen = set()
    for number, line in enumerate(stream.read_text().splitlines(), start=1):
        if not line.startswith("data: ") or line == "data: [DONE]":
            continue
        try:
            payload = json.loads(line[len("data: ") :])
        except json.JSONDecodeError as error:
            failures.append(f"{stream}:{number}: not JSON: {error}")
            return
        kind = payload.get("type")
        if not isinstance(kind, str):
            failures.append(f"{stream}: an event has no `type`")
            continue
        seen.add(kind)
    declared = set(manifest["stream_events"])
    for kind in sorted(seen - declared):
        failures.append(f"{stream}: event `{kind}` is not declared in the manifest")
    for kind in sorted(declared - seen):
        failures.append(f"{stream}: declared event `{kind}` never appears in the fixture")


def main() -> int:
    if not WIRES.is_dir():
        print(f"no provider-wire contracts under {WIRES}", file=sys.stderr)
        return 1

    failures: list[str] = []
    versions = 0
    for wire in sorted(path for path in WIRES.iterdir() if path.is_dir()):
        for version in sorted(path for path in wire.iterdir() if path.is_dir()):
            versions += 1
            check_version(version, failures)

    if versions == 0:
        print(f"no pinned wire versions under {WIRES}", file=sys.stderr)
        return 1

    for failure in failures:
        print(failure, file=sys.stderr)
    if failures:
        print(f"{len(failures)} provider-wire contract failures", file=sys.stderr)
        return 1
    print(f"provider-wire contracts: {versions} pinned version(s) verified")
    return 0


if __name__ == "__main__":
    sys.exit(main())
