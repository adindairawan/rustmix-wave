#!/usr/bin/env python3
"""Convert .twee (Twine) or .ink (Inkle Ink) files into RustMix STORY.TXT format."""

import argparse
import pathlib
import re
import sys


def sanitize_node_id(name: str) -> str:
    cleaned = re.sub(r"[^A-Za-z0-9_-]+", "_", name.strip()).strip("_")
    return cleaned.lower() or "node"


def convert_twee(text: str, title: str, author: str) -> str:
    passages: list[tuple[str, list[str]]] = []
    current_name: str | None = None
    current_lines: list[str] = []

    for raw in text.splitlines():
        if raw.startswith("::"):
            if current_name is not None:
                passages.append((current_name, current_lines))
            header = raw[2:].strip()
            header = re.sub(r"\[[^\]]*\]", "", header)
            header = re.sub(r"\{[^}]*\}", "", header)
            current_name = header.strip()
            current_lines = []
        elif current_name is not None:
            current_lines.append(raw)

    if current_name is not None:
        passages.append((current_name, current_lines))

    start_name = "start"
    out = [f"TITLE: {title}", f"AUTHOR: {author}"]
    id_map = {name: sanitize_node_id(name) for name, _ in passages}

    for name, lines in passages:
        lower = name.lower()
        if lower in ("storytitle", "storydata", "storyinit", "stylesheet", "script"):
            if lower == "storytitle":
                joined = " ".join(l.strip() for l in lines if l.strip())
                if joined:
                    out[0] = f"TITLE: {joined}"
            continue

        node_id = id_map[name]
        if lower == "start":
            start_name = node_id
        out.append("")
        out.append(f"== {node_id} ==")

        choices: list[str] = []
        for line in lines:
            stripped = line.strip()
            # Match Twine links: [[Label->Target]] or [[Label|Target]] or [[Target]]
            links = re.findall(r"\[\[([^\]]+)\]\]", stripped)
            if links and re.sub(r"\[\[[^\]]+\]\]", "", stripped).strip() in ("", "*", "-", ">"):
                for link in links:
                    if "->" in link:
                        lbl, tgt = link.split("->", 1)
                    elif "|" in link:
                        lbl, tgt = link.split("|", 1)
                    else:
                        lbl, tgt = link, link
                    tgt_id = id_map.get(tgt.strip(), sanitize_node_id(tgt))
                    choices.append(f"* [{lbl.strip()}] -> {tgt_id}")
                continue
            # Strip simple SugarCube/Harlowe tags while keeping prose readable
            cleaned = re.sub(r"<<[^>]+>>", "", line)
            cleaned = re.sub(r"\[\[([^\|\]]+)(?:\||->)[^\]]+\]\]", r"\1", cleaned)
            cleaned = cleaned.replace("[[", "").replace("]]", "").rstrip()
            out.append(cleaned)

        out.extend(choices)

    out.insert(2, f"START: {start_name}")
    return "\n".join(out) + "\n"


def convert_ink(text: str, title: str, author: str) -> str:
    out = [f"TITLE: {title}", f"AUTHOR: {author}"]
    current_node = "start"
    started_knot = False
    body_lines: list[str] = []
    choices: list[str] = []

    def flush() -> None:
        nonlocal body_lines, choices
        if not started_knot and not body_lines and not choices:
            return
        out.append("")
        out.append(f"== {current_node} ==")
        out.extend(body_lines)
        out.extend(choices)
        body_lines = []
        choices = []

    for raw in text.splitlines():
        line = raw.split("//", 1)[0].strip()
        if not line:
            if body_lines and body_lines[-1] != "":
                body_lines.append("")
            continue

        knot_match = re.match(r"^={2,}\s*([A-Za-z0-9_]+)\s*=*$", line)
        if knot_match:
            flush()
            current_node = sanitize_node_id(knot_match.group(1))
            started_knot = True
            continue

        choice_match = re.match(r"^[*+]+\s*\[([^\]]+)\](.*)$", line)
        if choice_match:
            label = choice_match.group(1).strip()
            rest = choice_match.group(2)
            divert = re.search(r"->\s*([A-Za-z0-9_]+)", rest)
            target = sanitize_node_id(divert.group(1)) if divert else current_node
            choices.append(f"* [{label}] -> {target}")
            continue

        if line.startswith("->"):
            target = sanitize_node_id(line[2:])
            if target != "end" and target != "done":
                choices.append(f"* [Continue] -> {target}")
            continue

        if line.startswith("VAR ") or line.startswith("~"):
            continue

        body_lines.append(line)

    flush()
    return "\n".join(out) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description="Convert .twee or .ink to RustMix STORY.TXT")
    parser.add_argument("input", type=pathlib.Path, help="Input .twee or .ink file")
    parser.add_argument("-o", "--output", type=pathlib.Path, default=pathlib.Path("STORY.TXT"))
    parser.add_argument("--title", default=None)
    parser.add_argument("--author", default="Community IF")
    args = parser.parse_args()

    text = args.input.read_text(encoding="utf-8")
    title = args.title or args.input.stem.replace("_", " ").replace("-", " ").title()
    ext = args.input.suffix.lower()

    if ext in (".twee", ".tw"):
        result = convert_twee(text, title, args.author)
    elif ext == ".ink":
        result = convert_ink(text, title, args.author)
    else:
        print(f"Unsupported extension '{ext}' (expected .twee, .tw, or .ink)", file=sys.stderr)
        return 1

    args.output.write_text(result, encoding="utf-8")
    print(f"Wrote {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
