#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/convert-if-story.sh <input.twee|input.ink> [-o STORY.TXT] [--title TITLE] [--author AUTHOR]

Converts a .twee (Twine) or .ink (Inkle Ink) file into RustMix STORY.TXT format
without requiring Python.
EOF
}

if [[ $# -lt 1 ]]; then
  usage >&2
  exit 1
fi

INPUT=""
OUTPUT="STORY.TXT"
TITLE=""
AUTHOR="Community IF"

while [[ $# -gt 0 ]]; do
  case "$1" in
    -h|--help)
      usage
      exit 0
      ;;
    -o|--output)
      OUTPUT="${2:?missing output path}"
      shift 2
      ;;
    --title)
      TITLE="${2:?missing title}"
      shift 2
      ;;
    --author)
      AUTHOR="${2:?missing author}"
      shift 2
      ;;
    -*)
      echo "Unknown option: $1" >&2
      usage >&2
      exit 1
      ;;
    *)
      if [[ -z "$INPUT" ]]; then
        INPUT="$1"
      else
        echo "Unexpected argument: $1" >&2
        exit 1
      fi
      shift
      ;;
  esac
done

if [[ -z "$INPUT" || ! -f "$INPUT" ]]; then
  echo "Input file not found: $INPUT" >&2
  exit 1
fi

if [[ -z "$TITLE" ]]; then
  base="$(basename "$INPUT")"
  base="${base%.*}"
  TITLE="$(printf '%s' "$base" | tr '_-' '  ')"
fi

ext="${INPUT##*.}"
ext_lower="$(printf '%s' "$ext" | tr '[:upper:]' '[:lower:]')"

case "$ext_lower" in
  twee|tw)
    awk -v title="$TITLE" -v author="$AUTHOR" '
      function trim(s) {
        gsub(/^[ \t\r]+|[ \t\r]+$/, "", s)
        return s
      }
      function slug(s,   out) {
        out = tolower(trim(s))
        gsub(/[^a-z0-9_-]+/, "_", out)
        gsub(/^_+|_+$/, "", out)
        return (out == "" ? "node" : out)
      }
      function flush_passage(   i, n, line, s, lbl, tgt, rest, tmp) {
        if (cur_name == "") return
        n = tolower(cur_name)
        if (n == "storytitle") {
          for (i = 1; i <= body_count; i++) {
            s = trim(body[i])
            if (s != "") { story_title = s; break }
          }
          return
        }
        if (n == "storydata" || n == "storyinit" || n == "stylesheet" || n == "script") {
          return
        }
        nid = slug(cur_name)
        if (n == "start") start_id = nid
        out_count++
        out_lines[out_count] = ""
        out_count++
        out_lines[out_count] = "== " nid " =="
        choice_count = 0
        for (i = 1; i <= body_count; i++) {
          line = body[i]
          gsub(/\r$/, "", line)
          s = trim(line)
          # Pure choice line(s) like [[Label->Target]] or [[Label|Target]] or [[Target]]
          tmp = s
          gsub(/\[\[[^\]]+\]\]/, "", tmp)
          gsub(/[*>-]/, "", tmp)
          if (s ~ /\[\[[^\]]+\]\]/ && trim(tmp) == "") {
            rest = s
            while (match(rest, /\[\[[^\]]+\]\]/)) {
              inner = substr(rest, RSTART + 2, RLENGTH - 4)
              rest = substr(rest, RSTART + RLENGTH)
              if (index(inner, "->") > 0) {
                pos = index(inner, "->")
                lbl = trim(substr(inner, 1, pos - 1))
                tgt = trim(substr(inner, pos + 2))
              } else if (index(inner, "|") > 0) {
                pos = index(inner, "|")
                lbl = trim(substr(inner, 1, pos - 1))
                tgt = trim(substr(inner, pos + 1))
              } else {
                lbl = trim(inner)
                tgt = lbl
              }
              choice_count++
              choices[choice_count] = "* [" lbl "] -> " slug(tgt)
            }
            continue
          }
          gsub(/<<[^>]+>>/, "", line)
          gsub(/\[\[|\]\]/, "", line)
          out_count++
          out_lines[out_count] = line
        }
        for (i = 1; i <= choice_count; i++) {
          out_count++
          out_lines[out_count] = choices[i]
        }
      }
      BEGIN {
        story_title = title
        start_id = "start"
        cur_name = ""
        body_count = 0
        out_count = 0
      }
      /^::/ {
        flush_passage()
        hdr = substr($0, 3)
        gsub(/\r$/, "", hdr)
        gsub(/\[[^\]]*\]/, "", hdr)
        gsub(/\{[^}]*\}/, "", hdr)
        cur_name = trim(hdr)
        body_count = 0
        next
      }
      {
        if (cur_name != "") {
          body_count++
          body[body_count] = $0
        }
      }
      END {
        flush_passage()
        print "TITLE: " story_title
        print "AUTHOR: " author
        print "START: " start_id
        for (i = 1; i <= out_count; i++) {
          print out_lines[i]
        }
      }
    ' "$INPUT" > "$OUTPUT"
    ;;

  ink)
    awk -v title="$TITLE" -v author="$AUTHOR" '
      function trim(s) {
        gsub(/^[ \t\r]+|[ \t\r]+$/, "", s)
        return s
      }
      function slug(s,   out) {
        out = tolower(trim(s))
        gsub(/[^a-z0-9_-]+/, "_", out)
        gsub(/^_+|_+$/, "", out)
        return (out == "" ? "node" : out)
      }
      function flush_knot(   i) {
        if (!started && body_count == 0 && choice_count == 0) return
        print ""
        print "== " cur_node " =="
        for (i = 1; i <= body_count; i++) print body[i]
        for (i = 1; i <= choice_count; i++) print choices[i]
        body_count = 0
        choice_count = 0
      }
      BEGIN {
        print "TITLE: " title
        print "AUTHOR: " author
        cur_node = "start"
        started = 0
        body_count = 0
        choice_count = 0
      }
      {
        line = $0
        gsub(/\r$/, "", line)
        pos = index(line, "//")
        if (pos > 0) line = substr(line, 1, pos - 1)
        line = trim(line)
        if (line == "") {
          if (body_count > 0 && body[body_count] != "") {
            body_count++
            body[body_count] = ""
          }
          next
        }
        if (line ~ /^==+[ \t]*[A-Za-z0-9_]+[ \t]*=*$/) {
          flush_knot()
          gsub(/^=+|=+$/, "", line)
          cur_node = slug(line)
          started = 1
          next
        }
        if (line ~ /^[*+]+[ \t]*\[[^\]]+\]/) {
          open_pos = index(line, "[")
          close_pos = index(line, "]")
          lbl = trim(substr(line, open_pos + 1, close_pos - open_pos - 1))
          rest = substr(line, close_pos + 1)
          tgt = cur_node
          if (match(rest, /->[ \t]*[A-Za-z0-9_]+/)) {
            tgt = substr(rest, RSTART + 2, RLENGTH - 2)
            tgt = slug(tgt)
          }
          choice_count++
          choices[choice_count] = "* [" lbl "] -> " tgt
          next
        }
        if (line ~ /^->[ \t]*[A-Za-z0-9_]+/) {
          tgt = slug(substr(line, 3))
          if (tgt != "end" && tgt != "done") {
            choice_count++
            choices[choice_count] = "* [Continue] -> " tgt
          }
          next
        }
        if (line ~ /^VAR / || line ~ /^~/) next
        body_count++
        body[body_count] = line
      }
      END {
        flush_knot()
      }
    ' "$INPUT" > "$OUTPUT"
    ;;

  *)
    echo "Unsupported extension '.$ext' (expected .twee, .tw, or .ink)" >&2
    exit 1
    ;;
esac

echo "Wrote $OUTPUT"
