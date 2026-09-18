#!/usr/bin/env python3
"""Read a `b10x-harness run --json` record and report the figures the measurements ask for.

This is a reader over an event stream, not an evaluation harness: it opens a JSONL file, adds up
fields that are already there, and prints them. `epic:measured-not-emulated` excludes "building an
evaluation harness"; nothing here builds one.

Every figure comes from a typed event that production already emits -- `crates/harness-loop/src/
event.rs`, tagged `{"kind": ...}` in kebab-case. Nothing was instrumented for this.

    ./measurements/score.py compaction measurements/runs/m1-dry-compaction-16000.jsonl \\
        --context-window 16000 --expect-keys

    ./measurements/score.py surface runs/m2-flat-1.jsonl --arm flat

# What it refuses to report

* **A trigger point with no window.** `--context-window` has to be stated, because the declared
  window is not in the event stream: `compacted` carries bytes and counts, and the only place the
  window and the occupancy that crossed it appear is the prose of the `conversation-compacted`
  warning beside it. That warning is parsed here as a **cross-check** on the figure the caller
  declared, never as its source, and a disagreement is printed rather than resolved.
* **A cost on a run that reported none.** Absent stays absent. A run whose card did not price the
  model the provider reported emits no `cost` event at all, and printing 0 there would be a claim
  that it was free.
* **A compaction that did not happen.** No `compacted` event means the run never compacted, which
  is a result -- and, in a rehearsal, usually means the fixture is wrong. It is reported as
  `compactions: 0` and the exit status is non-zero under `--require-compaction`.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import subprocess
import sys

DISCOVERY_TOOLS = {"tool_search", "tool_describe"}
COMPACTED_WARNING = re.compile(
    r"the conversation reached (\d+) tokens of the (\d+) declared"
)
KEY_LINE = re.compile(r"KEY-(\d{2})\s+(\S+)")


def events(path: pathlib.Path) -> list[dict]:
    out = []
    for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        line = line.strip()
        if not line:
            continue
        try:
            out.append(json.loads(line))
        except json.JSONDecodeError as error:
            raise SystemExit(f"{path}:{number}: not JSON: {error}") from error
    return out


def turns(stream: list[dict]) -> dict:
    """Turns started, and retries, kept apart.

    A turn whose stream broke after it started speaking is attempted again and announced as
    discardable (`turn-retried`). It does not start a new turn, so counting `turn-started` is the
    turn count and `turn-retried` is a separate figure -- summing the two would count one turn
    several times and make a flaky route look like an expensive one.
    """
    started = [event for event in stream if event["kind"] == "turn-started"]
    retried = [event for event in stream if event["kind"] == "turn-retried"]
    highest = max((event["turn"] for event in started), default=0)
    return {
        "turns": len(started),
        "highest_turn_number": highest,
        "retries": len(retried),
        "retried_turns": sorted({event["turn"] for event in retried}),
    }


def usage_total(stream: list[dict], start: int = 0, end: int | None = None) -> dict:
    window = stream[start : end if end is not None else len(stream)]
    total = {"input_tokens": 0, "cached_input_tokens": 0, "cache_creation_input_tokens": 0,
             "output_tokens": 0}
    seen = False
    for event in window:
        if event["kind"] != "usage":
            continue
        seen = True
        for field in total:
            total[field] += event.get(field) or 0
    return total if seen else {}


def cost_total(stream: list[dict], start: int = 0, end: int | None = None):
    """Micro-USD, or `None` where nothing priced the run. Never 0 for an unpriced run."""
    window = stream[start : end if end is not None else len(stream)]
    figures = [event["micro_usd"] for event in window if event["kind"] == "cost"]
    return sum(figures) if figures else None


def terminal(stream: list[dict]) -> dict:
    for event in reversed(stream):
        if event["kind"] in ("finished", "refused", "stopped"):
            return event
    return {"kind": "<none: the record has no terminal event>"}


def answer_text(stream: list[dict]) -> str:
    """Everything the run said after its last compaction, reassembled from the deltas."""
    last = max(
        (index for index, event in enumerate(stream) if event["kind"] == "compacted"),
        default=-1,
    )
    return "".join(
        event["text"] for event in stream[last + 1 :] if event["kind"] == "text-delta"
    )


def score_compaction(stream: list[dict], window: int | None, expect_keys: bool) -> dict:
    compactions = []
    for index, event in enumerate(stream):
        if event["kind"] != "compacted":
            continue
        before, after = event["bytes_before"], event["bytes_after"]

        # The occupancy that crossed the threshold, and the window it crossed, as the loop itself
        # computed them. Only the warning beside the event carries them; it is read as a check on
        # what the caller declared, and a mismatch is printed.
        occupied = declared = None
        for earlier in reversed(stream[:index]):
            if earlier["kind"] == "warning" and earlier.get("code") == "conversation-compacted":
                found = COMPACTED_WARNING.search(earlier["message"])
                if found:
                    occupied, declared = int(found.group(1)), int(found.group(2))
                break

        # The provider's own last reported input before the compaction: the independent figure, and
        # the one that says whether the trigger fired on the provider's accounting or on the loop's
        # four-bytes-per-token estimate.
        reported = None
        for earlier in reversed(stream[:index]):
            if earlier["kind"] == "usage":
                reported = earlier["input_tokens"]
                break

        # The summary turn, where there was one: the last turn opened before this event, and
        # whatever it reported. `summary_turn` is true even when that turn failed, because it was
        # still paid for.
        summary = None
        if event["summary_turn"]:
            opened = max(
                (
                    earlier
                    for earlier in range(index)
                    if stream[earlier]["kind"] == "turn-started"
                ),
                default=None,
            )
            if opened is not None:
                summary = {
                    "turn": stream[opened]["turn"],
                    "usage": usage_total(stream, opened, index) or None,
                    "cost_micro_usd": cost_total(stream, opened, index),
                }

        effective = window if window is not None else declared
        compactions.append(
            {
                "trigger": {
                    "declared_window": window,
                    "window_in_record": declared,
                    "window_disagrees": window is not None
                    and declared is not None
                    and window != declared,
                    "occupied_tokens": occupied,
                    "percent_of_window": round(100 * occupied / effective, 1)
                    if occupied and effective
                    else None,
                    "last_reported_input_tokens": reported,
                    "fired_on": None
                    if occupied is None or reported is None
                    else ("provider-reported count" if reported >= occupied else "loop estimate"),
                },
                "freed": {
                    "bytes_before": before,
                    "bytes_after": after,
                    "remaining_fraction": round(after / before, 4) if before else None,
                    "elided_results": event["elided_results"],
                    "elided_bytes": event["elided_bytes"],
                    "left_above_trigger": None
                    if not (occupied and effective and before)
                    else (occupied * after / before) > effective * 0.8,
                },
                "summary": {
                    "summary_turn": event["summary_turn"],
                    "summarised_items": event["summarised_items"],
                    "turn": summary,
                },
                "turns_after": sum(
                    1 for later in stream[index:] if later["kind"] == "turn-started"
                ),
            }
        )

    report = {
        "compactions": len(compactions),
        "detail": compactions,
        "run": {
            **turns(stream),
            "usage": usage_total(stream) or None,
            "cost_micro_usd": cost_total(stream),
            "terminal": terminal(stream),
        },
    }
    if expect_keys:
        report["continued_well"] = keys_recovered(stream)
    return report


def keys_recovered(stream: list[dict]) -> dict:
    """The fourth figure, made checkable: what the run could still say after compacting.

    The fixture puts one unguessable token in each chapter and nowhere else, so an answer that
    reports a chapter's token proves that chapter survived compaction in a usable form, and an
    answer that reports something else proves it did not. This is the one figure a single run
    cannot settle on a real model -- it is a judgement about output, not arithmetic -- so it is
    reported as observed, never as measured.
    """
    generator = pathlib.Path(__file__).parent / "fixtures" / "make-compaction-workspace.py"
    expected = {}
    for line in subprocess.run(
        [sys.executable, str(generator), "--keys", "-"],
        capture_output=True, text=True, check=True,
    ).stdout.splitlines():
        chapter, value = line.split()
        expected[chapter.removeprefix("KEY-")] = value
    said = dict(KEY_LINE.findall(answer_text(stream)))
    return {
        "expected": len(expected),
        "correct": sorted(k for k, v in expected.items() if said.get(k) == v),
        "wrong": sorted(k for k, v in expected.items() if k in said and said[k] != v),
        "missing": sorted(k for k in expected if k not in said),
        "note": "observed once; a judgement about output, not arithmetic",
    }


def score_surface(stream: list[dict], arm: str | None) -> dict:
    requested = [event for event in stream if event["kind"] == "tool-requested"]
    completed = [event for event in stream if event["kind"] == "tool-completed"]
    discovery = [event for event in requested if event["name"] in DISCOVERY_TOOLS]
    failed = [event for event in completed if event.get("failed")]
    started = next((event for event in stream if event["kind"] == "started"), {})
    end = terminal(stream)
    return {
        "arm": arm,
        "published_tools": started.get("published_tools"),
        "tool_calls": len(requested),
        "by_name": {
            name: sum(1 for event in requested if event["name"] == name)
            for name in sorted({event["name"] for event in requested})
        },
        "discovery_calls": len(discovery),
        "discovery_share": round(len(discovery) / len(requested), 4) if requested else None,
        "failed_calls": len(failed),
        "failed_share": round(len(failed) / len(completed), 4) if completed else None,
        **turns(stream),
        "usage": usage_total(stream) or None,
        "cost_micro_usd": cost_total(stream),
        "terminal": end,
        "completed": end.get("kind") == "finished"
        and end.get("stop", {}).get("kind") == "completed",
        "note": "an arm that finished cheaper because it gave up is cheaper and wrong; void a "
                "repeat whose arms did not reach the same end state",
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("measurement", choices=["compaction", "surface"])
    parser.add_argument("record", type=pathlib.Path)
    parser.add_argument("--context-window", type=int, default=None)
    parser.add_argument("--arm", default=None)
    parser.add_argument("--expect-keys", action="store_true",
                        help="check the answer against the compaction fixture's own keys")
    parser.add_argument("--require-compaction", action="store_true",
                        help="exit non-zero when the record holds no `compacted` event")
    parser.add_argument("--brief", action="store_true",
                        help="one or two readable lines instead of the whole report")
    arguments = parser.parse_args()

    stream = events(arguments.record)
    if arguments.measurement == "compaction":
        report = score_compaction(stream, arguments.context_window, arguments.expect_keys)
    else:
        report = score_surface(stream, arguments.arm)
    report["record"] = str(arguments.record)
    report["evidence_class"] = "provider_emulated" if emulated(stream) else "unlabelled"
    if arguments.brief and arguments.measurement == "compaction":
        brief_compaction(report)
    elif arguments.brief:
        brief_surface(report)
    else:
        print(json.dumps(report, indent=2))

    if arguments.require_compaction and not report.get("compactions"):
        print(
            "no `compacted` event: this run never compacted. In a rehearsal that is a fixture "
            "defect, not a result -- see measurements/README.md.",
            file=sys.stderr,
        )
        return 1
    return 0


def brief_compaction(report: dict) -> None:
    run = report["run"]
    stop = run["terminal"].get("stop", {}).get("kind", "-")
    print(
        f"compactions {report['compactions']}  turns {run['turns']:>2}  "
        f"{run['terminal']['kind']}/{stop}  [{report['evidence_class']}]"
    )
    for detail in report["detail"]:
        trigger, freed, summary = detail["trigger"], detail["freed"], detail["summary"]
        print(
            f"    fired at {trigger['percent_of_window']}% of the declared window "
            f"({trigger['occupied_tokens']} tokens, on the {trigger['fired_on']}); "
            f"left {freed['remaining_fraction']} of the bytes; "
            f"elided {freed['elided_results']} result(s); "
            f"summary_turn={summary['summary_turn']} summarised={summary['summarised_items']}"
        )
    keys = report.get("continued_well")
    if keys:
        print(
            f"    keys: {len(keys['correct'])} kept, {len(keys['wrong'])} lost, "
            f"{len(keys['missing'])} unanswered  ({keys['note']})"
        )


def brief_surface(report: dict) -> None:
    print(
        f"arm {report['arm']}  calls {report['tool_calls']}  "
        f"discovery {report['discovery_calls']} ({report['discovery_share']})  "
        f"failed {report['failed_calls']} ({report['failed_share']})  "
        f"turns {report['turns']}  cost {report['cost_micro_usd']}  "
        f"completed={report['completed']}  [{report['evidence_class']}]"
    )


def emulated(stream: list[dict]) -> str | bool:
    """`provider_emulated` where the record itself says the endpoint was the local emulator.

    Invariant 18: emulator evidence is `provider_emulated` and is never promoted to `vendor_live`.
    This labels what it can see and says `unlabelled` otherwise; it never writes `vendor_live`.
    """
    started = next((event for event in stream if event["kind"] == "started"), {})
    return started.get("model") == "b10x-emulated"


if __name__ == "__main__":
    raise SystemExit(main())
