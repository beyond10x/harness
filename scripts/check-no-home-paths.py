#!/usr/bin/env python3
"""Refuse a tracked file that carries an absolute home directory.

An audit found twenty tracked files across five repositories publishing one operator's home
directory, and it ran once, by hand, after publication (`story:history-carries-a-home-directory`).
A committed home path is two defects wearing one coat: it names the operator, and it is a test or
an example that resolves on exactly one machine.

Four shapes are deliberate.

* What is judged is the **index**, read with `git cat-file --batch` over `git ls-files -s`, because
  a commit records the index and not the working copy. Content staged with a leak and then tidied
  in the worktree would otherwise be committed by a check that passed it. The worktree copy of each
  tracked path is judged as well, because `git commit -a` stages it and the gate runs first.
* Every file is searched as **bytes**. The twentieth file that audit found was a committed `.pyc`
  whose `co_filename` embedded the path, and which no text grep would have found. A check that
  decoded UTF-8 and skipped what it could not read would have passed exactly the file it exists
  for. A second pass searches the same bytes with NUL bytes removed, which is what makes a path
  inside UTF-16 or UTF-32 text visible to an ASCII pattern.
* A home directory does **not** need a trailing separator. `HOME=/home/<name>` and
  `home = "/home/<name>"` publish the account exactly as `/home/<name>/work` does, and that is
  the commoner shape of the leak. (Written with `<name>`, here and below, because this file is
  itself tracked and this check searches it.)
* The pattern is the *shape* of a home directory, not one operator's name, and its account class
  admits every byte above ASCII, so a contributor whose account is `müller` or `张伟` is protected
  the same way the author is. What that class also admits is Unicode punctuation, so a candidate is
  trimmed to the name at its head: `/home/<ellipsis>/.ssh/id_rsa` in a doc comment names nobody.

What it does **not** cover, stated so that nobody reads more into a green run than is there:

* `~`, `$HOME` and any path assembled at run time. Only a literal absolute path is visible here.
* A Windows home directory (`C:\\Users\\alice`). The two shapes named above are POSIX.
* History. This judges the index and the worktree, never earlier commits;
  `story:history-carries-a-home-directory` records the decision not to rewrite what is already
  published.
* The single account name `you`, which is treated as a documentation placeholder everywhere. See
  `PLACEHOLDER_ACCOUNTS`.

`--self-test` runs the check against planted fixtures -- text, undecodable bytes, UTF-16, symlinks,
a staged-then-tidied worktree, and its own command-line exit status. It is a gate step of its own,
because the failure this check must never have -- passing everything -- is invisible in a green run.
"""

from __future__ import annotations

import contextlib
import os
import pathlib
import re
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent

# The shape of a home directory. The account class carries `_` and every byte above ASCII, so a
# non-ASCII account name is refused like any other; there is no trailing separator, because
# `HOME=/home/<name>` at the end of a line is the same leak as `/home/<name>/.ssh/id_rsa`. Greedy
# matching ends the candidate at the first byte that cannot be in an account, and `account_of`
# trims what is left to the name at its head.
# Written with the alternation inside a group so that this line does not match itself.
ACCOUNT = rb"[A-Za-z0-9._\-\x80-\xff]+"
HOME_PATH = re.compile(rb"/(?:home|Users)/(" + ACCOUNT + rb")")

# One documentation placeholder, deliberately narrow. `/home/you/` in a rendered example teaches a
# reader what the output looks like on their own machine, and `you` is not a name anyone's account
# has. `user` and `username` were here too and are not: they are ordinary account names on real
# machines -- a container image ships a `user` account -- and excluding them would have waved
# through `let key = "/home/<that account>/.ssh/id_ed25519"` in a source file. The exclusion is by
# account name and applies in every file type, including Rust: two of the four placeholders here are
# fixtures in `crates/harness-cli/src/render.rs` and `crates/harness-loop/src/event.rs`, which
# render example output and are not documentation by path.
PLACEHOLDER_ACCOUNTS = frozenset({"you"})

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


def account_of(candidate: bytes) -> str:
    """The account name at the head of a candidate: letters, digits and `._-`, and nothing else.

    The byte pattern admits every byte above ASCII so that a non-ASCII account name is caught, and
    that admits Unicode punctuation with it: a doc comment writing `/home/<ellipsis>/.ssh/id_rsa`
    means *some* home directory, and names nobody. Trimming at the first character that cannot be
    in a name is what tells the two apart, and an account that is punctuation all the way down is
    not an account.
    """
    kept: list[str] = []
    for character in candidate.decode("utf-8", "replace"):
        if character.isalnum() or character in "._-":
            kept.append(character)
        else:
            break
    account = "".join(kept)
    return account if any(character.isalnum() or character == "_" for character in account) else ""


def home_paths_in(name: str, data: bytes) -> list[str]:
    """Name every absolute home directory in `data`, by file and line."""
    views = [data]
    if b"\x00" in data:
        # UTF-16 and UTF-32 text is bytes an ASCII pattern cannot see: the characters are
        # interleaved with NULs. Removing them keeps every `\n` in place, so a line number counted
        # in this view is the line number a reader will find in the file.
        views.append(data.replace(b"\x00", b""))

    # Keyed by line and matched text: the same path found in both views is one fact, not two.
    findings: dict[tuple[int, str], str] = {}
    for view in views:
        for match in HOME_PATH.finditer(view):
            account = account_of(match.group(1))
            if not account:
                continue
            # A trailing dot belongs to the prose, not to the account: `/home/you.` at the end of a
            # sentence names the same placeholder as `/home/you`.
            if account.rstrip(".") in PLACEHOLDER_ACCOUNTS:
                continue
            line = view.count(b"\n", 0, match.start()) + 1
            root = view[match.start() : match.start(1)].decode("ascii")
            shown = root + account
            findings[(line, shown)] = f"{name}:{line}: absolute home directory `{shown}`"
    return [findings[key] for key in sorted(findings)]


def read_bytes(root: pathlib.Path, name: str) -> bytes:
    """The bytes of one path in the worktree."""
    full = root / name
    if full.is_symlink():
        # A symlink whose target is a home directory is the leak, and following it would read the
        # target's bytes -- or fail, when the target is another machine's -- instead of searching
        # the one thing worth searching.
        return os.fsencode(os.readlink(full))
    return full.read_bytes()


def inspect(root: pathlib.Path, name: str) -> list[str]:
    """Judge one path in the worktree. Unreadable is refused, never skipped."""
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


def index_entries(root: pathlib.Path) -> list[tuple[str, str]]:
    """Every path in the index with the blob a commit would record for it."""
    listing = subprocess.run(
        ["git", "-C", str(root), "ls-files", "-s", "-z"],
        capture_output=True,
        check=False,
    )
    if listing.returncode != 0:
        raise SystemExit(
            f"git ls-files failed in {root}: {listing.stderr.decode('utf-8', 'replace').strip()}"
        )
    entries: list[tuple[str, str]] = []
    for raw in listing.stdout.split(b"\0"):
        if not raw:
            continue
        meta, _, name = raw.partition(b"\t")
        fields = meta.split(b" ")
        if len(fields) < 2:
            raise SystemExit(f"git ls-files -s produced an entry this cannot read: {raw!r}")
        mode, blob = fields[0], fields[1]
        if mode == b"160000":
            # A submodule pointer: the bytes it names are in another repository's object store.
            continue
        entries.append((os.fsdecode(name), blob.decode("ascii")))
    return entries


def tracked_files(root: pathlib.Path) -> list[str]:
    """The enumeration the acceptance names: `git ls-files`, never a filesystem walk."""
    return [name for name, _ in index_entries(root)]


def staged_bytes(root: pathlib.Path, blobs: list[str]) -> dict[str, bytes]:
    """The staged content of each blob id, in one `git cat-file --batch`."""
    wanted = sorted(set(blobs))
    if not wanted:
        return {}
    batch = subprocess.run(
        ["git", "-C", str(root), "cat-file", "--batch"],
        input=("\n".join(wanted) + "\n").encode("ascii"),
        capture_output=True,
        check=False,
    )
    if batch.returncode != 0:
        raise SystemExit(
            f"git cat-file failed in {root}: {batch.stderr.decode('utf-8', 'replace').strip()}"
        )
    out = batch.stdout
    contents: dict[str, bytes] = {}
    at = 0
    for blob in wanted:
        end = out.find(b"\n", at)
        if end < 0:
            break
        header = out[at:end].split(b" ")
        at = end + 1
        if len(header) < 3:
            # `<oid> missing`: reported by the caller rather than passed over.
            continue
        size = int(header[2])
        contents[blob] = out[at : at + size]
        at += size + 1
    return contents


def check(root: pathlib.Path) -> list[str]:
    """Every absolute home directory a commit here would record, or the worktree already carries."""
    findings: dict[str, None] = {}

    entries = index_entries(root)
    contents = staged_bytes(root, [blob for _, blob in entries])
    for name, blob in entries:
        if name in EXEMPT:
            continue
        data = contents.get(blob)
        if data is None:
            findings[f"{name}: staged as {blob}, which git cannot read"] = None
            continue
        for finding in home_paths_in(name, data):
            findings[finding] = None

    for name, _ in entries:
        if not os.path.lexists(root / name):
            # Deleted or staged for deletion. The index copy above is what a commit records, and
            # a missing worktree file is not a leak.
            continue
        for finding in inspect(root, name):
            findings[finding] = None

    return list(findings)


# The fixtures are assembled rather than written out, so that this file does not carry a literal
# home directory and fail its own check.
SEP = "/"
HOME = SEP + "home" + SEP
USERS = SEP + "Users" + SEP
ACCOUNT_NAME = "alice"
PLANTED = f"{HOME}{ACCOUNT_NAME}{SEP}work{SEP}id_rsa"
PLANTED_MACOS = f"{USERS}{ACCOUNT_NAME}{SEP}work{SEP}id_rsa"


def self_test() -> int:
    results: list[tuple[str, bool, str]] = []

    def case(name: str, held: bool, detail: str = "") -> None:
        results.append((name, held, detail))

    with contextlib.ExitStack() as stack:

        def scratch(prefix: str) -> pathlib.Path:
            return pathlib.Path(stack.enter_context(tempfile.TemporaryDirectory(prefix=prefix)))

        def repo(prefix: str = "self-test-repo-") -> pathlib.Path:
            root = scratch(prefix)
            for args in (
                ("init", "-q"),
                ("config", "user.email", "a@example.invalid"),
                ("config", "user.name", "a"),
            ):
                subprocess.run(["git", "-C", str(root), *args], check=True, capture_output=True)
            return root

        # -- the finding names the file and the line ------------------------------------------
        found = home_paths_in("fixture.toml", f'key = "{PLANTED}"\n'.encode())
        case(
            "a planted home directory is named with its file and its line",
            len(found) == 1 and found[0].startswith("fixture.toml:1:"),
            repr(found),
        )
        case(
            "the finding quotes the path it found",
            len(found) == 1 and ACCOUNT_NAME in found[0],
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
        case("every planted line is named, not only the first", len(found) == 2, repr(found))

        # -- a home directory needs no trailing separator ---------------------------------------
        for label, prefix in (("linux", HOME), ("macOS", USERS)):
            for form in (
                f'home = "{prefix}{ACCOUNT_NAME}"',
                f"HOME={prefix}{ACCOUNT_NAME}",
                f"{prefix}{ACCOUNT_NAME}",
            ):
                found = home_paths_in("f.txt", (form + "\n").encode())
                case(
                    f"[{label}] a home directory with no trailing slash is refused: {form!r}",
                    len(found) == 1,
                    repr(found),
                )

        root = repo()
        (root / "cfg.toml").write_text(f'home = "{HOME}{ACCOUNT_NAME}"\n', encoding="utf-8")
        subprocess.run(["git", "-C", str(root), "add", "cfg.toml"], check=True, capture_output=True)
        found = check(root)
        case(
            "end to end: a tracked file whose home path has no trailing slash is refused",
            len(found) == 1 and found[0].startswith("cfg.toml:1:"),
            repr(found),
        )

        # -- bytes, not decoded text -------------------------------------------------------------
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
        found = home_paths_in("f.txt", PLANTED.encode("utf-16-le"))
        case(
            "a home path in a UTF-16 encoded tracked file is not silently passed",
            len(found) == 1,
            repr(found),
        )
        found = home_paths_in("f.txt", PLANTED.encode("utf-16-be"))
        case(
            "a home path in a big-endian UTF-16 tracked file is not silently passed",
            len(found) == 1,
            repr(found),
        )

        # -- the shape protects a contributor the same way -------------------------------------
        for account in ("müller", "José", "张伟"):
            found = home_paths_in("f.txt", f"{HOME}{account}{SEP}work\n".encode())
            case(
                f"a contributor whose account is non-ASCII is protected: {account!r}",
                len(found) == 1,
                repr(found),
            )
        found = home_paths_in("f.txt", f"{HOME}_timo{SEP}work\n".encode())
        case(
            "an account name starting with an underscore is a home directory",
            len(found) == 1,
            repr(found),
        )

        # -- clean content, and the one placeholder ----------------------------------------------
        case(
            "a file carrying no home directory passes",
            home_paths_in("fixture.toml", b'workspace = "."\n') == [],
        )
        case(
            "the documentation placeholder is not an operator's home directory",
            home_paths_in("guide.md", (HOME + "you" + SEP + ".codex" + SEP + "auth.json\n").encode())
            == [],
        )
        case(
            "a real account named `user` is not waved through as a documentation placeholder",
            len(
                home_paths_in(
                    "crates/x/src/lib.rs",
                    f'let key = "{HOME}user{SEP}.ssh{SEP}id_ed25519";\n'.encode(),
                )
            )
            == 1,
        )
        for account in ("you-know-who", "username2", "users", "user2", "younger", "youtube"):
            found = home_paths_in("f.txt", f"{HOME}{account}{SEP}x\n".encode())
            case(
                f"the placeholder exclusion does not extend to {account!r}",
                len(found) == 1,
                repr(found),
            )
        case(
            "a directory that merely starts with the word is not a home directory",
            home_paths_in("fixture.toml", b"/homeserver/alice/keys\n") == [],
        )

        # Prose naming *some* home directory names nobody. `crates/harness-cli/src/workflow.rs`
        # carries the ellipsis form in a doc comment, and refusing it would be a false alarm in a
        # file this unit may not edit.
        for prose in ("\u2026", "...", "\u201c", "\u2014"):
            case(
                f"a home directory written as prose is not an account: {prose!r}",
                home_paths_in("src/lib.rs", f"{HOME}{prose}{SEP}.ssh{SEP}id_ed25519\n".encode())
                == [],
                repr(home_paths_in("src/lib.rs", f"{HOME}{prose}{SEP}x\n".encode())),
            )
        case(
            "an account followed by a curly quote is still that account",
            len(home_paths_in("guide.md", f"{HOME}{ACCOUNT_NAME}\u201d\n".encode())) == 1,
            repr(home_paths_in("guide.md", f"{HOME}{ACCOUNT_NAME}\u201d\n".encode())),
        )
        case(
            "the placeholder is still the placeholder when prose follows it",
            home_paths_in("guide.md", (HOME + "you\u2019s machine\n").encode()) == [],
            repr(home_paths_in("guide.md", (HOME + "you\u2019s machine\n").encode())),
        )

        # -- symlinks ----------------------------------------------------------------------------
        links = scratch("self-test-links-")
        os.symlink(f"{HOME}{ACCOUNT_NAME}", links / "home_itself")
        found = inspect(links, "home_itself")
        case(
            "a symlink pointing at a home directory itself is refused",
            len(found) == 1 and "absolute home directory" in found[0],
            repr(found),
        )
        os.symlink(f"{USERS}{ACCOUNT_NAME}", links / "mac_home")
        found = inspect(links, "mac_home")
        case(
            "a symlink pointing at a macOS home directory itself is refused",
            len(found) == 1 and "absolute home directory" in found[0],
            repr(found),
        )
        # Deleting the readlink branch leaves a bare `len(found) == 1` green, because reading a
        # dangling symlink raises OSError and yields exactly one finding. The kind of finding is
        # what the branch decides, so the kind is what this asserts.
        os.symlink(PLANTED, links / "deep")
        found = inspect(links, "deep")
        case(
            "a symlink is refused for its target, not merely as unreadable",
            len(found) == 1 and "absolute home directory" in found[0],
            repr(found),
        )

        # -- exemptions, and what does not inherit them ------------------------------------------
        exempt_root = scratch("self-test-exempt-")
        exempt = sorted(EXEMPT)[0]
        (exempt_root / exempt).parent.mkdir(parents=True, exist_ok=True)
        (exempt_root / exempt).write_text(f"{PLANTED}\n", encoding="utf-8")
        case(
            "an exempt path is not judged",
            inspect(exempt_root, exempt) == [],
            repr(inspect(exempt_root, exempt)),
        )
        for suffix in (".bak", ".orig", "2"):
            name = exempt + suffix
            (exempt_root / name).write_text(f"{PLANTED}\n", encoding="utf-8")
            found = inspect(exempt_root, name)
            case(
                f"a sibling of an exempt path does not inherit the exemption: {suffix!r}",
                len(found) == 1,
                repr(found),
            )

        found = inspect(exempt_root, "vanished.txt")
        case(
            "a worktree file that cannot be read is refused by name",
            len(found) == 1 and found[0].startswith("vanished.txt:"),
            repr(found),
        )

        # -- the index is what a commit records --------------------------------------------------
        root = repo()
        (root / "f.txt").write_text(f"{PLANTED}\n", encoding="utf-8")
        (root / "untracked.txt").write_text(f"{PLANTED}\n", encoding="utf-8")
        subprocess.run(["git", "-C", str(root), "add", "f.txt"], check=True, capture_output=True)
        found = check(root)
        case(
            "a planted path in a tracked file is refused end to end",
            len(found) == 1 and found[0].startswith("f.txt:1:"),
            repr(found),
        )
        case(
            "an untracked file carrying the same path is not judged",
            all("untracked.txt" not in finding for finding in found),
            repr(found),
        )

        root = repo()
        (root / "f.txt").write_text(f"{PLANTED}\n", encoding="utf-8")
        subprocess.run(["git", "-C", str(root), "add", "f.txt"], check=True, capture_output=True)
        (root / "f.txt").write_text("clean\n", encoding="utf-8")
        found = check(root)
        case(
            "a home path staged for commit is refused even when the worktree copy is clean",
            len(found) == 1,
            repr(found),
        )

        root = repo()
        (root / "f.txt").write_text("clean\n", encoding="utf-8")
        subprocess.run(["git", "-C", str(root), "add", "f.txt"], check=True, capture_output=True)
        (root / "f.txt").write_text(f"{PLANTED}\n", encoding="utf-8")
        found = check(root)
        case(
            "a home path in the worktree of a tracked file is refused before it is staged",
            len(found) == 1,
            repr(found),
        )

        root = repo()
        (root / "f.txt").write_text(f"{PLANTED}\n", encoding="utf-8")
        subprocess.run(["git", "-C", str(root), "add", "f.txt"], check=True, capture_output=True)
        (root / "f.txt").unlink()
        found = check(root)
        case(
            "a tracked file deleted from the worktree is judged by its staged bytes, not refused "
            "as unreadable",
            len(found) == 1 and "unreadable" not in found[0],
            repr(found),
        )

        # -- the command line, whose exit status is the acceptance --------------------------------
        root = repo()
        (root / "scripts").mkdir()
        (root / "scripts" / "check.py").write_bytes(
            (ROOT / "scripts" / "check-no-home-paths.py").read_bytes()
        )
        (root / "leak.toml").write_text(f'key = "{PLANTED}"\n', encoding="utf-8")
        subprocess.run(
            ["git", "-C", str(root), "add", "leak.toml", "scripts/check.py"],
            check=True,
            capture_output=True,
        )
        proc = subprocess.run(
            [sys.executable, str(root / "scripts" / "check.py")],
            capture_output=True,
            text=True,
            cwd=str(root),
        )
        case(
            "the command-line entry point exits non-zero when a tracked file carries a home "
            "directory",
            proc.returncode != 0,
            f"exit={proc.returncode} stdout={proc.stdout!r} stderr={proc.stderr!r}",
        )
        case(
            "the command-line entry point names the file and the line on stderr",
            "leak.toml:1:" in proc.stderr,
            f"stderr={proc.stderr!r}",
        )

        root = repo()
        (root / "scripts").mkdir()
        (root / "scripts" / "check.py").write_bytes(
            (ROOT / "scripts" / "check-no-home-paths.py").read_bytes()
        )
        (root / "clean.toml").write_text('workspace = "."\n', encoding="utf-8")
        subprocess.run(
            ["git", "-C", str(root), "add", "clean.toml", "scripts/check.py"],
            check=True,
            capture_output=True,
        )
        proc = subprocess.run(
            [sys.executable, str(root / "scripts" / "check.py")],
            capture_output=True,
            text=True,
            cwd=str(root),
        )
        case(
            "the command-line entry point exits zero on a tree that carries none, and does not "
            "fail its own check",
            proc.returncode == 0,
            f"exit={proc.returncode} stdout={proc.stdout!r} stderr={proc.stderr!r}",
        )

    failures = [result for result in results if not result[1]]
    for name, held, detail in results:
        if not held:
            print(f"self-test: {name}" + (f": {detail}" if detail else ""), file=sys.stderr)
    if failures:
        print(f"{len(failures)} of {len(results)} self-test case(s) failed", file=sys.stderr)
        return 1
    print(f"home paths: self-test green, {len(results)} case(s)")
    return 0


def main(argv: list[str]) -> int:
    if argv[1:] == ["--self-test"]:
        return self_test()
    if argv[1:]:
        print(f"usage: {argv[0]} [--self-test]", file=sys.stderr)
        return 2

    findings = check(ROOT)
    for finding in findings:
        print(finding, file=sys.stderr)
    if findings:
        print(
            f"{len(findings)} absolute home director(ies) in tracked files",
            file=sys.stderr,
        )
        return 1
    print(
        f"home paths: {len(tracked_files(ROOT))} tracked file(s) searched as bytes, "
        "staged and in the worktree, none absolute"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
