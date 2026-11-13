"""Compile dead_weight_chapter_1_and_2.md into a fully-compatible RustMix STORY.TXT."""

import pathlib
import re

src = pathlib.Path(r"c:\Users\asus\Downloads\dead_weight_chapter_1_and_2.md").read_text(
    encoding="utf-8"
)

# 1. Clean header/vars
body_start = src.find("== start ==")
header = """TITLE: Dead Weight: Ch 1 & 2
AUTHOR: Inkle Interactive Novel
START: start
"""

text = src[body_start:]

# 2. Process choice conditions
# Translate Ink conditionals on choices to RustMix syntax:
# `* { toby_status == "dead" } [Text]` -> `* [Text] ?toby_dead -> target`
# `* { toby_status == "alive" } [Text]` -> `* [Text] ?toby_alive -> target`
# `* { not jamie_knows_bite } [Text]` -> `* [Text] ?!jamie_knows_bite -> target`
# `* { saved_chart } [Text]` -> `* [Text] ?saved_chart -> target`

# Map assignments to flag operations
def convert_assignments(block: str) -> str:
    res = []
    for line in block.splitlines():
        l = line.strip()
        if l.startswith("~"):
            # Flags to set
            if "saved_chart = true" in l:
                res.append("+saved_chart")
            elif "julian_knows_bite = true" in l:
                res.append("+julian_knows_bite")
            elif "jamie_knows_bite = true" in l:
                res.append("+jamie_knows_bite")
            elif "kieran_knows_bite = true" in l:
                res.append("+kieran_knows_bite")
            elif "wearing_kieran_jacket = true" in l:
                res.append("+wearing_kieran_jacket")
            elif "has_lemon_tin = true" in l:
                res.append("+has_lemon_tin")
            elif 'toby_status = "alive"' in l:
                res.append("+toby_alive")
                res.append("-toby_dead")
            elif 'toby_status = "dead"' in l:
                res.append("+toby_dead")
                res.append("-toby_alive")
            elif 'triage_choice = "saved_kieran"' in l:
                res.append("+saved_kieran")
            elif 'ch2_plan = "overpass"' in l:
                res.append("+plan_overpass")
            elif 'ch2_plan = "armory"' in l:
                res.append("+plan_armory")
            continue
        res.append(line)
    return "\n".join(res)

text = convert_assignments(text)

# 3. Clean inline conditionals: { condition: textA - else: textB } or { condition: textA }
# Since RustMix evaluates node-level flags, for inline variants we provide the combined or dominant narrative
def simplify_inline_conditionals(s: str) -> str:
    # Pattern: { condition: ... - else: ... }
    def repl_two(m):
        content = m.group(1)
        if "- else:" in content:
            parts = content.split("- else:", 1)
            # Use affirmative part cleanly
            first_body = parts[0].split(":", 1)[1] if ":" in parts[0] else parts[0]
            return first_body.strip()
        elif ":" in content:
            return content.split(":", 1)[1].strip()
        return content.strip()

    # Iterative replace
    prev = None
    while prev != s:
        prev = s
        s = re.sub(r'\{([^{}]+)\}', repl_two, s)
    return s

text = simplify_inline_conditionals(text)

# 4. Standardize choice lines
# Convert choice lines with embedded conditions
lines = []
for line in text.splitlines():
    l = line.strip()
    if re.match(r'^\*\s*\{', l):
        # e.g. * { toby_status == "dead" } [Text] -> target
        m = re.match(r'^\*\s*\{([^}]+)\}\s*\[([^\]]+)\](.*)$', l)
        if m:
            cond = m.group(1).strip()
            label = m.group(2).strip()
            rest = m.group(3).strip()
            flag_tag = ""
            if 'toby_status == "dead"' in cond:
                flag_tag = "?toby_dead"
            elif 'toby_status == "alive"' in cond:
                flag_tag = "?toby_alive"
            elif "not jamie_knows_bite" in cond:
                flag_tag = "?!jamie_knows_bite"
            elif "saved_chart" in cond:
                flag_tag = "?saved_chart"
            lines.append(f"* [{label}] {flag_tag} {rest}".strip())
            continue
    lines.append(line)

text = "\n".join(lines)

# 5. Fix choice format: ensure every choice has `-> target`
# In Ink, choices often have inline bodies followed by `-> target`.
# Let's parse knot by knot.
knots_raw = re.split(r'==\s*([A-Za-z0-9_]+)\s*==', text)
output_passages = []

for i in range(1, len(knots_raw), 2):
    kname = knots_raw[i].strip()
    kbody = knots_raw[i+1]
    
    # Process lines in kbody
    body_lines = []
    choices = []
    sub_node_counter = 0

    curr_choice_label = None
    curr_choice_flag = ""
    curr_choice_target = None
    curr_choice_body = []

    for bline in kbody.splitlines():
        trimmed = bline.strip()
        if trimmed.startswith("* [") or trimmed.startswith("*["):
            # Flush previous choice if any had inline body
            if curr_choice_label:
                sub_node_counter += 1
                sub_id = f"{kname}_c{sub_node_counter}"
                if not curr_choice_target:
                    curr_choice_target = sub_id
                    output_passages.append((
                        sub_id,
                        "\n".join(curr_choice_body).strip() or curr_choice_label,
                        [f"* [Continue] -> {kname}_next"]
                    ))
                choices.append(f"* [{curr_choice_label}] {curr_choice_flag} -> {curr_choice_target}".strip())
                curr_choice_body = []
                curr_choice_target = None

            # Parse new choice
            m = re.match(r'^\*\s*\[([^\]]+)\](.*)$', trimmed)
            if m:
                curr_choice_label = m.group(1).strip()
                after = m.group(2).strip()
                # Check flags
                curr_choice_flag = ""
                for fl in ["?toby_dead", "?toby_alive", "?!jamie_knows_bite", "?saved_chart"]:
                    if fl in after:
                        curr_choice_flag = fl
                        after = after.replace(fl, "").strip()
                if "->" in after:
                    curr_choice_target = after.split("->", 1)[1].strip()
                else:
                    curr_choice_target = None
            continue

        if curr_choice_label:
            if "->" in trimmed and not trimmed.startswith("*"):
                # Divert at end of choice block
                curr_choice_target = trimmed.split("->", 1)[1].strip()
            else:
                curr_choice_body.append(trimmed)
        else:
            body_lines.append(bline)

    if curr_choice_label:
        sub_node_counter += 1
        sub_id = f"{kname}_c{sub_node_counter}"
        if not curr_choice_target:
            curr_choice_target = sub_id
            output_passages.append((
                sub_id,
                "\n".join(curr_choice_body).strip() or curr_choice_label,
                [f"* [Continue] -> {kname}_next"]
            ))
        choices.append(f"* [{curr_choice_label}] {curr_choice_flag} -> {curr_choice_target}".strip())

    cleaned_body = "\n".join(body_lines).strip()
    output_passages.append((kname, cleaned_body, choices))

# Write out STORY.TXT
out_lines = [
    "TITLE: Dead Weight: Ch 1 & 2",
    "AUTHOR: Inkle Interactive Novel",
    "START: start",
]

for nid, nbody, nchoices in output_passages:
    out_lines.append("")
    out_lines.append(f"== {nid} ==")
    if nbody:
        out_lines.append(nbody)
    for c in nchoices:
        out_lines.append(c)

full_story = "\n".join(out_lines) + "\n"

# Clean up empty choices or dangling arrows
full_story = re.sub(r'->\s*$', '-> start', full_story, flags=re.M)

target_path = pathlib.Path(
    r"C:\Users\asus\OneDrive\Desktop\Project\Headstone Project\rustmix-wave\examples\sd-card\RUSTMIX\APPS\STORY\STORIES\DEADWGT.TXT"
)
target_path.write_text(full_story, encoding="utf-8")
print(f"Wrote {target_path} ({len(full_story)} bytes, {len(output_passages)} passages)")
