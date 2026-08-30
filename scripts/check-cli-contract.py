#!/usr/bin/env python3
"""Verify each pinned command-line contract against its argv document.

The command line is a third interface with consumers of its own: metaharness's `b10x` adapter
launches this binary and reads its record. `--substrate-embedded` once changed from taking a value
to being bare, and a consumer pinned to `0.1.0` went on passing a value clap then refused -- before
any harness code ran, so nothing in the run's own record could say why.

This makes the pinned document load-bearing: change a flag's shape without cutting a new version
and this fails here rather than at a driver.

The Rust side proves the other half -- that clap's own definition still produces exactly these
bytes. Neither check is sufficient alone.

`--self-test` runs this checker against planted version directories -- a row missing `short`, a
`short` that is not a one-letter flag, a bare flag naming a placeholder, a digest that does not
match, a list naming a flag of another command, and the same documents under a version cut before
those rules existed. It is a gate step of its own, because the failure this check must never have
-- passing everything -- is invisible in a green run, and every rule here is a branch that fires
against no directory in the tree.
"""

from __future__ import annotations

import contextlib
import hashlib
import json
import pathlib
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
CONTRACTS = ROOT / "contracts" / "cli"

REQUIRED_KEYS = {
    "product",
    "version",
    "interface",
    "generated_from",
    "subcommands",
    "files",
}

REQUIRED_ARGUMENT_KEYS = {
    "long",
    "short",
    "takes_value",
    "value_name",
    "default",
    "required",
    "conflicts_with",
    "requires",
}

REQUIRED_POSITIONAL_KEYS = {"name", "required", "multiple"}

# The cut that learned `short` -- the one-letter spelling of the same flag -- the `positionals`
# field, and the rule that a flag eating no word names no placeholder. All three are asked of this
# version and of everything after it, and of nothing before: a released version is immutable
# (`AGENTS.md` invariant 13), so a version cut earlier cannot grow a key or drop a placeholder, and
# it has to go on verifying. Nothing is ever added here; the boundary is a date, and a directory cut
# after it carries all three.
#
# Moving this **forward** would exempt the version in force and read as a green gate, which is why
# `--self-test` plants against the literal `2026-08-30.2` and against a later date rather than
# against whatever this says.
FIRST_VERSION_DESCRIBING_SHORT_FLAGS = "2026-08-30.2"

# The two that say what a flag may not appear beside and what it may not appear without. Both are
# lists of long flags, both are always present, and both are empty rather than absent when there is
# nothing to say -- a key that appeared only sometimes reads as a document that forgot.
LIST_OF_FLAGS_KEYS = ("conflicts_with", "requires")


def cut_order(version: str) -> tuple[str, int]:
    """The day a version was cut, and which cut of that day it was.

    Split rather than compared as a string, the same way `contract.rs` orders them: invariant 13's
    scheme has no ceiling on `.N`, and a plain comparison puts `2026-08-30.10` -- the eleventh cut
    of that day -- between `.1` and `.2`, which would put a version on the wrong side of a rule.
    """
    day, _, nth = version.rpartition(".")
    if day and nth.isdigit():
        return (day, int(nth))
    return (version, 0)


def describes_short_flags(version: str) -> bool:
    """Whether this version was cut with `short` and the no-placeholder rule, or before them."""
    return cut_order(version) >= cut_order(FIRST_VERSION_DESCRIBING_SHORT_FLAGS)


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

    if manifest["product"] != directory.parent.name:
        failures.append(
            f"{manifest_path}: product `{manifest['product']}` does not match its directory"
        )
    if manifest["version"] != directory.name:
        failures.append(f"{manifest_path}: version does not match its directory")
    # A product naming itself after a vendor makes an incident unreadable, so a manifest may not
    # claim to be one either.
    if not manifest["product"].startswith("b10x"):
        failures.append(
            f"{manifest_path}: product `{manifest['product']}` must name this implementation"
        )
    if not manifest["files"]:
        failures.append(f"{manifest_path}: records no files")

    check_files(directory, manifest, manifest_path, failures)
    check_argv(directory, manifest, failures)


def check_files(
    directory: pathlib.Path,
    manifest: dict,
    manifest_path: pathlib.Path,
    failures: list[str],
) -> None:
    recorded = {entry["path"] for entry in manifest["files"]}
    present = {
        path.name
        for path in sorted(directory.glob("*.json"))
        if path.name != "manifest.json"
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
            failures.append(f"{target}: {len(body)} bytes, manifest says {entry['bytes']}")
        digest = hashlib.sha256(body).hexdigest()
        if digest != entry["sha256"]:
            failures.append(f"{target}: sha256 {digest}, manifest says {entry['sha256']}")


def check_argv(directory: pathlib.Path, manifest: dict, failures: list[str]) -> None:
    argv_path = directory / "argv.json"
    if not argv_path.is_file():
        failures.append(f"{directory}: no argv.json")
        return
    try:
        argv = json.loads(argv_path.read_text())
    except json.JSONDecodeError as error:
        failures.append(f"{argv_path}: not JSON: {error}")
        return

    if argv.get("product") != manifest["product"]:
        failures.append(f"{argv_path}: names a different product from its manifest")
    declared = manifest["subcommands"]
    if argv.get("subcommands") != declared:
        failures.append(f"{argv_path}: subcommands differ from the manifest's {declared}")
    if declared != sorted(declared):
        failures.append(f"{argv_path}: the subcommand list is not in name order")

    # A key learned after some versions were pinned is asked only of the versions cut with it.
    describes_short = describes_short_flags(manifest["version"])
    required_keys = REQUIRED_ARGUMENT_KEYS
    if not describes_short:
        required_keys = REQUIRED_ARGUMENT_KEYS - {"short"}

    arguments = argv.get("arguments")
    if not isinstance(arguments, dict):
        failures.append(f"{argv_path}: `arguments` is not an object")
        return
    # Every subcommand must have a row set, or the document pins a name and nothing about it.
    for name in declared:
        if name not in arguments:
            failures.append(f"{argv_path}: `{name}` is declared but has no arguments recorded")
    for name, rows in sorted(arguments.items()):
        if not isinstance(rows, list):
            failures.append(f"{argv_path}: `{name}` does not hold a list of arguments")
            continue
        longs = []
        for row in rows:
            if not isinstance(row, dict):
                failures.append(f"{argv_path}: `{name}` holds an argument that is not an object")
                continue
            missing = required_keys - row.keys()
            if missing:
                failures.append(
                    f"{argv_path}: `{name}` `{row.get('long')}` is missing {sorted(missing)}"
                )
                continue
            if not str(row["long"]).startswith("--"):
                failures.append(f"{argv_path}: `{name}` `{row['long']}` is not a long flag")
            check_flag_lists(argv_path, name, row, rows, failures)
            # A flag with a default it never reads is a flag whose default is a lie about what
            # happens when it is left out.
            if row["default"] is not None and not row["takes_value"]:
                failures.append(
                    f"{argv_path}: `{name}` `{row['long']}` takes no value but declares a default"
                )
            if describes_short:
                check_short_and_placeholder(argv_path, name, row, failures)
            longs.append(row["long"])
        if longs != sorted(longs):
            failures.append(f"{argv_path}: `{name}` arguments are not in name order")
        if len(set(longs)) != len(longs):
            failures.append(f"{argv_path}: `{name}` declares a flag twice")

    if describes_short:
        check_positionals(argv_path, argv, arguments, failures)


def check_positionals(
    argv_path: pathlib.Path,
    argv: dict,
    arguments: dict,
    failures: list[str],
) -> None:
    """Every command has a positional list, and every row of one says what a caller must type.

    `b10x-harness profiles show` exits 1 with *the following required arguments were not provided:
    <NAME>*, and six pinned versions said this command line had no positional arguments at all. A
    driver generating an invocation from those was refused by clap before any harness code ran,
    which is the failure this contract exists for.

    Not checked for order, and that is deliberate: a positional is identified by its place, so the
    list is in the order the words are typed and sorting it would describe a command line nobody
    can type.
    """
    positionals = argv.get("positionals")
    if not isinstance(positionals, dict):
        failures.append(f"{argv_path}: `positionals` is not an object")
        return
    if set(positionals) != set(arguments):
        for name in sorted(set(arguments) - set(positionals)):
            failures.append(f"{argv_path}: `{name}` has arguments recorded but no positional list")
        for name in sorted(set(positionals) - set(arguments)):
            failures.append(f"{argv_path}: `{name}` has a positional list and no arguments")
    for name, rows in sorted(positionals.items()):
        if not isinstance(rows, list):
            failures.append(f"{argv_path}: `{name}` does not hold a list of positionals")
            continue
        for row in rows:
            if not isinstance(row, dict):
                failures.append(f"{argv_path}: `{name}` holds a positional that is not an object")
                continue
            missing = REQUIRED_POSITIONAL_KEYS - row.keys()
            if missing:
                failures.append(
                    f"{argv_path}: `{name}` positional {row.get('name')!r} is missing "
                    f"{sorted(missing)}"
                )
                continue
            if not isinstance(row["name"], str) or not row["name"]:
                failures.append(f"{argv_path}: `{name}` holds a positional with no placeholder")
            for key in ("required", "multiple"):
                if not isinstance(row[key], bool):
                    failures.append(
                        f"{argv_path}: `{name}` positional `{row['name']}` `{key}` is not a boolean"
                    )


def check_flag_lists(
    argv_path: pathlib.Path,
    name: str,
    row: dict,
    rows: list,
    failures: list[str],
) -> None:
    """`conflicts_with` and `requires` name real flags of this command, in name order.

    A requirement is as load-bearing as a conflict: `--delegate-turns` without `--delegate` and
    `--oauth-token-pointer` without an oauth source are both refused by clap before any harness code
    runs, which is the failure this contract exists to make visible. A name that is not a flag of
    this command is a document nobody can act on.
    """
    longs = {other.get("long") for other in rows if isinstance(other, dict)}
    for key in LIST_OF_FLAGS_KEYS:
        listed = row[key]
        if not isinstance(listed, list) or not all(
            isinstance(entry, str) for entry in listed
        ):
            failures.append(f"{argv_path}: `{name}` `{row['long']}` `{key}` is not a list of flags")
            continue
        if listed != sorted(listed):
            failures.append(f"{argv_path}: `{name}` `{row['long']}` `{key}` is not in name order")
        if row["long"] in listed:
            failures.append(f"{argv_path}: `{name}` `{row['long']}` `{key}` names itself")
        for entry in sorted(set(listed) - longs):
            failures.append(
                f"{argv_path}: `{name}` `{row['long']}` `{key}` names `{entry}`, "
                "which is not a flag of that command"
            )


def check_short_and_placeholder(
    argv_path: pathlib.Path,
    name: str,
    row: dict,
    failures: list[str],
) -> None:
    """The short spelling is a flag, and a flag that eats no word names no placeholder.

    `value_name` is *the placeholder in the usage line*, and clap prints none beside a bare flag. A
    document that carries one anyway reads as "this flag takes this argument" to anything generating
    an invocation from it -- which emits `--substrate-embedded SUBSTRATE_EMBEDDED`, the exact word
    clap refused from the consumer pinned to `0.1.0`.
    """
    short = row["short"]
    if short is not None and (
        not isinstance(short, str) or len(short) != 2 or not short.startswith("-")
    ):
        failures.append(
            f"{argv_path}: `{name}` `{row['long']}` `short` is `{short}`, not a one-letter flag"
        )
    if not row["takes_value"] and row["value_name"] is not None:
        failures.append(
            f"{argv_path}: `{name}` `{row['long']}` takes no value but names the placeholder "
            f"`{row['value_name']}`, which this binary prints for no bare flag"
        )


def planted_row(long: str, **overrides: object) -> dict:
    """One flag row of a planted document, every pinned key at its quietest value."""
    row = {
        "long": long,
        "short": None,
        "takes_value": True,
        "value_name": "VALUE",
        "default": None,
        "required": False,
        "conflicts_with": [],
        "requires": [],
    }
    row.update(overrides)
    return row


def plant(
    root: pathlib.Path,
    version: str,
    arguments: dict,
    *,
    sha256: str | None = None,
    positionals: dict | None = None,
) -> pathlib.Path:
    """A version directory written from scratch, manifest and digests included.

    The product directory is named `b10x-harness` because a manifest names the directory it sits
    in, and a planted tree that could not satisfy that would be testing the planting rather than
    the check.
    """
    directory = root / "b10x-harness" / version
    directory.mkdir(parents=True)
    subcommands = sorted(arguments)
    argv = {
        "product": "b10x-harness",
        "subcommands": subcommands,
        "arguments": arguments,
        # Empty for every command unless the case is about them, the way the real document holds an
        # empty list for the sixteen commands that take no word.
        "positionals": (
            positionals if positionals is not None else {name: [] for name in arguments}
        ),
    }
    body = (json.dumps(argv, indent=2, sort_keys=True) + "\n").encode()
    (directory / "argv.json").write_bytes(body)
    manifest = {
        "product": "b10x-harness",
        "version": version,
        "interface": "argv",
        "generated_from": "clap::CommandFactory::command()",
        "subcommands": subcommands,
        "files": [
            {
                "path": "argv.json",
                "bytes": len(body),
                "sha256": sha256 or hashlib.sha256(body).hexdigest(),
            }
        ],
    }
    (directory / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    return directory


def self_test() -> int:
    """Every rule this checker holds, run against a document that breaks it and one that does not.

    Both halves for each rule. A check that reported everything would pass a suite that only ever
    planted defects, and that is the same class of failure as one that reported nothing -- which is
    what the two rules added for `short` and the usage-line placeholder were until this existed:
    turning either off left all seven pinned versions verifying, and the gate green.
    """
    results: list[tuple[str, bool, str]] = []

    def case(name: str, held: bool, detail: str = "") -> None:
        results.append((name, held, detail))

    with contextlib.ExitStack() as stack:

        def failures_for(version: str, arguments: dict, **planting: object) -> list[str]:
            root = pathlib.Path(
                stack.enter_context(tempfile.TemporaryDirectory(prefix="cli-contract-self-test-"))
            )
            found: list[str] = []
            check_version(plant(root, version, arguments, **planting), found)
            return found

        # The literal version this cut is, never the constant: an off-by-one that moved
        # `FIRST_VERSION_DESCRIBING_SHORT_FLAGS` to `2026-08-30.3` would exempt the version in
        # force from all three rules, and a self-test that followed the constant wherever it went
        # stayed 27/27 green while it did. Measured, and the reason these are literals.
        current = "2026-08-30.2"
        later = "2027-01-01"
        earlier = "2026-08-30.1"

        # -- the version gate, which decides whether the newer rules are asked at all ------------
        for version in (current, later, "2026-08-30.3", "2026-08-30.10", "2026-09-01.4"):
            case(
                f"`{version}` is cut with `short`, `positionals` and the placeholder rule",
                describes_short_flags(version),
            )
        for version in ("2026-08-29", "2026-08-29.10", "2026-08-30", earlier):
            case(
                f"`{version}` was cut before them and is asked for none of them",
                not describes_short_flags(version),
            )
        case(
            "the version in force is on the near side of the boundary",
            describes_short_flags(current),
            f"FIRST_VERSION_DESCRIBING_SHORT_FLAGS={FIRST_VERSION_DESCRIBING_SHORT_FLAGS!r}",
        )
        case(
            "the tenth cut of a day is later than the second, not earlier",
            cut_order("2026-08-30.10") > cut_order("2026-08-30.2"),
            repr((cut_order("2026-08-30.10"), cut_order("2026-08-30.2"))),
        )

        # -- a document that breaks no rule is reported as breaking no rule ----------------------
        clean = {
            "run": [
                planted_row("--bare", takes_value=False, value_name=None),
                planted_row("--profile", short="-p"),
            ]
        }
        for version in (current, later):
            found = failures_for(version, clean)
            case(f"a document that breaks no rule is clean at `{version}`", found == [], repr(found))

        # -- `short` is required of every version cut with it, and of no version cut before -------
        without_short = {
            "run": [
                {key: value for key, value in planted_row("--profile").items() if key != "short"}
            ]
        }
        for version in (current, later):
            found = failures_for(version, without_short)
            case(
                f"a row with no `short` key fails `{version}`",
                len(found) == 1 and "is missing ['short']" in found[0],
                repr(found),
            )
        found = failures_for(earlier, without_short)
        case("the same row passes a version cut before the key existed", found == [], repr(found))

        for spelling in ("--p", "p", "-pp", ""):
            found = failures_for(current, {"run": [planted_row("--profile", short=spelling)]})
            case(
                f"`short` of {spelling!r} is not a one-letter flag",
                len(found) == 1 and "not a one-letter flag" in found[0],
                repr(found),
            )
        found = failures_for(current, {"run": [planted_row("--profile", short="-p")]})
        case("`-p` is", found == [], repr(found))

        # -- a flag that eats no word names no placeholder ---------------------------------------
        bare_with_placeholder = {
            "run": [planted_row("--substrate-embedded", takes_value=False, value_name="SUBSTRATE")]
        }
        for version in (current, later):
            found = failures_for(version, bare_with_placeholder)
            case(
                f"a bare flag naming a placeholder fails `{version}`",
                len(found) == 1 and "prints for no bare flag" in found[0],
                repr(found),
            )
        found = failures_for(earlier, bare_with_placeholder)
        case("the same row passes the six versions cut before the rule", found == [], repr(found))
        found = failures_for(
            current, {"run": [planted_row("--model", takes_value=True, value_name="MODEL")]}
        )
        case("a flag that does eat a word may name one", found == [], repr(found))

        # -- `positionals`, which six versions said this command line had none of -----------------
        rows = {"run": [planted_row("--json")]}
        found = failures_for(current, rows, positionals={})
        case(
            "a command with no positional list is reported",
            any("no positional list" in failure for failure in found),
            repr(found),
        )
        found = failures_for(
            current,
            rows,
            positionals={"run": [], "profiles show": [{"name": "NAME", "required": True,
                                                       "multiple": False}]},
        )
        case(
            "a positional list for a command that has no arguments is reported",
            any("no arguments" in failure for failure in found),
            repr(found),
        )
        found = failures_for(
            current, rows, positionals={"run": [{"name": "NAME", "required": True}]}
        )
        case(
            "a positional missing `multiple` is reported",
            len(found) == 1 and "is missing ['multiple']" in found[0],
            repr(found),
        )
        found = failures_for(
            current,
            rows,
            positionals={"run": [{"name": "NAME", "required": "yes", "multiple": False}]},
        )
        case(
            "a positional whose `required` is not a boolean is reported",
            len(found) == 1 and "`required` is not a boolean" in found[0],
            repr(found),
        )
        found = failures_for(
            current,
            rows,
            positionals={"run": [{"name": "", "required": True, "multiple": False}]},
        )
        case(
            "a positional with no placeholder is reported",
            len(found) == 1 and "no placeholder" in found[0],
            repr(found),
        )
        found = failures_for(
            current,
            rows,
            positionals={"run": [{"name": "NAME", "required": True, "multiple": False}]},
        )
        case("a positional that says all three things is clean", found == [], repr(found))
        found = failures_for(earlier, rows, positionals=None)
        case(
            "a version cut before the field is not asked for it",
            found == [],
            repr(found),
        )

        # -- the rules that predate this change, which no directory in the tree exercises either --
        found = failures_for(current, clean, sha256="0" * 64)
        case(
            "a digest that does not match the file is reported",
            any("sha256" in failure for failure in found),
            repr(found),
        )
        found = failures_for(
            current,
            {"run": [planted_row("--json", takes_value=False, value_name=None, default="false")]},
        )
        case(
            "a flag that takes no value and declares a default is reported",
            len(found) == 1 and "declares a default" in found[0],
            repr(found),
        )
        found = failures_for(current, {"run": [planted_row("--a", conflicts_with=["--absent"])]})
        case(
            "a conflict naming a flag of no such command is reported",
            len(found) == 1 and "not a flag of that command" in found[0],
            repr(found),
        )
        found = failures_for(current, {"run": [planted_row("--b"), planted_row("--a")]})
        case(
            "arguments out of name order are reported",
            len(found) == 1 and "not in name order" in found[0],
            repr(found),
        )
        found = failures_for(current, {"run": [planted_row("-p")]})
        case(
            "a row whose `long` is not a long flag is reported",
            len(found) == 1 and "is not a long flag" in found[0],
            repr(found),
        )

    failures = [result for result in results if not result[1]]
    for name, held, detail in results:
        if not held:
            print(f"self-test: {name}" + (f": {detail}" if detail else ""), file=sys.stderr)
    if failures:
        print(f"{len(failures)} of {len(results)} self-test case(s) failed", file=sys.stderr)
        return 1
    print(f"command line: self-test green, {len(results)} case(s)")
    return 0


def main(argv: list[str]) -> int:
    if argv[1:] == ["--self-test"]:
        return self_test()
    if argv[1:]:
        print(f"usage: {argv[0]} [--self-test]", file=sys.stderr)
        return 2

    if not CONTRACTS.is_dir():
        print(f"no command-line contracts under {CONTRACTS}", file=sys.stderr)
        return 1

    failures: list[str] = []
    versions = 0
    for product in sorted(path for path in CONTRACTS.iterdir() if path.is_dir()):
        for version in sorted(path for path in product.iterdir() if path.is_dir()):
            versions += 1
            check_version(version, failures)

    if versions == 0:
        print(f"no pinned command-line versions under {CONTRACTS}", file=sys.stderr)
        return 1

    for failure in failures:
        print(failure, file=sys.stderr)
    if failures:
        print(f"{len(failures)} command-line contract failures", file=sys.stderr)
        return 1
    print(f"command line: {versions} pinned version(s) verified")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
