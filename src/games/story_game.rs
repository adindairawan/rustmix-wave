// Repo Path: src/games/story_game.rs

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::{
    canvas::{CanvasTextStyle, NativeGameCanvas},
    dirty_regions::DirtyRect,
};
use crate::buttons::ButtonEvent;

pub const STORY_FILE: &str = "STORY.TXT";
pub const STORY_SAVE_FILE: &str = "SAVE.TXT";
pub const STORY_DIR: &str = "STORIES";
pub const STORY_MAX_BYTES: u64 = 256 * 1024;
pub const STORY_CHARS_PER_LINE: usize = 42;
pub const STORY_LINES_PER_PAGE: usize = 12;
pub const STORY_MAX_UNDO: usize = 32;

const STORY_FULL_RECT: DirtyRect = DirtyRect::new(0, 0, 480, 800);
const STORY_BODY_RECT: DirtyRect = DirtyRect::new(16, 110, 448, 600);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoryChoice {
    pub label: String,
    pub target: String,
    pub require_flag: Option<(String, bool)>,
    pub set_flags: Vec<(String, bool)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoryNode {
    pub id: String,
    pub body: String,
    pub set_flags: Vec<(String, bool)>,
    pub choices: Vec<StoryChoice>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorySnapshot {
    pub node_id: String,
    pub flags: BTreeSet<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoryEventResult {
    pub reason: &'static str,
    pub node_index: usize,
    pub choice_index: usize,
    pub turns: u32,
    pub ended: bool,
    pub dirty_regions: Vec<DirtyRect>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoryGame {
    pub title: String,
    pub author: String,
    pub start_node: String,
    pub nodes: BTreeMap<String, StoryNode>,
    pub node_order: Vec<String>,
    pub current_node: String,
    pub flags: BTreeSet<String>,
    pub page: usize,
    pub selected_choice: usize,
    pub turns: u32,
    pub undo_stack: Vec<StorySnapshot>,
    pub save_path: Option<PathBuf>,
    pub status: String,
}

impl Default for StoryGame {
    fn default() -> Self {
        Self::new()
    }
}

impl StoryGame {
    #[must_use]
    pub fn new() -> Self {
        Self::from_story_text(DEFAULT_STORY).unwrap_or_else(|_| Self {
            title: "Story".into(),
            author: "RustMix".into(),
            start_node: "start".into(),
            nodes: BTreeMap::new(),
            node_order: Vec::new(),
            current_node: "start".into(),
            flags: BTreeSet::new(),
            page: 0,
            selected_choice: 0,
            turns: 0,
            undo_stack: Vec::new(),
            save_path: None,
            status: "Empty story".into(),
        })
    }

    pub fn from_app_dir(encoded: &str, app_dir: Option<&Path>) -> Result<Self, String> {
        let mut story_text: Option<String> = None;
        let mut save_path: Option<PathBuf> = None;

        if let Some(dir) = app_dir {
            // First check if a multiple-stories subfolder STORIES/ exists
            let stories_sub = dir.join(STORY_DIR);
            if stories_sub.is_dir() {
                if let Some(hub) = build_story_hub_text(&stories_sub) {
                    story_text = Some(hub);
                    save_path = Some(dir.join(STORY_SAVE_FILE));
                }
            }

            if story_text.is_none() {
                let candidate = dir.join(STORY_FILE);
                if let Some(text) = read_bounded_text(&candidate) {
                    story_text = Some(text);
                }
                save_path = Some(dir.join(STORY_SAVE_FILE));
            }
        }

        let source = if let Some(text) = story_text {
            text
        } else {
            let trimmed = encoded.trim();
            if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("default") {
                DEFAULT_STORY.to_string()
            } else {
                trimmed.replace("\\n", "\n")
            }
        };

        let mut game = Self::from_story_text(&source)?;
        game.save_path = save_path;
        game.try_load_save();
        Ok(game)
    }

    pub fn from_story_text(text: &str) -> Result<Self, String> {
        let mut title = String::from("Interactive Story");
        let mut author = String::from("SD Story");
        let mut start_override: Option<String> = None;
        let mut nodes = BTreeMap::new();
        let mut node_order = Vec::new();

        let mut current_id: Option<String> = None;
        let mut current_lines: Vec<String> = Vec::new();
        let mut current_flags: Vec<(String, bool)> = Vec::new();
        let mut current_choices: Vec<StoryChoice> = Vec::new();

        let flush_node = |id_opt: &mut Option<String>,
                          lines: &mut Vec<String>,
                          flags: &mut Vec<(String, bool)>,
                          choices: &mut Vec<StoryChoice>,
                          nodes: &mut BTreeMap<String, StoryNode>,
                          order: &mut Vec<String>| {
            if let Some(id) = id_opt.take() {
                let body = lines.join("\n").trim().to_string();
                if !nodes.contains_key(&id) {
                    order.push(id.clone());
                }
                nodes.insert(
                    id.clone(),
                    StoryNode {
                        id,
                        body,
                        set_flags: std::mem::take(flags),
                        choices: std::mem::take(choices),
                    },
                );
                lines.clear();
            }
        };

        for raw in text.lines() {
            let trimmed = raw.trim();
            if trimmed.starts_with("--") || trimmed.starts_with("//") {
                continue;
            }

            if trimmed.starts_with("==") && trimmed.ends_with("==") && trimmed.len() > 4 {
                flush_node(
                    &mut current_id,
                    &mut current_lines,
                    &mut current_flags,
                    &mut current_choices,
                    &mut nodes,
                    &mut node_order,
                );
                let id = trimmed[2..trimmed.len() - 2].trim().to_string();
                if !id.is_empty() {
                    current_id = Some(id);
                }
                continue;
            }

            if current_id.is_none() {
                if let Some((key, val)) = trimmed.split_once(':') {
                    let k = key.trim().to_ascii_uppercase();
                    let v = val.trim();
                    match k.as_str() {
                        "TITLE" if !v.is_empty() => title = v.to_string(),
                        "AUTHOR" if !v.is_empty() => author = v.to_string(),
                        "START" if !v.is_empty() => start_override = Some(v.to_string()),
                        _ => {}
                    }
                }
                continue;
            }

            if trimmed.starts_with("* ") || trimmed.starts_with("*[") {
                if let Some(choice) = parse_choice_line(trimmed) {
                    current_choices.push(choice);
                    continue;
                }
            }

            if let Some(flag_name) = trimmed.strip_prefix('+') {
                let f = flag_name.trim();
                if is_flag_ident(f) {
                    current_flags.push((f.to_string(), true));
                    continue;
                }
            }
            if let Some(flag_name) = trimmed.strip_prefix('-') {
                let f = flag_name.trim();
                if is_flag_ident(f) {
                    current_flags.push((f.to_string(), false));
                    continue;
                }
            }

            current_lines.push(raw.trim_end().to_string());
        }

        flush_node(
            &mut current_id,
            &mut current_lines,
            &mut current_flags,
            &mut current_choices,
            &mut nodes,
            &mut node_order,
        );

        if nodes.is_empty() {
            return Err("STORY.TXT contains no '== node ==' passages".into());
        }

        let start_node = start_override
            .filter(|s| nodes.contains_key(s))
            .or_else(|| {
                if nodes.contains_key("start") {
                    Some("start".to_string())
                } else {
                    node_order.first().cloned()
                }
            })
            .unwrap_or_else(|| "start".to_string());

        let mut game = Self {
            title,
            author,
            start_node: start_node.clone(),
            nodes,
            node_order,
            current_node: start_node.clone(),
            flags: BTreeSet::new(),
            page: 0,
            selected_choice: 0,
            turns: 0,
            undo_stack: Vec::new(),
            save_path: None,
            status: "UP/DN select  SELECT choose  BOOT back".into(),
        };
        game.apply_node_flags(&start_node);
        Ok(game)
    }

    fn apply_node_flags(&mut self, node_id: &str) {
        if let Some(node) = self.nodes.get(node_id).cloned() {
            for (flag, state) in node.set_flags {
                if state {
                    self.flags.insert(flag);
                } else {
                    self.flags.remove(&flag);
                }
            }
        }
    }

    #[must_use]
    pub fn visible_choices(&self) -> Vec<StoryChoice> {
        let Some(node) = self.nodes.get(&self.current_node) else {
            return Vec::new();
        };
        node.choices
            .iter()
            .filter(|c| match &c.require_flag {
                None => true,
                Some((flag, expected)) => self.flags.contains(flag) == *expected,
            })
            .cloned()
            .collect()
    }

    #[must_use]
    pub fn wrapped_pages(&self) -> Vec<Vec<String>> {
        let body = self
            .nodes
            .get(&self.current_node)
            .map(|n| n.body.as_str())
            .unwrap_or("(Missing passage)");
        let lines = wrap_passage(body, STORY_CHARS_PER_LINE);
        if lines.is_empty() {
            return vec![vec![String::new()]];
        }
        lines
            .chunks(STORY_LINES_PER_PAGE)
            .map(|chunk| chunk.to_vec())
            .collect()
    }

    #[must_use]
    pub fn is_ending(&self) -> bool {
        self.visible_choices().is_empty()
    }

    pub fn restart(&mut self) {
        self.current_node = self.start_node.clone();
        self.flags.clear();
        self.page = 0;
        self.selected_choice = 0;
        self.turns = 0;
        self.undo_stack.clear();
        let start = self.start_node.clone();
        self.apply_node_flags(&start);
        self.status = "Story restarted".into();
        self.persist_save();
    }

    pub fn choose_selected(&mut self) -> bool {
        let choices = self.visible_choices();
        let Some(choice) = choices.get(self.selected_choice).cloned() else {
            return false;
        };
        if !self.nodes.contains_key(&choice.target) {
            self.status = format!("Missing passage: {}", choice.target);
            return false;
        }
        if self.undo_stack.len() >= STORY_MAX_UNDO {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(StorySnapshot {
            node_id: self.current_node.clone(),
            flags: self.flags.clone(),
        });
        for (flag, state) in choice.set_flags {
            if state {
                self.flags.insert(flag);
            } else {
                self.flags.remove(&flag);
            }
        }
        self.current_node = choice.target.clone();
        self.apply_node_flags(&choice.target);
        self.page = 0;
        self.selected_choice = 0;
        self.turns += 1;
        self.status = if self.is_ending() {
            "THE END  SELECT restart  BOOT undo".into()
        } else {
            format!("Scene: {}", self.current_node)
        };
        self.persist_save();
        true
    }

    pub fn undo(&mut self) -> bool {
        let Some(prev) = self.undo_stack.pop() else {
            return false;
        };
        self.current_node = prev.node_id;
        self.flags = prev.flags;
        self.page = 0;
        self.selected_choice = 0;
        self.turns = self.turns.saturating_sub(1);
        self.status = format!("Back to {}", self.current_node);
        self.persist_save();
        true
    }

    fn persist_save(&self) {
        let Some(path) = &self.save_path else {
            return;
        };
        let mut out = format!("NODE={}\nTURNS={}\n", self.current_node, self.turns);
        for flag in &self.flags {
            out.push_str(&format!("FLAG={flag}\n"));
        }
        let _ = std::fs::write(path, out);
    }

    fn try_load_save(&mut self) {
        let Some(path) = &self.save_path else {
            return;
        };
        let Some(raw) = read_bounded_text(path) else {
            return;
        };
        let mut node: Option<String> = None;
        let mut turns = 0u32;
        let mut flags = BTreeSet::new();
        for line in raw.lines() {
            let line = line.trim();
            if let Some(n) = line.strip_prefix("NODE=") {
                if self.nodes.contains_key(n) {
                    node = Some(n.to_string());
                }
            } else if let Some(t) = line.strip_prefix("TURNS=") {
                turns = t.parse().unwrap_or(0);
            } else if let Some(f) = line.strip_prefix("FLAG=") {
                if !f.is_empty() {
                    flags.insert(f.to_string());
                }
            }
        }
        if let Some(n) = node {
            self.current_node = n;
            self.turns = turns;
            self.flags = flags;
            self.status = format!("Resumed: {}", self.current_node);
        }
    }

    pub fn render_initial(&self, canvas: &mut NativeGameCanvas) -> Result<(), String> {
        self.render_commands(canvas)?;
        canvas.reset_dirty_regions();
        canvas.invalidate_rect(STORY_FULL_RECT);
        canvas.request_refresh();
        Ok(())
    }

    pub fn apply_button_and_render(
        &mut self,
        event: ButtonEvent,
        canvas: &mut NativeGameCanvas,
    ) -> Result<Option<StoryEventResult>, String> {
        let pages_len = self.wrapped_pages().len().max(1);
        let choices_len = self.visible_choices().len();

        let reason = match event {
            ButtonEvent::Up => {
                if self.page + 1 == pages_len && choices_len > 0 && self.selected_choice > 0 {
                    self.selected_choice -= 1;
                    "choice-up"
                } else if self.page > 0 {
                    self.page -= 1;
                    "page-prev"
                } else if choices_len > 0 {
                    self.selected_choice = choices_len - 1;
                    "choice-wrap"
                } else {
                    "top"
                }
            }
            ButtonEvent::Down => {
                if self.page + 1 < pages_len {
                    self.page += 1;
                    "page-next"
                } else if choices_len > 0 {
                    self.selected_choice = (self.selected_choice + 1) % choices_len;
                    "choice-down"
                } else {
                    "end"
                }
            }
            ButtonEvent::Select => {
                if self.page + 1 < pages_len {
                    self.page += 1;
                    "page-next"
                } else if choices_len == 0 {
                    self.restart();
                    "restart"
                } else if self.choose_selected() {
                    "choose"
                } else {
                    "invalid-target"
                }
            }
        };

        self.render_commands(canvas)?;
        canvas.reset_dirty_regions();
        let dirty_regions = vec![STORY_FULL_RECT, STORY_BODY_RECT];
        for rect in &dirty_regions {
            canvas.invalidate_rect(*rect);
        }
        canvas.request_refresh();
        Ok(Some(self.build_result(reason, dirty_regions)))
    }

    pub fn apply_boot_short_press_and_render(
        &mut self,
        canvas: &mut NativeGameCanvas,
    ) -> Result<StoryEventResult, String> {
        let reason = if self.is_ending() || self.undo_stack.is_empty() {
            self.restart();
            "restart"
        } else if self.undo() {
            "undo"
        } else {
            "undo-empty"
        };
        self.render_commands(canvas)?;
        canvas.reset_dirty_regions();
        let dirty_regions = vec![STORY_FULL_RECT, STORY_BODY_RECT];
        for rect in &dirty_regions {
            canvas.invalidate_rect(*rect);
        }
        canvas.request_refresh();
        Ok(self.build_result(reason, dirty_regions))
    }

    fn build_result(
        &self,
        reason: &'static str,
        dirty_regions: Vec<DirtyRect>,
    ) -> StoryEventResult {
        let node_index = self
            .node_order
            .iter()
            .position(|id| id == &self.current_node)
            .unwrap_or(0);
        StoryEventResult {
            reason,
            node_index,
            choice_index: self.selected_choice,
            turns: self.turns,
            ended: self.is_ending(),
            dirty_regions,
        }
    }

    fn render_commands(&self, canvas: &mut NativeGameCanvas) -> Result<(), String> {
        canvas.clear_frame();
        let pages = self.wrapped_pages();
        let pages_len = pages.len().max(1);
        let page_idx = self.page.min(pages_len - 1);

        canvas.text(20, 76, self.title.clone(), CanvasTextStyle::Heading)?;
        canvas.text(
            20,
            104,
            format!(
                "{}  |  Turn {}  |  Pg {}/{}",
                self.author,
                self.turns,
                page_idx + 1,
                pages_len
            ),
            CanvasTextStyle::Detail,
        )?;
        canvas.line(20, 114, 460, 114)?;

        let mut y = 144;
        for line in &pages[page_idx] {
            if !line.is_empty() {
                canvas.text(20, y, line.clone(), CanvasTextStyle::Body)?;
            }
            y += 28;
        }

        canvas.line(20, 496, 460, 496)?;

        if page_idx + 1 < pages_len {
            canvas.text(
                20,
                532,
                "> [More...] Press DOWN or SELECT".into(),
                CanvasTextStyle::Heading,
            )?;
        } else {
            let choices = self.visible_choices();
            if choices.is_empty() {
                canvas.text(20, 532, "--- THE END ---".into(), CanvasTextStyle::Heading)?;
                canvas.text(
                    20,
                    568,
                    "> Press SELECT to play again".into(),
                    CanvasTextStyle::Body,
                )?;
            } else {
                let mut cy = 520;
                for (idx, choice) in choices.iter().take(4).enumerate() {
                    let is_sel = idx == self.selected_choice;
                    let prefix = if is_sel { ">" } else { " " };
                    let style = if is_sel {
                        CanvasTextStyle::Heading
                    } else {
                        CanvasTextStyle::Body
                    };

                    let full_label = format!("{prefix} {}. {}", idx + 1, choice.label);
                    // Word-wrap long choice labels up to 2 lines so they don't bleed off screen
                    let wrapped = wrap_passage(&full_label, 36);
                    if let Some(first_line) = wrapped.first() {
                        canvas.text(20, cy, first_line.clone(), style)?;
                        cy += 24;
                    }
                    if let Some(second_line) = wrapped.get(1) {
                        let indent = format!("     {second_line}");
                        canvas.text(20, cy, indent, style)?;
                        cy += 24;
                    } else {
                        cy += 6;
                    }
                }
            }
        }

        canvas.text(20, 706, self.status.clone(), CanvasTextStyle::Detail)?;
        let hint = if self.undo_stack.is_empty() {
            "UP/DN choose   SELECT ok   BOOT restart"
        } else {
            "UP/DN choose   SELECT ok   BOOT undo"
        };
        canvas.text(20, 736, hint.into(), CanvasTextStyle::Detail)?;
        Ok(())
    }
}

fn is_flag_ident(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn parse_choice_line(line: &str) -> Option<StoryChoice> {
    let rest = line.trim_start_matches('*').trim();
    let open = rest.find('[')?;
    let close = rest[open + 1..].find(']')? + open + 1;
    let label = rest[open + 1..close].trim().to_string();
    let after = rest[close + 1..].trim();
    let (meta, target_raw) = after.split_once("->")?;
    let target = target_raw.trim().to_string();
    if label.is_empty() || target.is_empty() {
        return None;
    }

    let mut require_flag = None;
    let mut set_flags = Vec::new();
    for token in meta.split_whitespace() {
        if let Some(f) = token.strip_prefix("?!") {
            if is_flag_ident(f) {
                require_flag = Some((f.to_string(), false));
            }
        } else if let Some(f) = token.strip_prefix('?') {
            if is_flag_ident(f) {
                require_flag = Some((f.to_string(), true));
            }
        } else if let Some(f) = token.strip_prefix('+') {
            if is_flag_ident(f) {
                set_flags.push((f.to_string(), true));
            }
        } else if let Some(f) = token.strip_prefix('-') {
            if is_flag_ident(f) {
                set_flags.push((f.to_string(), false));
            }
        }
    }

    Some(StoryChoice {
        label,
        target,
        require_flag,
        set_flags,
    })
}

fn wrap_passage(text: &str, max_chars: usize) -> Vec<String> {
    let mut out = Vec::new();
    for raw_line in text.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            if out.last().is_some_and(|l: &String| !l.is_empty()) {
                out.push(String::new());
            }
            continue;
        }
        let mut current = String::new();
        for word in trimmed.split_whitespace() {
            if current.is_empty() {
                current.push_str(word);
            } else if current.len() + 1 + word.len() <= max_chars {
                current.push(' ');
                current.push_str(word);
            } else {
                out.push(current);
                current = word.to_string();
            }
        }
        if !current.is_empty() {
            out.push(current);
        }
    }
    out
}

fn read_bounded_text(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() > STORY_MAX_BYTES {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

fn build_story_hub_text(dir: &Path) -> Option<String> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut files = Vec::new();
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_file() {
            if let Some(ext) = p.extension() {
                if ext.eq_ignore_ascii_case("txt") {
                    files.push(p);
                }
            }
        }
    }
    files.sort();
    if files.is_empty() {
        return None;
    }

    let mut hub = String::from("TITLE: Story Collection\nAUTHOR: SD Stories\nSTART: start\n\n== start ==\nSelect a story to play:\n");
    let mut stories_content = String::new();

    for (idx, path) in files.iter().enumerate() {
        let Some(text) = read_bounded_text(path) else {
            continue;
        };
        let prefix = format!("s{}", idx + 1);

        // Read metadata
        let mut title = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled")
            .to_string();
        for line in text.lines() {
            let l = line.trim();
            if let Some(t) = l.strip_prefix("TITLE:") {
                title = t.trim().to_string();
                break;
            }
            if l.starts_with("==") {
                break;
            }
        }

        hub.push_str(&format!("* [Play: {}] -> {prefix}_start\n", title));

        // Prefix all node identifiers in this story to avoid collision
        let mut current_node = String::new();
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("TITLE:")
                || trimmed.starts_with("AUTHOR:")
                || trimmed.starts_with("START:")
            {
                continue;
            }
            if trimmed.starts_with("==") && trimmed.ends_with("==") && trimmed.len() > 4 {
                let id = trimmed[2..trimmed.len() - 2].trim();
                current_node = format!("{prefix}_{id}");
                stories_content.push_str(&format!("\n== {current_node} ==\n"));
                continue;
            }
            if current_node.is_empty() {
                continue;
            }
            if let Some((before_arrow, target)) = line.split_once("->") {
                let new_target = format!("{prefix}_{}", target.trim());
                stories_content.push_str(&format!("{before_arrow}-> {new_target}\n"));
            } else {
                stories_content.push_str(&format!("{line}\n"));
            }
        }
    }

    hub.push_str(&stories_content);
    Some(hub)
}

pub const DEFAULT_STORY: &str = r#"TITLE: The Saltmire Beacon
AUTHOR: RustMix Interactive

== start ==
Cold rain taps the glass of the old lighthouse. On the oak desk sits a brass lantern beside a bolted iron trapdoor.

A wet spiral staircase winds down into the dark.
* [Inspect the brass lantern] ?!has_key -> lantern
* [Unlock the trapdoor] ?has_key -> cellar
* [Descend the spiral stairs] -> stairs

== lantern ==
+has_key
You lift the brass lantern. Tucked underneath the oil can is a heavy iron key stamped with an anchor.
* [Pocket the key and step back] -> start

== stairs ==
Halfway down, the sea wind howls through a cracked shutter. You spot a frayed signal rope tied to the bell.
* [Pull the signal rope] -> bell_ending
* [Climb back up to the lantern room] -> start

== cellar ==
The iron key turns with a dry click. Below the trapdoor lies a dry cedar chest holding the missing harbour logbook — and a warm thermos of tea.

You found the Saltmire logbook!

== bell_ending ==
The bronze bell booms across the bay. Minutes later, a fishing boat answers with its searchlight, guiding you safely off the shoals.

Safe rescue — Ending 2 of 2.
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_story_plays_with_flags_and_undo() {
        let mut game = StoryGame::new();
        assert_eq!(game.current_node, "start");
        assert_eq!(game.visible_choices().len(), 2);

        let mut canvas = NativeGameCanvas::default();
        game.render_initial(&mut canvas).unwrap();
        assert!(!canvas.commands().is_empty());

        // Pick "Inspect the brass lantern" -> sets has_key.
        game.apply_button_and_render(ButtonEvent::Select, &mut canvas)
            .unwrap();
        assert_eq!(game.current_node, "lantern");
        assert!(game.flags.contains("has_key"));

        // Go back to start -> now "Unlock the trapdoor" is unlocked!
        game.apply_button_and_render(ButtonEvent::Select, &mut canvas)
            .unwrap();
        assert_eq!(game.current_node, "start");
        let choices = game.visible_choices();
        assert_eq!(choices.len(), 2);
        assert_eq!(choices[0].target, "cellar");

        // Undo returns to lantern.
        game.apply_boot_short_press_and_render(&mut canvas).unwrap();
        assert_eq!(game.current_node, "lantern");
    }

    #[test]
    fn loads_story_txt_and_persists_save_in_app_dir() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rustmix-story-{nonce}"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("STORY.TXT"),
            "TITLE: Mini Test\n== start ==\nHello.\n* [Go] +done -> end\n== end ==\nFin.\n",
        )
        .unwrap();

        let mut game = StoryGame::from_app_dir("default", Some(&dir)).unwrap();
        assert_eq!(game.title, "Mini Test");
        let mut canvas = NativeGameCanvas::default();
        game.apply_button_and_render(ButtonEvent::Select, &mut canvas)
            .unwrap();
        assert_eq!(game.current_node, "end");
        assert!(game.is_ending());

        // Reloading from same app_dir resumes at "end".
        let resumed = StoryGame::from_app_dir("default", Some(&dir)).unwrap();
        assert_eq!(resumed.current_node, "end");
        assert!(resumed.flags.contains("done"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn loads_multiple_stories_from_subfolder_and_builds_menu() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let app_dir = std::env::temp_dir().join(format!("rustmix-story-hub-{nonce}"));
        let stories_dir = app_dir.join(STORY_DIR);
        std::fs::create_dir_all(&stories_dir).unwrap();

        std::fs::write(
            stories_dir.join("BEACON.TXT"),
            "TITLE: Saltmire Beacon\n== start ==\nLighthouse rain.\n* [Look] -> look\n== look ==\nBright.\n",
        )
        .unwrap();

        std::fs::write(
            stories_dir.join("DEADWGT.TXT"),
            "TITLE: Dead Weight\n== start ==\nZombies outside.\n* [Fight] -> fight\n== fight ==\nVictory.\n",
        )
        .unwrap();

        let mut game = StoryGame::from_app_dir("default", Some(&app_dir)).unwrap();
        assert_eq!(game.current_node, "start");
        let choices = game.visible_choices();
        assert_eq!(choices.len(), 2);
        assert_eq!(choices[0].label, "Play: Saltmire Beacon");
        assert_eq!(choices[1].label, "Play: Dead Weight");

        let mut canvas = NativeGameCanvas::default();
        // Pick Dead Weight (index 1)
        game.selected_choice = 1;
        game.apply_button_and_render(ButtonEvent::Select, &mut canvas)
            .unwrap();
        assert_eq!(game.current_node, "s2_start");
        assert!(game
            .nodes
            .get("s2_start")
            .unwrap()
            .body
            .contains("Zombies outside"));

        std::fs::remove_dir_all(app_dir).unwrap();
    }
}
