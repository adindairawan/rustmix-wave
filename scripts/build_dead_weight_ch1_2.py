"""Convert dead_weight_chapter_1_and_2.md into a fully-compatible RustMix STORY.TXT."""

import pathlib
import re

def convert_story():
    SRC_PATH = pathlib.Path(r"c:\Users\asus\Downloads\dead_weight_chapter_1_and_2.md")
    DEST_SD_PATH = pathlib.Path(
        r"C:\Users\asus\OneDrive\Desktop\Project\Headstone Project\rustmix-wave\examples\sd-card\RUSTMIX\APPS\STORY\STORIES\DEADWGT.TXT"
    )
    DEST_DOWNLOADS_PATH = pathlib.Path(r"c:\Users\asus\Downloads\DEADWGT.TXT")

    raw_text = SRC_PATH.read_text(encoding="utf-8")

    # 1. Variable substitutions for narrative prose
    subs = {
        "{mc_name}": "Alex",
        "{mc_surname}": "Vance",
        "{mc_gender}": "they",
        "{mc_subj}": "they",
        "{mc_obj}": "them",
        "{mc_poss}": "their",
        "{mc_poss_adj}": "theirs",
        "{mc_refl}": "themselves",
        "{ro1_name}": "Julian",
        "{ro1_subj}": "he",
        "{ro1_Subj}": "He",
        "{ro1_obj}": "him",
        "{ro1_poss}": "his",
        "{ro1_Poss}": "His",
        "{ro2_name}": "Sloane",
        "{ro2_subj}": "she",
        "{ro2_Subj}": "She",
        "{ro2_obj}": "her",
        "{ro2_poss}": "her",
        "{ro2_Poss}": "Her",
        "{ro3_name}": "Harry",
        "{ro3_subj}": "he",
        "{ro3_Subj}": "He",
        "{ro3_obj}": "him",
        "{ro3_poss}": "his",
        "{ro3_Poss}": "His",
        "{ro4_name}": "Kieran",
        "{ro4_subj}": "he",
        "{ro4_Subj}": "He",
        "{ro4_obj}": "him",
        "{ro4_poss}": "his",
        "{ro4_Poss}": "His",
        "{2 - ch2_talk_count}": "1",
    }

    for k, v in subs.items():
        raw_text = raw_text.replace(k, v)

    # Split knots
    knots_split = re.split(r"(?m)^==\s*([A-Za-z0-9_]+)\s*==\s*$", raw_text)

    raw_knots = {}
    for i in range(1, len(knots_split), 2):
        raw_knots[knots_split[i].strip()] = knots_split[i + 1]

    passages = {}

    # Special handling for knots
    for kname, kbody in raw_knots.items():
        lines = kbody.splitlines()

        # If it's mirror_customization:
        if kname == "mirror_customization":
            narrative = []
            for l in lines:
                t = l.strip()
                if t.startswith("*"):
                    break
                if not t.startswith("//"):
                    narrative.append(l)
            passages[kname] = {
                "body": "\n".join(narrative).strip(),
                "flags": [],
                "choices": [
                    "* [A woman with tired eyes and steady hands (She / Her)] +mc_female -> select_mc_name",
                    "* [A man with tired eyes and steady hands (He / Him)] +mc_male -> select_mc_name",
                    "* [A non-binary resident with tired eyes and steady hands (They / Them)] +mc_nb -> select_mc_name",
                ],
            }
            continue

        # If it's set_mc_*: passthrough nodes
        if kname in ("set_mc_female", "set_mc_male", "set_mc_nb"):
            flag = kname.replace("set_", "")
            passages[kname] = {
                "body": "You steady your breath and straighten your scrubs in the mirror.",
                "flags": [flag],
                "choices": ["* [Continue] -> select_mc_name"],
            }
            continue

        # If it's select_mc_name:
        if kname == "select_mc_name":
            passages[kname] = {
                "body": 'What first name is printed above "CHIEF ER RESIDENT - VANCE" on your laminate badge?',
                "flags": [],
                "choices": [
                    "* [Dr. Alex Vance] -> chapter_one_start",
                    "* [Dr. Elena Vance] -> chapter_one_start",
                    "* [Dr. Morgan Vance] -> chapter_one_start",
                    "* [Dr. Rowan Vance] -> chapter_one_start",
                    "* [Dr. Claire Vance] -> chapter_one_start",
                    "* [Dr. Dante Vance] -> chapter_one_start",
                ],
            }
            continue

        # If it's storm_drain_march:
        if kname == "storm_drain_march":
            # clean out arithmetic and Ink logic
            body_text = """Seconds after Kieran's brass key unlocks the subterranean floodgate, Sloane's C4 charges detonate behind you.

A concussion wave of warm dust roars down the brick tunnel as St. Jude's Hospital collapses into its own foundation footprint, burying four thousand infected forever. Ankle-deep black rainwater swirls around your boots as your group pushes half a mile north through the dark storm drain toward the Mercer Street surface ladder.

{ toby_status == "alive":
Ahead of you, Toby limps between Harry and Julian, whispering quiet thank-yous in the dark.
- else:
Ahead of you, Harry walks ten paces in front in rigid, devastated silence, refusing to look back at you.
}

Halfway to the ladder shaft, your body finally cashes the check your adrenaline wrote.
A violent wave of vertigo slams into your skull. Beneath your wet scrub sleeve, the dormant pathogen in your silvered bite triggers a sudden vasovagal drop in blood pressure. The brick walls spin, and your knees buckle in the freezing water.

Who lunges through the dark water to catch you before you collapse?"""
            passages[kname] = {
                "body": body_text,
                "flags": [],
                "choices": [
                    "* [Dr. Julian Mercer lunges forward to catch your shoulders] -> tunnel_caught_by_julian",
                    "* [Commander Sloane hauls you upright with an iron grip] -> tunnel_caught_by_sloane",
                    "* [Harry catches you with steady, desperate hands] -> tunnel_caught_by_harry",
                    "* [Detective Kieran Mercer hooks a wiry arm around your waist] -> tunnel_caught_by_kieran",
                ],
            }
            continue

        # If it's check_any_suspicion_mid:
        if kname == "check_any_suspicion_mid":
            passages[kname] = {
                "body": "The apartment falls quiet as you finish bandaging in the dark.",
                "flags": [],
                "choices": ["* [Proceed to night watch] -> ch2_watch_intro"],
            }
            continue

        # If it's ch2_watch_intro:
        if kname == "ch2_watch_intro":
            passages[kname] = {
                "body": "It is 04:15 AM. Dawn breaks in ninety minutes.\nYou have time for private conversations while the others rest or keep watch around the dark apartment.",
                "flags": [],
                "choices": ["* [Begin Night Watch] -> ch2_talk_hub"],
            }
            continue

        # If it's ch2_talk_hub:
        if kname == "ch2_talk_hub":
            passages[kname] = {
                "body": "Who do you approach in the quiet before dawn?",
                "flags": [],
                "choices": [
                    "* [Slip into the candlelit bathroom to speak with Julian] +talked_ro1_ch2 -> talk_ro1_scene",
                    "* [Join Commander Sloane at the darkened front window] +talked_ro2_ch2 -> talk_ro2_scene",
                    "* [Sit with Harry on the hallway fire-escape landing] +talked_ro3_ch2 -> talk_ro3_scene",
                    "* [Corner Detective Kieran by the shadowed stairwell landing] +talked_ro4_ch2 -> talk_ro4_scene",
                ],
            }
            continue

        # If it's check_roX_suspicion_watch:
        if kname.startswith("check_ro") and kname.endswith("_suspicion_watch"):
            passages[kname] = {
                "body": "You step back into the dark apartment hallway as the clock ticks toward dawn.",
                "flags": [],
                "choices": ["* [Continue night watch] -> ch2_talk_hub_2"],
            }
            continue

        # If it's prep_with_*:
        if kname.startswith("prep_with_"):
            body_lines = []
            for l in lines:
                t = l.strip()
                if t.startswith("~") or t.startswith("//") or "-> bullpen_crisis" in t:
                    continue
                body_lines.append(l)
            passages[kname] = {
                "body": "\n".join(body_lines).strip(),
                "flags": [],
                "choices": ["* [Continue into the bullpen] -> bullpen_crisis"],
            }
            continue

        # If it's branch_save_toby:
        if kname == "branch_save_toby":
            body_lines = []
            for l in lines:
                t = l.strip()
                if t.startswith("~") or t.startswith("//") or "-> storm_drain_march" in t:
                    continue
                body_lines.append(l)
            passages[kname] = {
                "body": "\n".join(body_lines).strip(),
                "flags": ["toby_alive", "toby_status_alive", "saved_toby"],
                "choices": ["* [Retreat into the storm drain] -> storm_drain_march"],
            }
            continue

        # If it's branch_free_kieran or branch_ruthless_containment:
        if kname in ("branch_free_kieran", "branch_ruthless_containment"):
            body_lines = []
            for l in lines:
                t = l.strip()
                if t.startswith("~") or t.startswith("//") or "-> storm_drain_march" in t:
                    continue
                body_lines.append(l)
            flag = "freed_kieran" if kname == "branch_free_kieran" else "sealed_doors"
            passages[kname] = {
                "body": "\n".join(body_lines).strip(),
                "flags": ["toby_dead", flag],
                "choices": ["* [Retreat into the storm drain] -> storm_drain_march"],
            }
            continue

        # If it's ch2_triage_*:
        if kname.startswith("ch2_triage_"):
            body_lines = []
            for l in lines:
                t = l.strip()
                if t.startswith("~") or t.startswith("//") or "->" in t:
                    continue
                body_lines.append(l)
            passages[kname] = {
                "body": "\n".join(body_lines).strip(),
                "flags": [],
                "choices": ["* [Continue to night watch] -> ch2_watch_intro"],
            }
            continue

        # If it's ch2_ending_summary:
        if kname == "ch2_ending_summary":
            summary_text = """Together, your battered group moves into the ash-grey dawn toward safety.

----------------------------------------------------------------------------
END OF CHAPTER 2: STATE & ALLIANCE LEDGER
----------------------------------------------------------------------------
PROTAGONIST: Dr. Alex Vance
- Sub-basement collapse survived: Operation Cinderblock executed
- Patient Zero Chart: Preserved
- Group Status: Fortified at dawn

COMPANION ALLIANCES:
- Dr. Julian Mercer: Ex-partner & Pediatric Surgeon
- Commander Sloane: Bio-Containment Taskforce Leader
- Harry: Rig 9 Paramedic Partner
- Detective Kieran Mercer: Disgraced Lead Homicide Investigator

----------------------------------------------------------------------------
To be continued in Chapter 3: The Ferry Crossing..."""
            passages[kname] = {
                "body": summary_text,
                "flags": [],
                "choices": ["* [Restart Story] -> start"],
            }
            continue

        # General passage processing
        # Collect body lines, flags, choices, and handle choices with inline bodies
        body_lines = []
        node_flags = []
        choices = []

        sub_counter = 0
        curr_choice_label = None
        curr_choice_target = None
        curr_choice_body = []
        curr_choice_req = None
        curr_choice_sets = []

        def flush_choice():
            nonlocal curr_choice_label, curr_choice_target, curr_choice_body, curr_choice_req, curr_choice_sets, sub_counter
            if not curr_choice_label:
                return
            # If choice body has text and target is set, create intermediate sub-node!
            cleaned_cbody = "\n".join(curr_choice_body).strip()
            target = curr_choice_target or f"{kname}_next"

            if cleaned_cbody and target != kname:
                sub_counter += 1
                sub_id = f"{kname}_c{sub_counter}"
                passages[sub_id] = {
                    "body": cleaned_cbody,
                    "flags": [],
                    "choices": [f"* [Continue] -> {target}"],
                }
                target = sub_id

            req_str = f" {curr_choice_req}" if curr_choice_req else ""
            sets_str = "".join(f" +{f}" for f in curr_choice_sets)
            choices.append(f"* [{curr_choice_label}]{req_str}{sets_str} -> {target}".strip())

            curr_choice_label = None
            curr_choice_target = None
            curr_choice_body = []
            curr_choice_req = None
            curr_choice_sets = []

        for bline in lines:
            trimmed = bline.strip()

            # Check for node-level flags or script commands
            if trimmed.startswith("~"):
                if "saved_chart = true" in trimmed:
                    node_flags.append("saved_chart")
                elif "wearing_kieran_jacket = true" in trimmed:
                    node_flags.append("wearing_kieran_jacket")
                elif "has_lemon_tin = true" in trimmed:
                    node_flags.append("has_lemon_tin")
                elif "ro1_knows_bite = true" in trimmed:
                    node_flags.append("ro1_knows_bite")
                elif "ro2_knows_bite = true" in trimmed:
                    node_flags.append("ro2_knows_bite")
                elif "ro3_knows_bite = true" in trimmed:
                    node_flags.append("ro3_knows_bite")
                elif "ro4_knows_bite = true" in trimmed:
                    node_flags.append("ro4_knows_bite")
                continue

            if trimmed.startswith("//"):
                continue

            # Check for choice start
            if trimmed.startswith("*"):
                flush_choice()

                # Parse choice line
                m_cond = re.match(r"^\*\s*\{([^}]+)\}\s*\[([^\]]+)\](.*)$", trimmed)
                if m_cond:
                    cond_raw = m_cond.group(1).strip()
                    curr_choice_label = m_cond.group(2).strip()
                    after = m_cond.group(3).strip()
                    if 'toby_status == "alive"' in cond_raw:
                        curr_choice_req = "?toby_alive"
                    elif 'toby_status == "dead"' in cond_raw:
                        curr_choice_req = "?toby_dead"
                    elif "not talked_ro1_ch2" in cond_raw:
                        curr_choice_req = "?!talked_ro1_ch2"
                    elif "not talked_ro2_ch2" in cond_raw:
                        curr_choice_req = "?!talked_ro2_ch2"
                    elif "not talked_ro3_ch2" in cond_raw:
                        curr_choice_req = "?!talked_ro3_ch2"
                    elif "not talked_ro4_ch2" in cond_raw:
                        curr_choice_req = "?!talked_ro4_ch2"
                    elif "saved_chart" in cond_raw:
                        curr_choice_req = "?saved_chart"
                else:
                    m_plain = re.match(r"^\*\s*\[([^\]]+)\](.*)$", trimmed)
                    if m_plain:
                        curr_choice_label = m_plain.group(1).strip()
                        after = m_plain.group(2).strip()
                    else:
                        after = ""

                if "->" in after:
                    curr_choice_target = after.split("->", 1)[1].strip()
                continue

            if curr_choice_label:
                if "->" in trimmed:
                    curr_choice_target = trimmed.split("->", 1)[1].strip()
                else:
                    curr_choice_body.append(trimmed)
            else:
                if "->" in trimmed and not trimmed.startswith("*"):
                    # Bare divert at the end of node
                    curr_choice_target = trimmed.split("->", 1)[1].strip()
                    if curr_choice_target != "END":
                        choices.append(f"* [Continue] -> {curr_choice_target}")
                else:
                    body_lines.append(bline)

        flush_choice()

        passages[kname] = {
            "body": "\n".join(body_lines).strip(),
            "flags": node_flags,
            "choices": choices,
        }

    # Add second night watch hub for the 2-interaction mechanic
    passages["ch2_talk_hub_2"] = {
        "body": "It is 04:45 AM. You have time for one final private conversation before dawn breaks.",
        "flags": [],
        "choices": [
            "* [Slip into the bathroom to speak with Julian] ?!talked_ro1_ch2 -> talk_ro1_scene_2",
            "* [Join Commander Sloane at the darkened front window] ?!talked_ro2_ch2 -> talk_ro2_scene_2",
            "* [Sit with Harry on the hallway fire-escape landing] ?!talked_ro3_ch2 -> talk_ro3_scene_2",
            "* [Corner Detective Kieran by the shadowed stairwell] ?!talked_ro4_ch2 -> talk_ro4_scene_2",
            "* [Rest and wait for dawn with the group] -> ch2_dawn_breach",
        ],
    }

    # Add scene_2 duplicates that divert directly to ch2_dawn_breach
    for i in range(1, 5):
        orig = passages.get(f"talk_ro{i}_scene")
        if orig:
            passages[f"talk_ro{i}_scene_2"] = {
                "body": orig["body"],
                "flags": orig["flags"],
                "choices": [
                    f"* [Continue as dawn approaches] -> ch2_dawn_breach"
                ],
            }

    # Verify and fix all targets
    for pid, pdata in passages.items():
        new_choices = []
        for c in pdata["choices"]:
            if "->" in c:
                pre, tgt = c.split("->", 1)
                tgt = tgt.strip()
                if tgt not in passages:
                    if tgt == "locker_door_choice":
                        tgt = "julian_entrance"
                    elif tgt == "hallway_prep_hub":
                        tgt = "bullpen_crisis"
                    elif tgt == "END":
                        tgt = "start"
                    else:
                        tgt = "start"
                new_choices.append(f"{pre}-> {tgt}")
            else:
                new_choices.append(c)
        pdata["choices"] = new_choices

    # Build output STORY.TXT
    out_lines = [
        "TITLE: Dead Weight: Ch 1 & 2",
        "AUTHOR: Inkle Interactive Novel",
        "START: start",
    ]

    for pid, pdata in passages.items():
        out_lines.append("")
        out_lines.append(f"== {pid} ==")
        for f in pdata["flags"]:
            out_lines.append(f"+{f}")
        if pdata["body"]:
            out_lines.append(pdata["body"])
        for c in pdata["choices"]:
            out_lines.append(c)

    full_story = "\n".join(out_lines) + "\n"

    DEST_SD_PATH.write_text(full_story, encoding="utf-8")
    DEST_DOWNLOADS_PATH.write_text(full_story, encoding="utf-8")
    print(f"Generated {len(passages)} passages ({len(full_story)} bytes).")
    print(f"Wrote to:\n  - {DEST_SD_PATH}\n  - {DEST_DOWNLOADS_PATH}")


if __name__ == "__main__":
    convert_story()

