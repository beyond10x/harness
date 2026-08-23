#!/usr/bin/env python3
"""Verify each pinned app-server profile against its trace.

Bridge mode only works because two components agree on a method inventory without sharing a crate.
A protocol is the right seam for that, but only while something checks it: this makes the pinned
trace load-bearing, so widening what the server emits without re-pinning fails here rather than at
a bridge that stops driving it.

The Rust side proves the other half -- that the server's own constants match this manifest.
"""

from __future__ import annotations

import hashlib
import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
PROFILES = ROOT / "contracts" / "app-server-profile"

REQUIRED_KEYS = {
    "profile",
    "version",
    "product",
    "transport",
    "client_methods",
    "refused_client_methods",
    "server_methods",
    "terminal_statuses",
    "dynamic_tool_item",
    "conformance",
    "files",
}


def check_version(directory: pathlib.Path, failures: list[str]) -> None:
    manifest_path = directory / "manifest.json"
    if not manifest_path.is_file():
        failures.append(f"{directory}: no manifest.json")
        return
    manifest = json.loads(manifest_path.read_text())

    missing = REQUIRED_KEYS - manifest.keys()
    if missing:
        failures.append(f"{manifest_path}: missing keys {sorted(missing)}")
        return

    if manifest["profile"] != directory.parent.name:
        failures.append(f"{manifest_path}: profile does not match its directory")
    if manifest["version"] != directory.name:
        failures.append(f"{manifest_path}: version does not match its directory")

    # A server naming itself after the vendor makes an incident unreadable, so the manifest may
    # not claim to be one either.
    if not manifest["product"].startswith("b10x"):
        failures.append(
            f"{manifest_path}: product `{manifest['product']}` must name this implementation"
        )

    overlap = set(manifest["client_methods"]) & set(manifest["refused_client_methods"])
    if overlap:
        failures.append(
            f"{manifest_path}: {sorted(overlap)} are both served and refused"
        )

    check_files(directory, manifest, manifest_path, failures)
    check_trace(directory, manifest, failures)


def check_files(
    directory: pathlib.Path,
    manifest: dict,
    manifest_path: pathlib.Path,
    failures: list[str],
) -> None:
    recorded = {entry["path"] for entry in manifest["files"]}
    present = {f"fixtures/{path.name}" for path in sorted((directory / "fixtures").glob("*"))}
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
            failures.append(f"{target}: {len(body)} bytes, manifest says {entry['bytes']}")
        digest = hashlib.sha256(body).hexdigest()
        if digest != entry["sha256"]:
            failures.append(f"{target}: sha256 {digest}, manifest says {entry['sha256']}")


def check_trace(directory: pathlib.Path, manifest: dict, failures: list[str]) -> None:
    trace_path = directory / "fixtures" / "walking-trace.jsonl"
    if not trace_path.is_file():
        failures.append(f"{directory}: no fixtures/walking-trace.jsonl")
        return

    client_declared = set(manifest["client_methods"])
    refused_declared = set(manifest["refused_client_methods"])
    server_declared = set(manifest["server_methods"])
    seen_client: set[str] = set()
    seen_server: set[str] = set()
    outstanding: dict[str, str] = {}
    terminal = None

    for number, line in enumerate(trace_path.read_text().splitlines(), start=1):
        entry = json.loads(line)
        direction, frame = entry["direction"], entry["frame"]
        method = frame.get("method")
        where = f"{trace_path}:{number}"

        if method is None:
            # A response or an error: either way it answers something that was actually asked.
            key = f"{'client' if direction == 'server' else 'server'}:{frame.get('id')}"
            if key not in outstanding:
                failures.append(f"{where}: answers a request that was never made")
            else:
                del outstanding[key]
            continue

        if frame.get("id") is not None:
            outstanding[f"{direction}:{frame['id']}"] = method

        if direction == "client":
            seen_client.add(method)
            if method not in client_declared | refused_declared:
                failures.append(f"{where}: `{method}` is not a declared client method")
        else:
            seen_server.add(method)
            if method not in server_declared:
                failures.append(f"{where}: `{method}` is not a declared server method")
            if method == "turn/completed":
                terminal = frame["params"]["turn"]["status"]

    for key, method in sorted(outstanding.items()):
        failures.append(f"{trace_path}: `{method}` ({key}) was never answered")

    if terminal is None:
        failures.append(f"{trace_path}: the trace never reaches a terminal turn")
    elif terminal not in manifest["terminal_statuses"]:
        failures.append(f"{trace_path}: terminal status `{terminal}` is not declared")

    for method in sorted(server_declared - seen_server):
        failures.append(
            f"{trace_path}: declared server method `{method}` never appears in the trace"
        )
    # The same rule for the receiving side. Without it a declared method can go untraced and
    # untested -- which is exactly how a crash on the interrupt path reached a green gate.
    for method in sorted((client_declared | refused_declared) - seen_client):
        failures.append(
            f"{trace_path}: declared client method `{method}` never appears in the trace"
        )


def main() -> int:
    if not PROFILES.is_dir():
        print(f"no app-server profiles under {PROFILES}", file=sys.stderr)
        return 1

    failures: list[str] = []
    versions = 0
    for profile in sorted(path for path in PROFILES.iterdir() if path.is_dir()):
        for version in sorted(path for path in profile.iterdir() if path.is_dir()):
            versions += 1
            check_version(version, failures)

    if versions == 0:
        print(f"no pinned profile versions under {PROFILES}", file=sys.stderr)
        return 1

    for failure in failures:
        print(failure, file=sys.stderr)
    if failures:
        print(f"{len(failures)} app-server profile failures", file=sys.stderr)
        return 1
    print(f"app-server profiles: {versions} pinned version(s) verified")
    return 0


if __name__ == "__main__":
    sys.exit(main())
