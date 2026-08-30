#!/usr/bin/env python3
"""Refuse a tracked file that carries an absolute home directory.

An audit found twenty tracked files across five repositories publishing one operator's home
directory, and it ran once, by hand, after publication (`story:history-carries-a-home-directory`).
A committed home path is two defects wearing one coat: it names the operator, and it is a test or
an example that resolves on exactly one machine.

Two shapes are deliberate.

* The enumeration is `git ls-files`, never a filesystem walk. A walk judges untracked scratch
  nobody is committing, and it reaches into `target/` and `.git/` where nothing is committed either.
* Every file is searched as **bytes**. The twentieth file that audit found was a committed `.pyc`
  whose `co_filename` embedded the path, and which no text grep would have found. A check that
  decoded UTF-8 and skipped what it could not read would have passed exactly the file it exists for.

`--self-test` runs the check against planted fixtures, including one that is not valid UTF-8. It is
a gate step of its own, because the failure this check must never have -- passing everything -- is
invisible in a green run.
"""

from __future__ import annotations

import os
import pathlib
import re
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent

# The shape of a home directory, not one operator's name: a contributor's account is refused the
# same way the author's is. Written with the alternation inside a group so that this line does not
# match itself.
HOME_PATH = re.compile(rb"/(?:home|Users)/([A-Za-z0-9][A-Za-z0-9._-]*)/")

# Documentation placeholders. `/home/you/` in a rendered example teaches the reader what the output
# looks like on their own machine; it publishes no account and resolves nowhere. Excluding these by
# name keeps the exclusion in the *shape* rather than in a list of files, so a doc author does not
# have to amend this script to add an example.
PLACEHOLDER_ACCOUNTS = frozenset({b"you", b"user", b"username"})

# Paths that carry a real home directory as history rather than as a live path.
#
# The planning store is append-only and committed: the journal *is* the record of what was decided
# and when, and `history-carries-a-home-directory` is the story that records the leak itself. Both
# quote the operator's home directory because that is the fact they exist to preserve. Editing
# either to satisfy this check would forge the record, and the decision not to rewrite that history
# is `story:history-carries-a-home-directory` and stands. Nothing here resolves a path at run time.
EXEMPT = frozenset(
    {
        ".engineering/planning/journal.jsonl",
        ".engineering/planning/story/history-carries-a-home-directory.md",
    }
)


def home_paths_in(name: str, data: bytes) -> list[str]:
    """Name every absolute home directory in `data`, by file and line."""
    findings: list[str] = []
    for match in HOME_PATH.finditer(data):
        if match.group(1) in PLACEHOLDER_ACCOUNTS:
            continue
        # Counted over bytes, so a file that never decodes still reports a line a reader can
        # find with `sed -n`.
        line = data.count(b"\n", 0, match.start()) + 1
        shown = match.group(0).decode("utf-8", "replace")
        findings.append(f"{name}:{line}: absolute home directory `{shown}`")
    return findings


def read_bytes(root: pathlib.Path, name: str) -> bytes:
    """The committed bytes of one tracked entry, as they sit in the worktree."""
    full = root / name
    if full.is_symlink():
        # A symlink whose target is a home directory is the leak, and following it would read the
        # target's bytes instead of the one thing worth searching.
        return os.fsencode(os.readlink(full))
    return full.read_bytes()


def inspect(root: pathlib.Path, name: str) -> list[str]:
    """Judge one tracked entry. Unreadable is refused, never skipped."""
    if name in EXEMPT:
        return []
    full = root / name
    if full.is_dir() and not full.is_symlink():
        # A submodule pointer. Its bytes live in another repository, and this check is that
        # repository's to run.
        return []
    try:
        data = read_bytes(root, name)
    except OSError as error:
        return [f"{name}: tracked but unreadable: {error.strerror or error}"]
    return home_paths_in(name, data)


def tracked_files(root: pathlib.Path) -> list[str]:
    listing = subprocess.run(
        ["git", "-C", str(root), "ls-files", "-z"],
        capture_output=True,
        check=False,
    )
    if listing.returncode != 0:
        raise SystemExit(
            f"git ls-files failed in {root}: {listing.stderr.decode('utf-8', 'replace').strip()}"
        )
    return [os.fsdecode(entry) for entry in listing.stdout.split(b"\0") if entry]


def check(root: pathlib.Path) -> list[str]:
    findings: list[str] = []
    for name in tracked_files(root):
        findings.extend(inspect(root, name))
    return findings


# The fixtures are assembled rather than written out, so that this file does not carry a literal
# home directory and fail its own check.
PLANTED_ACCOUNT = "alice"
PLANTED = f"/home/{PLANTED_ACCOUNT}/work/id_rsa"
PLANTED_MACOS = f"/Users/{PLANTED_ACCOUNT}/work/id_rsa"
PLACEHOLDER = "/home/{}/.codex/auth.json"


def self_test() -> int:
    failures: list[str] = []

    def case(name: str, held: bool, detail: str = "") -> None:
        if not held:
            failures.append(f"{name}{': ' + detail if detail else ''}")

    found = home_paths_in("fixture.toml", f'key = "{PLANTED}"\n'.encode())
    case(
        "a planted home directory is named with its file and its line",
        len(found) == 1 and found[0].startswith("fixture.toml:1:"),
        repr(found),
    )
    case(
        "the finding quotes the path it found",
        len(found) == 1 and PLANTED_ACCOUNT in found[0],
        repr(found),
    )

    found = home_paths_in("fixture.toml", f'key = "{PLANTED_MACOS}"\n'.encode())
    case(
        "a macOS home directory is a home directory",
        len(found) == 1 and found[0].startswith("fixture.toml:1:"),
        repr(found),
    )

    found = home_paths_in("fixture.toml", f"one\ntwo\n{PLANTED}\n".encode())
    case(
        "the line named is the line the path is on",
        len(found) == 1 and found[0].startswith("fixture.toml:3:"),
        repr(found),
    )

    found = home_paths_in("fixture.toml", f"{PLANTED}\n{PLANTED_MACOS}\n".encode())
    case(
        "every planted line is named, not only the first",
        len(found) == 2,
        repr(found),
    )

    # The `.pyc` case, which is the one this check exists for.
    undecodable = b"\xed\xa0\x80\xff" + PLANTED.encode() + b"\x00\xfe"
    try:
        undecodable.decode("utf-8")
        decodes = True
    except UnicodeDecodeError:
        decodes = False
    case("the undecodable fixture is genuinely undecodable", not decodes)
    found = home_paths_in("fixture.pyc", undecodable)
    case(
        "bytes that are not UTF-8 are searched, not silently passed",
        len(found) == 1,
        repr(found),
    )

    case(
        "a file carrying no home directory passes",
        home_paths_in("fixture.toml", b"workspace = \".\"\n") == [],
    )
    case(
        "a documented placeholder is not an operator's home directory",
        home_paths_in(
            "guide.md",
            (PLACEHOLDER.format("you") + "\n" + PLACEHOLDER.format("user") + "\n").encode(),
        )
        == [],
    )
    case(
        "a directory that merely starts with the word is not a home directory",
        home_paths_in("fixture.toml", b"/homeserver/alice/keys\n") == [],
    )

    with tempfile.TemporaryDirectory() as raw:
        root = pathlib.Path(raw)

        exempt = sorted(EXEMPT)[0]
        (root / exempt).parent.mkdir(parents=True, exist_ok=True)
        (root / exempt).write_text(f"{PLANTED}\n", encoding="utf-8")
        case(
            "an exempt path is not judged",
            inspect(root, exempt) == [],
            repr(inspect(root, exempt)),
        )

        found = inspect(root, "vanished.txt")
        case(
            "a tracked file that cannot be read is refused by name",
            len(found) == 1 and found[0].startswith("vanished.txt:"),
            repr(found),
        )

        os.symlink(PLANTED, root / "link")
        found = inspect(root, "link")
        case(
            "a symlink pointing at a home directory is refused",
            len(found) == 1,
            repr(found),
        )

    with tempfile.TemporaryDirectory() as raw:
        root = pathlib.Path(raw)
        subprocess.run(["git", "-C", str(root), "init", "-q"], check=True)
        (root / "tracked.txt").write_text(f"{PLANTED}\n", encoding="utf-8")
        (root / "untracked.txt").write_text(f"{PLANTED}\n", encoding="utf-8")
        subprocess.run(["git", "-C", str(root), "add", "tracked.txt"], check=True)
        found = check(root)
        case(
            "a planted path in a tracked file is refused end to end",
            len(found) == 1 and found[0].startswith("tracked.txt:1:"),
            repr(found),
        )
        case(
            "an untracked file carrying the same path is not judged",
            all("untracked.txt" not in finding for finding in found),
            repr(found),
        )

    for failure in failures:
        print(f"self-test: {failure}", file=sys.stderr)
    if failures:
        print(f"{len(failures)} self-test failure(s)", file=sys.stderr)
        return 1
    print("home paths: self-test green")
    return 0


def main(argv: list[str]) -> int:
    if argv[1:] == ["--self-test"]:
        return self_test()
    if argv[1:]:
        print(f"usage: {argv[0]} [--self-test]", file=sys.stderr)
        return 2

    names = tracked_files(ROOT)
    findings: list[str] = []
    for name in names:
        findings.extend(inspect(ROOT, name))
    for finding in findings:
        print(finding, file=sys.stderr)
    if findings:
        print(
            f"{len(findings)} absolute home director(ies) in tracked files",
            file=sys.stderr,
        )
        return 1
    print(f"home paths: {len(names)} tracked file(s) searched as bytes, none absolute")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
