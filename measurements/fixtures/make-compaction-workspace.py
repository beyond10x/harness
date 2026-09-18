#!/usr/bin/env python3
"""Materialise the compaction measurement's workspace, byte-for-byte the same every time.

# Why a generator and not eight checked-in files

The fixture decides the answer. A workspace whose files are small never reaches the trigger; one
with a single enormous file triggers on turn one and measures nothing about accumulation. So the
fixture has to be *fixed* -- the same bytes for the free emulator rehearsal and for the paid run,
or the two are not comparable. It also has to be *bulk*, and sixty kilobytes of filler in a source
tree is sixty kilobytes nobody will ever read.

A deterministic generator is both: the bytes are pinned by this file, and `--check` proves a tree
on disk is the one this file describes. Nothing here is random and nothing reads the clock.

# What it builds, and why in this shape

Eight files of roughly equal size, each a self-contained "chapter" carrying facts a later question
can only be answered from. Equal size so the conversation grows in even steps and the turn the
trigger fires on is a measurement rather than an artefact of one large file. Self-contained facts
so the fourth figure -- did the run continue *well* -- has something to be wrong about: every
chapter states one `KEY-nn` token, and a run that compacts and then cannot say what `KEY-03` was
lost something the summary should have kept.

Chapter 1 is deliberately the one the task asks about last. It is inside
[`FIRST_KEPT_ITEM`]-protected territory only if the model read it first, which is the point: the
summary has to carry it.
"""

from __future__ import annotations

import argparse
import hashlib
import pathlib
import sys

CHAPTERS = 8
# ~7.4 KiB each. Eight of them is ~59 KiB, about 15k tokens at the loop's own four-bytes-per-token
# estimate -- so a 16000-token declared window is crossed somewhere around the sixth read, and not
# on turn one. Change this number and the paid run measures a different thing; change it in one
# place and the rehearsal and the paid run stop being comparable.
PARAGRAPHS_PER_CHAPTER = 9

TOPICS = [
    "how a turn is assembled",
    "what an approval envelope carries",
    "why a tool result is elided rather than deleted",
    "what the record says when a stream breaks",
    "how a budget is refused rather than ignored",
    "where a rate card's provenance lives",
    "what a workspace tool may not reach",
    "why the summary turn costs a turn",
]


def paragraph(chapter: int, index: int) -> str:
    topic = TOPICS[(chapter - 1) % len(TOPICS)]
    return (
        f"Paragraph {index:02d} of chapter {chapter:02d} concerns {topic}. "
        "The rule stated here is the rule the surrounding component enforces, written out at "
        "length so that the passage has weight as well as content, because a fixture whose files "
        "are short never reaches the point it was built to reach. A reader who has this passage "
        "in front of them can answer the question at the end of the run; a reader who has only a "
        "summary of it can answer only if the summary kept the part that mattered. That is the "
        "distinction this measurement exists to make, and it is why the passage is repetitive "
        "without being empty: every sentence restates the obligation from a slightly different "
        "angle so that a compression which drops any one of them still leaves the sense intact, "
        "and a compression which drops all of them does not.\n"
    )


def chapter_text(chapter: int) -> str:
    lines = [
        f"# Chapter {chapter:02d}\n",
        "\n",
        f"KEY-{chapter:02d} is {key_token(chapter)}.\n",
        "\n",
        "This chapter is one of eight of roughly equal size. It is read by one `file_read` call\n"
        "and it is the only place in this workspace where its own key appears.\n",
        "\n",
    ]
    for index in range(1, PARAGRAPHS_PER_CHAPTER + 1):
        lines.append(paragraph(chapter, index))
        lines.append("\n")
    lines.append(f"End of chapter {chapter:02d}. Remember KEY-{chapter:02d}.\n")
    return "".join(lines)


def key_token(chapter: int) -> str:
    """A token nothing can guess and nothing else in the tree contains."""
    digest = hashlib.sha256(f"b10x-compaction-fixture/chapter-{chapter:02d}".encode()).hexdigest()
    return f"{digest[:4]}-{digest[4:8]}-{digest[8:12]}"


def files() -> dict[str, str]:
    built = {
        f"chapters/chapter-{chapter:02d}.md": chapter_text(chapter)
        for chapter in range(1, CHAPTERS + 1)
    }
    built["README.md"] = (
        "# Compaction fixture\n"
        "\n"
        f"{CHAPTERS} chapters under `chapters/`, each stating one `KEY-nn` value and nothing else\n"
        "stating it. Read them in order and then answer what each key was.\n"
    )
    return built


def write(root: pathlib.Path) -> None:
    for name, text in files().items():
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")


def check(root: pathlib.Path) -> int:
    bad = 0
    for name, text in files().items():
        path = root / name
        if not path.exists():
            print(f"missing: {name}", file=sys.stderr)
            bad += 1
        elif path.read_text(encoding="utf-8") != text:
            print(f"differs: {name}", file=sys.stderr)
            bad += 1
    return bad


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=pathlib.Path, help="where to materialise the workspace")
    parser.add_argument(
        "--check",
        action="store_true",
        help="compare an existing tree against this file instead of writing it",
    )
    parser.add_argument(
        "--keys",
        action="store_true",
        help="print the expected KEY-nn answers, for scoring the fourth figure",
    )
    arguments = parser.parse_args()
    if arguments.keys:
        for chapter in range(1, CHAPTERS + 1):
            print(f"KEY-{chapter:02d} {key_token(chapter)}")
        return 0
    if arguments.check:
        bad = check(arguments.root)
        print(f"compaction fixture: {'stale' if bad else 'current'} ({len(files())} files)")
        return 1 if bad else 0
    arguments.root.mkdir(parents=True, exist_ok=True)
    write(arguments.root)
    total = sum(len(text.encode()) for text in files().values())
    print(f"wrote {len(files())} files, {total} bytes, to {arguments.root}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
