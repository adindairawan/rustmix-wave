#!/usr/bin/env python3
"""Convert .twee (Twine) or .ink (Inkle Ink) files into RustMix STORY.TXT format."""

import argparse
import html
from html.parser import HTMLParser
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
    lines = text.splitlines()
    start_node = "start"

    # Extract Title/Author from header if present
    for line in lines[:25]:
        s = line.strip()
        if s.startswith("TITLE:"):
            title = s.split(":", 1)[1].strip()
        elif s.startswith("AUTHOR:"):
            author = s.split(":", 1)[1].strip()
        elif s.startswith("START:"):
            start_node = s.split(":", 1)[1].strip()

    # Parse into knots
    knots: list[tuple[str, list[str]]] = []
    cur_id: str | None = None
    cur_lines: list[str] = []

    for line in lines:
        s = line.strip()
        if s.startswith("==") and s.endswith("==") and len(s) > 4:
            if cur_id is not None:
                knots.append((cur_id, cur_lines))
            cur_id = sanitize_node_id(s.strip("=").strip())
            cur_lines = []
        elif cur_id is not None:
            cur_lines.append(line)
    if cur_id is not None:
        knots.append((cur_id, cur_lines))

    if not knots:
        # Single implicit passage if no == knots == were defined
        knots.append(("start", lines))

    choice_pat = re.compile(r"^\s*[*+]\s*(\{[^}]+\})?\s*\[([^\]]+)\](.*)$")

    out_passages: dict[str, dict] = {}
    extra_passages: dict[str, dict] = {}

    for knot_id, klines in knots:
        p_flags: list[str] = []
        p_body: list[str] = []
        p_choices: list[str] = []

        i = 0
        while i < len(klines):
            raw = klines[i]
            s = raw.strip()

            # Skip comments and variable declarations
            if s.startswith("//") or s.startswith("--") or s.startswith("VAR "):
                i += 1
                continue

            # Node-level flag assignments (~ flag = val)
            if s.startswith("~"):
                assign = s[1:].strip()
                if "saved_chart = true" in assign:
                    p_flags.append("+saved_chart")
                elif "julian_knows_bite = true" in assign:
                    p_flags.append("+julian_knows_bite")
                elif "jamie_knows_bite = true" in assign:
                    p_flags.append("+jamie_knows_bite")
                elif "kieran_knows_bite = true" in assign:
                    p_flags.append("+kieran_knows_bite")
                elif "wearing_kieran_jacket = true" in assign:
                    p_flags.append("+wearing_kieran_jacket")
                elif "has_lemon_tin = true" in assign:
                    p_flags.append("+has_lemon_tin")
                elif 'toby_status = "alive"' in assign or "toby_status = 'alive'" in assign:
                    p_flags.extend(["+toby_alive", "-toby_dead"])
                elif 'toby_status = "dead"' in assign or "toby_status = 'dead'" in assign:
                    p_flags.extend(["+toby_dead", "-toby_alive"])
                i += 1
                continue

            m = choice_pat.match(raw)
            if m:
                cond_raw = m.group(1) or ""
                label = m.group(2).strip()
                rest = m.group(3).strip()

                # Parse choice conditions
                req_flag = ""
                if "saved_chart" in cond_raw:
                    req_flag = "?saved_chart "
                elif "not jamie_knows_bite" in cond_raw or "!jamie_knows_bite" in cond_raw:
                    req_flag = "?!jamie_knows_bite "
                elif 'toby_status == "dead"' in cond_raw or "toby_status == 'dead'" in cond_raw:
                    req_flag = "?toby_dead "
                elif 'toby_status == "alive"' in cond_raw or "toby_status == 'alive'" in cond_raw:
                    req_flag = "?toby_alive "

                # Check for direct divert on choice line
                direct_target = None
                if "->" in rest:
                    direct_target = rest.split("->", 1)[1].strip()

                # Collect sublines (narrative reaction or assignment)
                sublines: list[str] = []
                choice_target = direct_target
                set_flags: list[str] = []

                j = i + 1
                while j < len(klines):
                    sub_raw = klines[j]
                    sub_s = sub_raw.strip()
                    if sub_s.startswith("==") and sub_s.endswith("=="):
                        break
                    if choice_pat.match(sub_raw):
                        break
                    # Ignore epilogue summary bullet points that are not choices
                    if sub_s.startswith("* Intern") or sub_s.startswith("* Patient") or sub_s.startswith("* Romance"):
                        break
                    if sub_s.startswith("//") or sub_s.startswith("--"):
                        j += 1
                        continue
                    if sub_s.startswith("~"):
                        assign = sub_s[1:].strip()
                        if "saved_chart = true" in assign:
                            set_flags.append("+saved_chart")
                        elif "jamie_knows_bite = true" in assign:
                            set_flags.append("+jamie_knows_bite")
                        elif "julian_knows_bite = true" in assign:
                            set_flags.append("+julian_knows_bite")
                        elif "kieran_knows_bite = true" in assign:
                            set_flags.append("+kieran_knows_bite")
                        elif "wearing_kieran_jacket = true" in assign:
                            set_flags.append("+wearing_kieran_jacket")
                        elif "has_lemon_tin = true" in assign:
                            set_flags.append("+has_lemon_tin")
                        elif 'toby_status = "alive"' in assign:
                            set_flags.extend(["+toby_alive", "-toby_dead"])
                        elif 'toby_status = "dead"' in assign:
                            set_flags.extend(["+toby_dead", "-toby_alive"])
                        j += 1
                        continue

                    if sub_s.startswith("->"):
                        choice_target = sub_s[2:].strip()
                        j += 1
                        break
                    elif "->" in sub_s:
                        b_text, tgt = sub_s.split("->", 1)
                        if b_text.strip():
                            sublines.append(b_text.strip())
                        choice_target = tgt.strip()
                        j += 1
                        break
                    else:
                        if sub_s:
                            sublines.append(sub_s)
                    j += 1

                i = j

                # Clean choice target
                if choice_target:
                    choice_target = sanitize_node_id(choice_target)
                    if choice_target in ("end", "done"):
                        choice_target = None

                flags_str = req_flag + " ".join(set_flags)
                flags_str = (flags_str.strip() + " ") if flags_str.strip() else ""

                # Clean sublines from conditional blocks if any
                clean_sublines: list[str] = []
                for sl in sublines:
                    cleaned = re.sub(r"\{[^}]+\}", "", sl).strip()
                    if (
                        cleaned
                        and not cleaned.startswith("~")
                        and not cleaned.startswith("- else:")
                        and not cleaned.startswith("{")
                        and cleaned != "}"
                    ):
                        clean_sublines.append(cleaned)

                if clean_sublines and choice_target:
                    opt_id = f"{knot_id}_opt{len(p_choices) + 1}"
                    extra_passages[opt_id] = {
                        "flags": [],
                        "body": clean_sublines,
                        "choices": [f"* [Continue] -> {choice_target}"],
                    }
                    p_choices.append(f"* [{label}] {flags_str}-> {opt_id}")
                elif choice_target:
                    p_choices.append(f"* [{label}] {flags_str}-> {choice_target}")
                continue

            # Regular body lines - preserve conditional blocks { ... } for engine evaluation!
            clean_s = raw.strip()
            if (
                clean_s.startswith("~")
                or clean_s.startswith("//")
                or clean_s.startswith("--")
                or clean_s.startswith("VAR ")
            ):
                i += 1
                continue

            if clean_s:
                p_body.append(raw.rstrip())
            i += 1

        out_passages[knot_id] = {
            "flags": p_flags,
            "body": p_body,
            "choices": p_choices,
        }

    out = [f"TITLE: {title}", f"AUTHOR: {author}", f"START: {start_node}"]
    combined = {**out_passages, **extra_passages}
    for nid, p in combined.items():
        out.append("")
        out.append(f"== {nid} ==")
        for f in p["flags"]:
            out.append(f)
        for b in p["body"]:
            out.append(b)
        for c in p["choices"]:
            out.append(c)

    return "\n".join(out) + "\n"


class _TwineHtmlParser(HTMLParser):
    def __init__(self) -> None:
        super().__init__()
        self.story_name: str | None = None
        self.passages: list[tuple[str, str]] = []
        self._current_name: str | None = None
        self._buf: list[str] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        attr_map = dict(attrs)
        if tag.lower() == "tw-storydata":
            if attr_map.get("name"):
                self.story_name = attr_map["name"]
        elif tag.lower() == "tw-passagedata":
            self._current_name = attr_map.get("name") or "node"
            self._buf = []

    def handle_data(self, data: str) -> None:
        if self._current_name is not None:
            self._buf.append(data)

    def handle_endtag(self, tag: str) -> None:
        if tag.lower() == "tw-passagedata" and self._current_name is not None:
            self.passages.append((self._current_name, html.unescape("".join(self._buf))))
            self._current_name = None
            self._buf = []


def convert_twine_html(raw_html: str, title: str, author: str) -> str:
    parser = _TwineHtmlParser()
    parser.feed(raw_html)
    if not parser.passages:
        raise ValueError(
            "No Twine <tw-passagedata> passages found in HTML "
            "(this looks like a webpage wrapper rather than a compiled Twine .html game)."
        )
    twee_parts: list[str] = []
    if parser.story_name:
        twee_parts.append(f":: StoryTitle\n{parser.story_name}\n")
    for name, body in parser.passages:
        twee_parts.append(f":: {name}\n{body}\n")
    return convert_twee("\n".join(twee_parts), parser.story_name or title, author)


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Convert .twee, .ink, .md, or Twine .html to RustMix STORY.TXT"
    )
    parser.add_argument("input", type=pathlib.Path, help="Input .twee, .ink, .md, or Twine .html file")
    parser.add_argument("-o", "--output", type=pathlib.Path, default=pathlib.Path("STORY.TXT"))
    parser.add_argument("--title", default=None)
    parser.add_argument("--author", default="Community IF")
    args = parser.parse_args()

    text = args.input.read_text(encoding="utf-8", errors="replace")
    title = args.title or args.input.stem.replace("_", " ").replace("-", " ").title()
    ext = args.input.suffix.lower()

    if ext in (".twee", ".tw"):
        result = convert_twee(text, title, args.author)
    elif ext in (".ink", ".md", ".txt"):
        # Auto-detect if it contains Twee passages (:: name) or HTML
        if "<tw-storydata" in text:
            result = convert_twine_html(text, title, args.author)
        elif re.search(r"^::\s+[A-Za-z0-9_]+", text, re.MULTILINE):
            result = convert_twee(text, title, args.author)
        else:
            result = convert_ink(text, title, args.author)
    elif ext in (".html", ".htm"):
        try:
            result = convert_twine_html(text, title, args.author)
        except ValueError as err:
            print(f"Error: {err}", file=sys.stderr)
            return 1
    else:
        print(
            f"Unsupported extension '{ext}' (expected .twee, .tw, .ink, .md, or .html)",
            file=sys.stderr,
        )
        return 1

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(result, encoding="utf-8")
    print(f"Wrote {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
