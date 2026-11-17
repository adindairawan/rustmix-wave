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
pub const STORY_MAX_BYTES: u64 = 1024 * 1024;
pub const STORY_CHARS_PER_LINE: usize = 42;
pub const STORY_LINES_PER_PAGE: usize = 12;
pub const STORY_MAX_UNDO: usize = 32;
pub const STORY_SETTINGS_COUNT: usize = 4;
pub const STORY_CHOICES_WINDOW_SIZE: usize = 3;

const STORY_FULL_RECT: DirtyRect = DirtyRect::new(0, 0, 480, 800);
const STORY_BODY_RECT: DirtyRect = DirtyRect::new(16, 110, 448, 600);

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum StorySettingsOption {
    Resume = 0,
    PreviousDecision = 1,
    Restart = 2,
    QuitStory = 3,
}

impl StorySettingsOption {
    pub const fn from_index(idx: usize) -> Option<Self> {
        match idx {
            0 => Some(Self::Resume),
            1 => Some(Self::PreviousDecision),
            2 => Some(Self::Restart),
            3 => Some(Self::QuitStory),
            _ => None,
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::Resume => "Resume Story",
            Self::PreviousDecision => "Previous Decision (Undo)",
            Self::Restart => "Restart Story",
            Self::QuitStory => "Quit to Story Menu",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Resume => "Return to the current passage",
            Self::PreviousDecision => "Step back to your previous choice",
            Self::Restart => "Start over from the beginning",
            Self::QuitStory => "Exit story and return to catalog",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoryChoice {
    pub label: String,
    pub target: String,
    pub require_flag: Option<(String, bool)>,
    pub set_flags: Vec<(String, bool)>,
    pub set_vars: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoryNode {
    pub id: String,
    pub body: String,
    pub set_flags: Vec<(String, bool)>,
    pub set_vars: Vec<(String, String)>,
    pub choices: Vec<StoryChoice>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorySnapshot {
    pub node_id: String,
    pub flags: BTreeSet<String>,
    pub vars: BTreeMap<String, String>,
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
    pub vars: BTreeMap<String, String>,
    pub initial_vars: BTreeMap<String, String>,
    pub page: usize,
    pub selected_choice: usize,
    pub turns: u32,
    pub undo_stack: Vec<StorySnapshot>,
    pub save_path: Option<PathBuf>,
    pub status: String,
    pub in_settings: bool,
    pub settings_cursor: usize,
    pub choice_scroll_offset: usize,
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
            vars: BTreeMap::new(),
            initial_vars: BTreeMap::new(),
            page: 0,
            selected_choice: 0,
            turns: 0,
            undo_stack: Vec::new(),
            save_path: None,
            status: "Empty story".into(),
            in_settings: false,
            settings_cursor: 0,
            choice_scroll_offset: 0,
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
        let mut initial_vars = BTreeMap::new();

        let mut current_id: Option<String> = None;
        let mut current_lines: Vec<String> = Vec::new();
        let mut current_flags: Vec<(String, bool)> = Vec::new();
        let mut current_vars: Vec<(String, String)> = Vec::new();
        let mut current_choices: Vec<StoryChoice> = Vec::new();

        let flush_node = |id_opt: &mut Option<String>,
                          lines: &mut Vec<String>,
                          flags: &mut Vec<(String, bool)>,
                          vars: &mut Vec<(String, String)>,
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
                        set_vars: std::mem::take(vars),
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
                let id = trimmed[2..trimmed.len() - 2].trim().to_string();
                if !id.is_empty() && !id.chars().all(|c| c == '=') {
                    flush_node(
                        &mut current_id,
                        &mut current_lines,
                        &mut current_flags,
                        &mut current_vars,
                        &mut current_choices,
                        &mut nodes,
                        &mut node_order,
                    );
                    current_id = Some(id);
                    continue;
                }
            }

            if let Some(rest) = trimmed
                .strip_prefix("VAR ")
                .or_else(|| trimmed.strip_prefix("var "))
            {
                if let Some((k, v)) = rest.split_once('=') {
                    let key = k.trim().to_string();
                    let val = v.trim().trim_matches('"').trim_matches('\'').to_string();
                    initial_vars.insert(key, val);
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

            if let Some(rest) = trimmed
                .strip_prefix('~')
                .or_else(|| trimmed.strip_prefix('$'))
            {
                if let Some((k, v)) = rest.split_once('=') {
                    let key = k.trim().to_string();
                    let val = v.trim().trim_matches('"').trim_matches('\'').to_string();
                    current_vars.push((key, val));
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
            &mut current_vars,
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
            vars: initial_vars.clone(),
            initial_vars,
            page: 0,
            selected_choice: 0,
            turns: 0,
            undo_stack: Vec::new(),
            save_path: None,
            status: "UP/DN select  SELECT choose  BOOT settings".into(),
            in_settings: false,
            settings_cursor: 0,
            choice_scroll_offset: 0,
        };
        game.apply_node_flags(&start_node);
        Ok(game)
    }

    pub fn ensure_choice_visible(&mut self) {
        let total = self.visible_choices().len();
        let window_size = STORY_CHOICES_WINDOW_SIZE;
        if total <= window_size {
            self.choice_scroll_offset = 0;
            return;
        }
        if self.selected_choice < self.choice_scroll_offset {
            self.choice_scroll_offset = self.selected_choice;
        } else if self.selected_choice >= self.choice_scroll_offset + window_size {
            self.choice_scroll_offset = self.selected_choice + 1 - window_size;
        }
        if self.choice_scroll_offset + window_size > total {
            self.choice_scroll_offset = total.saturating_sub(window_size);
        }
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
            for (k, v) in node.set_vars {
                apply_var_assignment(&mut self.vars, &k, &v);
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
                Some((flag, expected)) => {
                    eval_condition(flag, &self.flags, &self.vars) == *expected
                }
            })
            .map(|c| {
                let mut choice = c.clone();
                choice.label = evaluate_passage_text(&c.label, &self.flags, &self.vars);
                choice
            })
            .collect()
    }

    #[must_use]
    pub fn current_body(&self) -> String {
        let raw = self
            .nodes
            .get(&self.current_node)
            .map(|n| n.body.as_str())
            .unwrap_or("(Missing passage)");
        evaluate_passage_text(raw, &self.flags, &self.vars)
    }

    #[must_use]
    pub fn wrapped_pages(&self) -> Vec<Vec<String>> {
        let body = self.current_body();
        let lines = wrap_passage(&body, STORY_CHARS_PER_LINE);
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
        self.vars = self.initial_vars.clone();
        self.page = 0;
        self.selected_choice = 0;
        self.choice_scroll_offset = 0;
        self.turns = 0;
        self.undo_stack.clear();
        self.in_settings = false;
        self.settings_cursor = 0;
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
            vars: self.vars.clone(),
        });
        for (flag, state) in choice.set_flags {
            if state {
                self.flags.insert(flag);
            } else {
                self.flags.remove(&flag);
            }
        }
        for (k, v) in choice.set_vars {
            apply_var_assignment(&mut self.vars, &k, &v);
        }
        self.current_node = choice.target.clone();
        self.apply_node_flags(&choice.target);
        self.page = 0;
        self.selected_choice = 0;
        self.choice_scroll_offset = 0;
        self.turns += 1;
        self.status = if self.is_ending() {
            "THE END  SELECT restart  BOOT settings".into()
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
        self.vars = prev.vars;
        self.page = 0;
        self.selected_choice = 0;
        self.choice_scroll_offset = 0;
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
        for (k, v) in &self.vars {
            out.push_str(&format!("VAR={k}={v}\n"));
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
        let mut vars = self.initial_vars.clone();
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
            } else if let Some(v) = line.strip_prefix("VAR=") {
                if let Some((k, val)) = v.split_once('=') {
                    vars.insert(k.to_string(), val.to_string());
                }
            }
        }
        if let Some(n) = node {
            self.current_node = n;
            self.turns = turns;
            self.flags = flags;
            self.vars = vars;
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
        if self.in_settings {
            let reason = match event {
                ButtonEvent::Up => {
                    self.settings_cursor = self.settings_cursor.saturating_sub(1);
                    "settings-up"
                }
                ButtonEvent::Down => {
                    if self.settings_cursor + 1 < STORY_SETTINGS_COUNT {
                        self.settings_cursor += 1;
                    }
                    "settings-down"
                }
                ButtonEvent::Select => {
                    match StorySettingsOption::from_index(self.settings_cursor) {
                        Some(StorySettingsOption::Resume) => {
                            self.in_settings = false;
                            self.status = format!("Scene: {}", self.current_node);
                            "settings-resume"
                        }
                        Some(StorySettingsOption::PreviousDecision) => {
                            if self.undo() {
                                self.in_settings = false;
                                "settings-undo"
                            } else {
                                self.status = "No previous decision available".into();
                                "settings-undo-empty"
                            }
                        }
                        Some(StorySettingsOption::Restart) => {
                            self.restart();
                            "settings-restart"
                        }
                        Some(StorySettingsOption::QuitStory) => {
                            if self.nodes.contains_key("start") && self.current_node != "start" {
                                self.current_node = "start".into();
                                self.flags.clear();
                                self.page = 0;
                                self.selected_choice = 0;
                                self.choice_scroll_offset = 0;
                                self.turns = 0;
                                self.undo_stack.clear();
                                self.in_settings = false;
                                self.status = "Returned to Story Menu".into();
                                self.persist_save();
                                "settings-quit-catalog"
                            } else {
                                self.restart();
                                self.status = "Story reset to start".into();
                                "settings-quit"
                            }
                        }
                        None => "settings-invalid",
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
            return Ok(Some(self.build_result(reason, dirty_regions)));
        }

        let pages_len = self.wrapped_pages().len().max(1);
        let choices_len = self.visible_choices().len();

        let reason = match event {
            ButtonEvent::Up => {
                if self.page + 1 == pages_len && choices_len > 0 && self.selected_choice > 0 {
                    self.selected_choice -= 1;
                    self.ensure_choice_visible();
                    "choice-up"
                } else if self.page > 0 {
                    self.page -= 1;
                    self.selected_choice = 0;
                    self.choice_scroll_offset = 0;
                    "page-prev"
                } else if choices_len > 0 {
                    self.selected_choice = choices_len - 1;
                    self.ensure_choice_visible();
                    "choice-wrap"
                } else {
                    "top"
                }
            }
            ButtonEvent::Down => {
                if self.page + 1 < pages_len {
                    self.page += 1;
                    self.selected_choice = 0;
                    self.choice_scroll_offset = 0;
                    "page-next"
                } else if choices_len > 0 {
                    self.selected_choice = (self.selected_choice + 1) % choices_len;
                    self.ensure_choice_visible();
                    "choice-down"
                } else {
                    "end"
                }
            }
            ButtonEvent::Select => {
                if self.page + 1 < pages_len {
                    self.page += 1;
                    self.selected_choice = 0;
                    self.choice_scroll_offset = 0;
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
        let reason = if self.in_settings {
            self.in_settings = false;
            self.status = format!("Scene: {}", self.current_node);
            "settings-close"
        } else {
            self.in_settings = true;
            self.settings_cursor = 0;
            self.status = "Story Settings & Stats".into();
            "settings-open"
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
            choice_index: if self.in_settings {
                self.settings_cursor
            } else {
                self.selected_choice
            },
            turns: self.turns,
            ended: !self.in_settings && self.is_ending(),
            dirty_regions,
        }
    }

    fn render_commands(&self, canvas: &mut NativeGameCanvas) -> Result<(), String> {
        canvas.clear_frame();

        if self.in_settings {
            canvas.text(
                20,
                76,
                "STORY SETTINGS & STATS".into(),
                CanvasTextStyle::Heading,
            )?;
            canvas.text(
                20,
                104,
                format!("{}  |  By {}", self.title, self.author),
                CanvasTextStyle::Detail,
            )?;
            canvas.line(20, 114, 460, 114)?;

            // Section 1: Story Stats
            canvas.text(20, 142, "STORY STATS".into(), CanvasTextStyle::Heading)?;
            canvas.text(
                24,
                170,
                format!("Current Passage : {}", self.current_node),
                CanvasTextStyle::Body,
            )?;
            canvas.text(
                24,
                198,
                format!(
                    "Decisions Made  : {} turn(s)  ({} undo steps)",
                    self.turns,
                    self.undo_stack.len()
                ),
                CanvasTextStyle::Body,
            )?;

            let flags_summary = if self.flags.is_empty() {
                "Active Flags    : (none)".to_string()
            } else {
                let list: Vec<String> = self.flags.iter().cloned().collect();
                format!("Active Flags    : {}", list.join(", "))
            };
            let wrapped_flags = wrap_passage(&flags_summary, 38);
            let mut fy = 226;
            for line in wrapped_flags.iter().take(3) {
                canvas.text(24, fy, line.clone(), CanvasTextStyle::Detail)?;
                fy += 22;
            }

            canvas.line(20, 310, 460, 310)?;

            // Section 2: Actions Menu
            canvas.text(20, 338, "ACTIONS".into(), CanvasTextStyle::Heading)?;

            let mut my = 372;
            for idx in 0..STORY_SETTINGS_COUNT {
                let opt = StorySettingsOption::from_index(idx).unwrap();
                let is_sel = idx == self.settings_cursor;
                let prefix = if is_sel { ">" } else { " " };
                let style = if is_sel {
                    CanvasTextStyle::Heading
                } else {
                    CanvasTextStyle::Body
                };

                canvas.text(
                    20,
                    my,
                    format!("{prefix} {}. {}", idx + 1, opt.title()),
                    style,
                )?;
                canvas.text(
                    38,
                    my + 22,
                    format!("   {}", opt.description()),
                    CanvasTextStyle::Detail,
                )?;
                my += 58;
            }

            canvas.line(20, 630, 460, 630)?;
            canvas.text(20, 660, self.status.clone(), CanvasTextStyle::Detail)?;
            canvas.text(
                20,
                706,
                "UP/DN choose   SELECT confirm   BOOT resume".into(),
                CanvasTextStyle::Detail,
            )?;
            return Ok(());
        }

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
                let total_choices = choices.len();
                let window_size = STORY_CHOICES_WINDOW_SIZE;
                let start_idx = self
                    .choice_scroll_offset
                    .min(total_choices.saturating_sub(1));
                let end_idx = (start_idx + window_size).min(total_choices);

                let has_above = start_idx > 0;
                let has_below = end_idx < total_choices;

                if has_above {
                    canvas.text(
                        20,
                        508,
                        format!("▲ [{} option(s) above - UP to scroll]", start_idx),
                        CanvasTextStyle::Detail,
                    )?;
                }

                let mut cy = if has_above { 530 } else { 514 };
                for idx in start_idx..end_idx {
                    let choice = &choices[idx];
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
                    for (line_idx, line) in wrapped.iter().take(2).enumerate() {
                        let text = if line_idx == 0 {
                            line.clone()
                        } else {
                            format!("     {line}")
                        };
                        canvas.text(20, cy, text, style)?;
                        cy += 22;
                    }
                    cy += 6;
                }

                if has_below {
                    let remaining = total_choices - end_idx;
                    canvas.text(
                        20,
                        684,
                        format!(
                            "▼ [Option {}/{} - {} more below - DOWN to scroll]",
                            self.selected_choice + 1,
                            total_choices,
                            remaining
                        ),
                        CanvasTextStyle::Detail,
                    )?;
                } else if has_above {
                    canvas.text(
                        20,
                        684,
                        format!(
                            "▲ [Option {}/{} - End of options]",
                            self.selected_choice + 1,
                            total_choices
                        ),
                        CanvasTextStyle::Detail,
                    )?;
                }
            }
        }

        canvas.text(20, 706, self.status.clone(), CanvasTextStyle::Detail)?;
        let hint = if self.is_ending() {
            "SELECT restart   BOOT settings"
        } else {
            "UP/DN choose   SELECT ok   BOOT settings"
        };
        canvas.text(20, 736, hint.into(), CanvasTextStyle::Detail)?;
        Ok(())
    }
}

fn apply_var_assignment(vars: &mut BTreeMap<String, String>, key: &str, expr: &str) {
    let expr = expr.trim();
    if let Some((left, right)) = expr.split_once('+') {
        let left_val = vars
            .get(left.trim())
            .and_then(|s| s.parse::<i64>().ok())
            .or_else(|| left.trim().parse::<i64>().ok());
        let right_val = vars
            .get(right.trim())
            .and_then(|s| s.parse::<i64>().ok())
            .or_else(|| right.trim().parse::<i64>().ok());
        if let (Some(l), Some(r)) = (left_val, right_val) {
            vars.insert(key.to_string(), (l + r).to_string());
            return;
        }
    } else if let Some((left, right)) = expr.split_once('-') {
        let left_val = vars
            .get(left.trim())
            .and_then(|s| s.parse::<i64>().ok())
            .or_else(|| left.trim().parse::<i64>().ok());
        let right_val = vars
            .get(right.trim())
            .and_then(|s| s.parse::<i64>().ok())
            .or_else(|| right.trim().parse::<i64>().ok());
        if let (Some(l), Some(r)) = (left_val, right_val) {
            vars.insert(key.to_string(), (l - r).to_string());
            return;
        }
    }
    vars.insert(key.to_string(), expr.to_string());
}

pub fn eval_condition(
    cond: &str,
    flags: &BTreeSet<String>,
    vars: &BTreeMap<String, String>,
) -> bool {
    let cond = cond.trim();
    let cond = cond.strip_prefix('?').unwrap_or(cond).trim();

    if let Some((left, right)) = cond.split_once("&&").or_else(|| cond.split_once(" and ")) {
        return eval_condition(left, flags, vars) && eval_condition(right, flags, vars);
    }
    if let Some((left, right)) = cond.split_once("||").or_else(|| cond.split_once(" or ")) {
        return eval_condition(left, flags, vars) || eval_condition(right, flags, vars);
    }

    if let Some((left, right)) = cond.split_once(">=") {
        let l_num = vars
            .get(left.trim())
            .and_then(|s| s.parse::<i64>().ok())
            .or_else(|| left.trim().parse::<i64>().ok());
        let r_num = vars
            .get(right.trim())
            .and_then(|s| s.parse::<i64>().ok())
            .or_else(|| right.trim().trim_matches('"').trim_matches('\'').parse::<i64>().ok());
        if let (Some(l), Some(r)) = (l_num, r_num) {
            return l >= r;
        }
    }
    if let Some((left, right)) = cond.split_once("<=") {
        let l_num = vars
            .get(left.trim())
            .and_then(|s| s.parse::<i64>().ok())
            .or_else(|| left.trim().parse::<i64>().ok());
        let r_num = vars
            .get(right.trim())
            .and_then(|s| s.parse::<i64>().ok())
            .or_else(|| right.trim().trim_matches('"').trim_matches('\'').parse::<i64>().ok());
        if let (Some(l), Some(r)) = (l_num, r_num) {
            return l <= r;
        }
    }
    if let Some((left, right)) = cond.split_once('>') {
        let l_num = vars
            .get(left.trim())
            .and_then(|s| s.parse::<i64>().ok())
            .or_else(|| left.trim().parse::<i64>().ok());
        let r_num = vars
            .get(right.trim())
            .and_then(|s| s.parse::<i64>().ok())
            .or_else(|| right.trim().trim_matches('"').trim_matches('\'').parse::<i64>().ok());
        if let (Some(l), Some(r)) = (l_num, r_num) {
            return l > r;
        }
    }
    if let Some((left, right)) = cond.split_once('<') {
        let l_num = vars
            .get(left.trim())
            .and_then(|s| s.parse::<i64>().ok())
            .or_else(|| left.trim().parse::<i64>().ok());
        let r_num = vars
            .get(right.trim())
            .and_then(|s| s.parse::<i64>().ok())
            .or_else(|| right.trim().trim_matches('"').trim_matches('\'').parse::<i64>().ok());
        if let (Some(l), Some(r)) = (l_num, r_num) {
            return l < r;
        }
    }

    if let Some((left, right)) = cond.split_once("==") {
        let left = left.trim();
        let right = right.trim().trim_matches('"').trim_matches('\'');
        if let Some(val) = vars.get(left) {
            return val == right;
        }
        let combined = format!("{left}_{right}");
        return flags.contains(right)
            || flags.contains(&combined)
            || (right == "alive" && flags.contains("toby_alive"))
            || (right == "dead" && (flags.contains("toby_dead") || !flags.contains("toby_alive")));
    }
    if let Some((left, right)) = cond.split_once("!=") {
        let left = left.trim();
        let right = right.trim().trim_matches('"').trim_matches('\'');
        if let Some(val) = vars.get(left) {
            return val != right;
        }
        let combined = format!("{left}_{right}");
        let eq = flags.contains(right)
            || flags.contains(&combined)
            || (right == "alive" && flags.contains("toby_alive"))
            || (right == "dead" && (flags.contains("toby_dead") || !flags.contains("toby_alive")));
        return !eq;
    }

    if let Some(rest) = cond.strip_prefix('!') {
        return !eval_condition(rest, flags, vars);
    }
    if let Some(rest) = cond.strip_prefix("not ") {
        return !eval_condition(rest, flags, vars);
    }

    if let Some(val) = vars.get(cond) {
        return val != "false" && val != "0" && !val.is_empty();
    }

    flags.contains(cond)
}

pub fn evaluate_passage_text(
    text: &str,
    flags: &BTreeSet<String>,
    vars: &BTreeMap<String, String>,
) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '{' {
            let mut depth = 1usize;
            let mut block = String::new();
            while let Some(c) = chars.next() {
                if c == '{' {
                    depth += 1;
                    block.push(c);
                } else if c == '}' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    block.push(c);
                } else {
                    block.push(c);
                }
            }

            if let Some((cond_part, rest)) = block.split_once(':') {
                let cond_active = eval_condition(cond_part, flags, vars);

                let (true_part, false_part) = if let Some((t, f)) = rest.split_once("\n- else:") {
                    (t, Some(f))
                } else if let Some((t, f)) = rest.split_once("- else:") {
                    (t, Some(f))
                } else if let Some((t, f)) = rest.split_once('|') {
                    (t, Some(f))
                } else {
                    (rest, None)
                };

                let chosen = if cond_active {
                    true_part
                } else {
                    false_part.unwrap_or("")
                };

                let evaluated = evaluate_passage_text(chosen.trim(), flags, vars);
                if !evaluated.is_empty() {
                    if !out.is_empty()
                        && !out.ends_with('\n')
                        && !out.ends_with(' ')
                        && !evaluated.starts_with('\n')
                        && !evaluated.starts_with(' ')
                    {
                        out.push(' ');
                    }
                    out.push_str(&evaluated);
                }
            } else {
                let trimmed = block.trim();
                let (is_cap, var_key) = if let Some(inner) =
                    trimmed.strip_prefix("Cap(").and_then(|s| s.strip_suffix(')'))
                {
                    (true, inner.trim())
                } else {
                    (false, trimmed)
                };

                if let Some(val) = vars.get(var_key) {
                    if is_cap {
                        let mut c_iter = val.chars();
                        if let Some(first) = c_iter.next() {
                            out.extend(first.to_uppercase());
                            out.push_str(c_iter.as_str());
                        }
                    } else {
                        out.push_str(val);
                    }
                } else if flags.contains(trimmed) {
                    out.push_str(trimmed);
                }
            }
        } else {
            out.push(ch);
        }
    }

    out
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
    let prefix_part = rest[..open].trim();
    let after = rest[close + 1..].trim();
    let (meta, target_raw) = after.split_once("->")?;
    let target = target_raw.trim().to_string();
    if label.is_empty() || target.is_empty() {
        return None;
    }

    let mut require_flag = None;
    let mut set_flags = Vec::new();
    let mut set_vars = Vec::new();

    if let Some(c) = prefix_part
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
    {
        let c = c.trim();
        if let Some(f) = c.strip_prefix("not ").or_else(|| c.strip_prefix('!')) {
            require_flag = Some((f.trim().to_string(), false));
        } else {
            require_flag = Some((c.to_string(), true));
        }
    }

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
        } else if let Some(assign) = token.strip_prefix('$').or_else(|| token.strip_prefix('~')) {
            if let Some((k, v)) = assign.split_once('=') {
                set_vars.push((
                    k.trim().to_string(),
                    v.trim().trim_matches('"').trim_matches('\'').to_string(),
                ));
            }
        }
    }

    Some(StoryChoice {
        label,
        target,
        require_flag,
        set_flags,
        set_vars,
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

    let mut hub_header = String::from("TITLE: Story Collection\nAUTHOR: SD Stories\nSTART: start\n");
    let mut hub_menu = String::from("\n== start ==\nSelect a story to play:\n");
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

        hub_menu.push_str(&format!("* [Play: {}] -> {prefix}_start\n", title));

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
                if trimmed.starts_with("VAR ") || trimmed.starts_with("var ") {
                    hub_header.push_str(&format!("{trimmed}\n"));
                }
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

    let mut hub = hub_header;
    hub.push_str(&hub_menu);
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

        // BOOT opens settings menu!
        let boot_res = game.apply_boot_short_press_and_render(&mut canvas).unwrap();
        assert_eq!(boot_res.reason, "settings-open");
        assert!(game.in_settings);
        assert_eq!(game.settings_cursor, 0);

        // Select Previous Decision (Undo, index 1)
        game.settings_cursor = 1;
        let undo_res = game
            .apply_button_and_render(ButtonEvent::Select, &mut canvas)
            .unwrap()
            .unwrap();
        assert_eq!(undo_res.reason, "settings-undo");
        assert!(!game.in_settings);
        assert_eq!(game.current_node, "lantern");
    }

    #[test]
    fn settings_menu_navigation_and_actions() {
        let mut game = StoryGame::new();
        let mut canvas = NativeGameCanvas::default();

        // 1. BOOT opens settings
        game.apply_boot_short_press_and_render(&mut canvas).unwrap();
        assert!(game.in_settings);
        assert_eq!(game.settings_cursor, 0);

        // 2. Down navigates cursor
        game.apply_button_and_render(ButtonEvent::Down, &mut canvas)
            .unwrap();
        assert_eq!(game.settings_cursor, 1);

        // 3. Up navigates back
        game.apply_button_and_render(ButtonEvent::Up, &mut canvas)
            .unwrap();
        assert_eq!(game.settings_cursor, 0);

        // 4. BOOT again closes settings (resumes reading)
        let close_res = game.apply_boot_short_press_and_render(&mut canvas).unwrap();
        assert_eq!(close_res.reason, "settings-close");
        assert!(!game.in_settings);

        // 5. Navigate story: pick lantern
        game.apply_button_and_render(ButtonEvent::Select, &mut canvas)
            .unwrap();
        assert_eq!(game.current_node, "lantern");
        assert_eq!(game.turns, 1);

        // Open settings, select Restart (index 2)
        game.apply_boot_short_press_and_render(&mut canvas).unwrap();
        game.settings_cursor = 2; // Restart
        game.apply_button_and_render(ButtonEvent::Select, &mut canvas)
            .unwrap();
        assert!(!game.in_settings);
        assert_eq!(game.current_node, "start");
        assert_eq!(game.turns, 0);
        assert!(game.flags.is_empty());
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

        // Open settings and pick "Quit to Story Menu" (index 3)
        game.apply_boot_short_press_and_render(&mut canvas).unwrap();
        assert!(game.in_settings);
        game.settings_cursor = 3;
        let quit_res = game
            .apply_button_and_render(ButtonEvent::Select, &mut canvas)
            .unwrap()
            .unwrap();
        assert_eq!(quit_res.reason, "settings-quit-catalog");
        assert!(!game.in_settings);
        // Returns to the hub catalog start node!
        assert_eq!(game.current_node, "start");
        assert_eq!(game.visible_choices().len(), 2);

        std::fs::remove_dir_all(app_dir).unwrap();
    }

    #[test]
    fn evaluates_conditional_passage_text_and_choices() {
        let story_text = r#"TITLE: Conditional Test
== start ==
Before tunnel.

{ toby_status == "alive":
Ahead of you, Jamie and Julian take turns supporting young Toby on his splinted ankle.
- else:
The silence where young Toby's voice should be hangs over the tunnel like lead.
}

After tunnel. Jamie helps { toby_status == "alive": Toby | Julian }.
* { toby_status == "alive" } [Thank Toby] -> end
* { toby_status == "dead" } [Mourn Toby] -> end
== end ==
Done.
"#;
        let mut game = StoryGame::from_story_text(story_text).unwrap();

        // 1. By default, toby_alive is false -> renders else branch!
        let body_dead = game.current_body();
        assert!(body_dead.contains("The silence where young Toby's voice should be"));
        assert!(body_dead.contains("Jamie helps Julian"));
        let choices_dead = game.visible_choices();
        assert_eq!(choices_dead.len(), 1);
        assert_eq!(choices_dead[0].label, "Mourn Toby");

        // 2. Set toby_alive flag -> dynamically re-evaluates to true branch!
        game.flags.insert("toby_alive".into());
        let body_alive = game.current_body();
        assert!(body_alive.contains("Jamie and Julian take turns supporting young Toby"));
        assert!(body_alive.contains("Jamie helps Toby"));
        let choices_alive = game.visible_choices();
        assert_eq!(choices_alive.len(), 1);
        assert_eq!(choices_alive[0].label, "Thank Toby");
    }

    #[test]
    fn evaluates_user_exact_toby_status_snippet() {
        let snippet = r#"TITLE: User Snippet
== start ==
Dust hangs thick in the pitch-black storm drain.

{ toby_status == "alive":
    Ahead of you, Jamie and Julian take turns supporting young Toby on his splinted ankle, the nineteen-year-old whispering quiet thank-yous in the dark.
- else:
    The silence where young Toby's voice should be hangs over the tunnel like lead; Jamie walks ten paces ahead in the dark, shoulders rigid, refusing to look back at you.
}

Suddenly, your body cashes the check your adrenaline wrote.
* [Continue] -> end
== end ==
Done.
"#;
        let mut game = StoryGame::from_story_text(snippet).unwrap();

        // 1. Default (Toby dead)
        let body_dead = game.current_body();
        assert!(body_dead.contains("The silence where young Toby's voice should be"));
        assert!(!body_dead.contains("Ahead of you, Jamie and Julian"));
        assert!(!body_dead.contains('{'));
        assert!(!body_dead.contains('}'));
        assert!(!body_dead.contains("- else:"));

        // 2. Toby alive
        game.flags.insert("toby_alive".into());
        let body_alive = game.current_body();
        assert!(
            body_alive.contains("Ahead of you, Jamie and Julian take turns supporting young Toby")
        );
        assert!(!body_alive.contains("The silence where young Toby's voice should be"));
        assert!(!body_alive.contains('{'));
        assert!(!body_alive.contains('}'));
        assert!(!body_alive.contains("- else:"));
    }

    #[test]
    fn choice_window_scrolls_down_and_up_beyond_capacity() {
        let story_text = r#"TITLE: Many Choices Test
== start ==
You stand at a crossroads with many paths.
* [Go North into the dark forest] -> north
* [Go East toward the windy coast] -> east
* [Go South into the desert ruins] -> south
* [Go West toward the snowy mountains] -> west
* [Descend into the underground catacombs] -> down
* [Climb the ancient ruined watchtower] -> up
== north ==
North.
== east ==
East.
== south ==
South.
== west ==
West.
== down ==
Down.
== up ==
Up.
"#;
        let mut game = StoryGame::from_story_text(story_text).unwrap();
        assert_eq!(game.visible_choices().len(), 6);
        assert_eq!(game.selected_choice, 0);
        assert_eq!(game.choice_scroll_offset, 0);

        let mut canvas = NativeGameCanvas::default();
        game.render_initial(&mut canvas).unwrap();

        // Down to index 1: window stays at 0
        game.apply_button_and_render(ButtonEvent::Down, &mut canvas)
            .unwrap();
        assert_eq!(game.selected_choice, 1);
        assert_eq!(game.choice_scroll_offset, 0);

        // Down to index 2: window stays at 0
        game.apply_button_and_render(ButtonEvent::Down, &mut canvas)
            .unwrap();
        assert_eq!(game.selected_choice, 2);
        assert_eq!(game.choice_scroll_offset, 0);

        // Down to index 3: window shifts to offset 1
        game.apply_button_and_render(ButtonEvent::Down, &mut canvas)
            .unwrap();
        assert_eq!(game.selected_choice, 3);
        assert_eq!(game.choice_scroll_offset, 1);

        // Down to index 4: window shifts to offset 2
        game.apply_button_and_render(ButtonEvent::Down, &mut canvas)
            .unwrap();
        assert_eq!(game.selected_choice, 4);
        assert_eq!(game.choice_scroll_offset, 2);

        // Down to index 5: window shifts to offset 3
        game.apply_button_and_render(ButtonEvent::Down, &mut canvas)
            .unwrap();
        assert_eq!(game.selected_choice, 5);
        assert_eq!(game.choice_scroll_offset, 3);

        // Up to index 4: window stays at offset 3 (choice 4 is visible in [3, 4, 5])
        game.apply_button_and_render(ButtonEvent::Up, &mut canvas)
            .unwrap();
        assert_eq!(game.selected_choice, 4);
        assert_eq!(game.choice_scroll_offset, 3);

        // Up to index 3: window stays at offset 3 (choice 3 is visible in [3, 4, 5])
        game.apply_button_and_render(ButtonEvent::Up, &mut canvas)
            .unwrap();
        assert_eq!(game.selected_choice, 3);
        assert_eq!(game.choice_scroll_offset, 3);

        // Up to index 2: window shifts to offset 2
        game.apply_button_and_render(ButtonEvent::Up, &mut canvas)
            .unwrap();
        assert_eq!(game.selected_choice, 2);
        assert_eq!(game.choice_scroll_offset, 2);

        // Select on choice 2 (South): navigates and resets offset to 0
        game.apply_button_and_render(ButtonEvent::Select, &mut canvas)
            .unwrap();
        assert_eq!(game.current_node, "south");
        assert_eq!(game.selected_choice, 0);
        assert_eq!(game.choice_scroll_offset, 0);
    }

    #[test]
    fn parses_and_verifies_dead_weight_story_chapters_1_and_2() {
        let story_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples")
            .join("sd-card")
            .join("RUSTMIX")
            .join("APPS")
            .join("STORY")
            .join("STORIES")
            .join("DEADWGT.TXT");
        let content = std::fs::read_to_string(&story_path)
            .expect("examples/sd-card/RUSTMIX/APPS/STORY/STORIES/DEADWGT.TXT must exist");
        let game = StoryGame::from_story_text(&content)
            .expect("DEADWGT.TXT must parse cleanly without syntax errors");

        assert_eq!(game.start_node, "start");
        assert!(game.nodes.contains_key("start"));
        assert!(game.nodes.contains_key("mirror_customization"));
        assert!(game.nodes.contains_key("chapter_one_start"));
        assert!(game.nodes.contains_key("bullpen_crisis"));
        assert!(game.nodes.contains_key("storm_drain_march"));
        assert!(game.nodes.contains_key("chapter_two_start"));
        assert!(game.nodes.contains_key("ch2_ending_summary"));

        // Verify every choice target in every node resolves to a valid node
        for (node_id, node) in &game.nodes {
            for choice in &node.choices {
                assert!(
                    game.nodes.contains_key(&choice.target),
                    "Node '{node_id}' has choice pointing to non-existent node '{}'",
                    choice.target
                );
            }
        }

        // Simulate Playthrough 1: Save Toby -> Caught by Harry -> Stitch Toby -> Overpass Route
        let mut sim = game.clone();
        assert_eq!(sim.current_node, "start");
        sim.selected_choice = 0; // mirror
        sim.choose_selected();
        sim.selected_choice = 0; // female
        sim.choose_selected();
        sim.selected_choice = 0; // continue to name
        sim.choose_selected();
        sim.selected_choice = 0; // Dr. Alex Vance -> chapter_one_start
        sim.choose_selected();
        assert_eq!(sim.current_node, "chapter_one_start");
        sim.selected_choice = 2; // examine Maya Lin chart (+saved_chart)
        sim.choose_selected();
        assert_eq!(sim.current_node, "chapter_one_start_c3");
        assert!(sim.flags.contains("saved_chart"));
        sim.selected_choice = 0; // continue -> locker_knock
        sim.choose_selected();
        assert_eq!(sim.current_node, "locker_knock");
        sim.selected_choice = 0; // -> locker_door_choice
        sim.choose_selected();
        sim.selected_choice = 0; // unlock -> julian_entrance
        sim.choose_selected();
        sim.selected_choice = 1; // touch watch clasp -> julian_entrance_c2
        sim.choose_selected();
        sim.selected_choice = 0; // -> hallway_ro3_intro
        sim.choose_selected();
        sim.selected_choice = 0; // -> hallway_ro2_intro
        sim.choose_selected();
        sim.selected_choice = 0; // -> hallway_ro4_intro
        sim.choose_selected();
        sim.selected_choice = 0; // -> hallway_prep_hub
        sim.choose_selected();
        sim.selected_choice = 1; // prep with Harry -> prep_with_harry
        sim.choose_selected();
        sim.selected_choice = 0; // -> bullpen_crisis
        sim.choose_selected();
        assert_eq!(sim.current_node, "bullpen_crisis");
        sim.selected_choice = 0; // Save Toby -> branch_save_toby
        sim.choose_selected();
        assert!(sim.flags.contains("toby_alive"));
        sim.selected_choice = 0; // -> storm_drain_march
        sim.choose_selected();
        assert_eq!(sim.current_node, "storm_drain_march");
        let march_body = sim.current_body();
        assert!(march_body.contains("whispering quiet thank-yous"));
        sim.selected_choice = 0; // Deterministic calculation: Harry catches you (single auto-choice) -> tunnel_caught_by_harry
        sim.choose_selected();
        assert_eq!(sim.current_node, "tunnel_caught_by_harry");
        sim.selected_choice = 1; // squeeze hand -> tunnel_caught_by_harry_c2
        sim.choose_selected();
        assert_eq!(sim.current_node, "tunnel_caught_by_harry_c2");
        sim.selected_choice = 0; // -> chapter_one_ending
        sim.choose_selected();
        assert_eq!(sim.current_node, "chapter_one_ending");
        sim.selected_choice = 0; // -> chapter_two_start
        sim.choose_selected();
        assert_eq!(sim.current_node, "chapter_two_start");
        let ch2_start_body = sim.current_body();
        assert!(ch2_start_body.contains("severe compound ankle sprain"));
        sim.selected_choice = 0; // stitch Toby -> ch2_triage_toby
        sim.choose_selected();
        sim.selected_choice = 0; // -> ch2_watch_intro
        sim.choose_selected();
        sim.selected_choice = 0; // -> ch2_talk_hub
        sim.choose_selected();
        assert_eq!(sim.current_node, "ch2_talk_hub");
        sim.selected_choice = 2; // Harry -> talk_ro3_scene
        sim.choose_selected();
        assert!(sim.flags.contains("talked_ro3_ch2"));
        sim.selected_choice = 0; // kiss Harry -> talk_ro3_kiss
        sim.choose_selected();
        sim.selected_choice = 0; // -> ch2_talk_hub
        sim.choose_selected();
        assert_eq!(sim.current_node, "ch2_talk_hub");
        let hub2_choices = sim.visible_choices();
        assert!(!hub2_choices.iter().any(|c| c.label.contains("Harry"))); // Harry already spoken to!
        sim.selected_choice = 0; // Julian -> talk_ro1_scene
        sim.choose_selected();
        sim.selected_choice = 0; // kiss Julian -> talk_ro1_kiss
        sim.choose_selected();
        sim.selected_choice = 0; // -> ch2_talk_hub
        sim.choose_selected();
        assert_eq!(sim.current_node, "ch2_talk_hub");
        let hub3_choices = sim.visible_choices();
        assert_eq!(
            hub3_choices.last().unwrap().target,
            "ch2_dawn_breach"
        );
        sim.selected_choice = hub3_choices.len() - 1; // Proceed to Dawn
        sim.choose_selected();
        assert_eq!(sim.current_node, "ch2_dawn_breach");
        let breach_body = sim.current_body();
        assert!(breach_body.contains("haul Toby out"));
        sim.selected_choice = 0; // fire revolver -> ch2_dawn_breach_c1
        sim.choose_selected();
        sim.selected_choice = 0; // -> ch2_route_decision
        sim.choose_selected();
        assert_eq!(sim.current_node, "ch2_route_decision");
        sim.selected_choice = 0; // North Overpass -> ch2_ending_summary
        sim.choose_selected();
        assert_eq!(sim.current_node, "ch2_ending_summary");
        let summary = sim.current_body();
        assert!(summary.contains("PROTAGONIST"));
        assert!(summary.contains("Route Chosen: overpass"));

        // Play into Chapter 3
        sim.selected_choice = 0; // -> chapter_three_start
        sim.choose_selected();
        assert_eq!(sim.current_node, "chapter_three_start");
        sim.selected_choice = 0; // Overpass route -> ch3_overpass_route
        sim.choose_selected();
        assert_eq!(sim.current_node, "ch3_overpass_route");
        sim.selected_choice = 0; // save Paige -> overpass_save_paige
        sim.choose_selected();
        assert_eq!(sim.current_node, "overpass_save_paige");
        assert!(sim.flags.contains("saved_overpass_bus"));
        assert!(sim.flags.contains("clara_ferry_clue"));
        sim.selected_choice = 0; // -> check_ch3_mid_suspicion
        sim.choose_selected();
        assert_eq!(sim.current_node, "check_ch3_mid_suspicion");
        sim.selected_choice = 0; // Julian confronts you in culvert -> ch3_suspicion_ro1
        sim.choose_selected();
        assert_eq!(sim.current_node, "ch3_suspicion_ro1");
        assert!(sim.flags.contains("ro1_knows_bite"));
        sim.selected_choice = 0; // -> railyard_convergence
        sim.choose_selected();
        assert_eq!(sim.current_node, "railyard_convergence");
        sim.selected_choice = 0; // rescue workers -> railyard_flare_climax
        sim.choose_selected();
        assert_eq!(sim.current_node, "railyard_flare_climax");
        sim.selected_choice = 0; // climb into boxcar
        sim.choose_selected();
        assert!(sim.current_node.starts_with("boxcar_scene"));
        sim.selected_choice = 0; // -> ch3_ending_summary
        sim.choose_selected();
        assert_eq!(sim.current_node, "ch3_ending_summary");
        let ch3_summary = sim.current_body();
        assert!(ch3_summary.contains("END OF CHAPTER 3"));

        // Advance into Chapter 4: The Anatomy of a Lie
        sim.selected_choice = 0; // -> chapter_four_start
        sim.choose_selected();
        assert_eq!(sim.current_node, "chapter_four_start");
        let ch4_body = sim.current_body();
        assert!(ch4_body.contains("CHAPTER 4"));
    }

    #[test]
    fn sd_story_app_loads_hub_with_deadwgt_and_beacon() {
        let story_app_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples")
            .join("sd-card")
            .join("RUSTMIX")
            .join("APPS")
            .join("STORY");
        let save_file = story_app_dir.join("SAVE.TXT");
        let _ = std::fs::remove_file(&save_file);
        struct SaveCleanup(std::path::PathBuf);
        impl Drop for SaveCleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        let _guard = SaveCleanup(save_file);

        let mut game = StoryGame::from_app_dir("default", Some(&story_app_dir)).unwrap();
        assert_eq!(game.current_node, "start");
        let choices = game.visible_choices();
        assert_eq!(choices.len(), 2);
        assert_eq!(choices[0].label, "Play: The Saltmire Beacon");
        assert_eq!(
            choices[1].label,
            "Play: Dead Weight: Book I & II (Chapters 1 - 9 Complete)"
        );

        // Pick Dead Weight
        game.selected_choice = 1;
        assert!(game.choose_selected());
        assert_eq!(game.current_node, "s2_start");
        let body = game.current_body();
        assert!(body.contains("DEAD WEIGHT: BOOK ONE - THE TRIAGE OF ASH"));
        let sub_choices = game.visible_choices();
        assert_eq!(sub_choices.len(), 1);
        assert_eq!(
            sub_choices[0].label,
            "Step up to the locker room mirror (Begin Chapter 1)"
        );
    }

    #[test]
    fn dead_weight_dynamic_character_variables_and_locker_voice_interpolation() {
        let deadwgt_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples")
            .join("sd-card")
            .join("RUSTMIX")
            .join("APPS")
            .join("STORY")
            .join("STORIES")
            .join("DEADWGT.TXT");
        let content = std::fs::read_to_string(deadwgt_path).unwrap();
        let mut sim = StoryGame::from_story_text(&content).unwrap();
        assert_eq!(sim.current_node, "start");

        // start -> mirror_customization
        sim.selected_choice = 0;
        assert!(sim.choose_selected());
        assert_eq!(sim.current_node, "mirror_customization");

        // Choose Female MC (Elena / She / Her)
        sim.selected_choice = 0;
        assert!(sim.choose_selected());
        assert_eq!(sim.current_node, "set_mc_female");
        assert_eq!(sim.vars.get("mc_name").map(|s| s.as_str()), Some("Elena"));
        assert_eq!(sim.vars.get("mc_subj").map(|s| s.as_str()), Some("she"));

        // Continue -> select_mc_name
        sim.selected_choice = 0;
        assert!(sim.choose_selected());
        assert_eq!(sim.current_node, "select_mc_name");

        // Keep Dr. Elena Vance -> chapter_one_start
        sim.selected_choice = 0;
        assert!(sim.choose_selected());
        assert_eq!(sim.current_node, "chapter_one_start");

        // Bind arm with Coban wrap -> chapter_one_start_c1
        sim.selected_choice = 0;
        assert!(sim.choose_selected());
        assert_eq!(sim.current_node, "chapter_one_start_c1");

        // Continue -> locker_knock
        sim.selected_choice = 0;
        assert!(sim.choose_selected());
        assert_eq!(sim.current_node, "locker_knock");

        // Pick Juliet (index 1) -> locker_door_choice
        sim.selected_choice = 1;
        assert!(sim.choose_selected());
        assert_eq!(sim.current_node, "locker_door_choice");
        assert_eq!(sim.vars.get("ro1_name").map(|s| s.as_str()), Some("Juliet"));
        assert_eq!(sim.vars.get("ro1_subj").map(|s| s.as_str()), Some("she"));

        // Verify "{mc_name}?" {ro1_name}'s voice...
        let body = sim.current_body();
        assert!(
            body.contains("\"Elena?\" Juliet's voice filters through the steel door"),
            "Expected interpolated quote, got:\n{}",
            body
        );

        // Verify choices interpolate {ro1_name}
        let choices = sim.visible_choices();
        assert_eq!(
            choices[0].label,
            "Unlock the door immediately and let Juliet in"
        );
        assert_eq!(
            choices[2].label,
            "\"Are you alone, Juliet? Who else is out in that corridor?\""
        );

        // Test undo restores previous node & variable state
        assert!(sim.undo());
        assert_eq!(sim.current_node, "locker_knock");

        // Now pick Julian (index 0)
        sim.selected_choice = 0;
        assert!(sim.choose_selected());
        assert_eq!(sim.current_node, "locker_door_choice");
        assert_eq!(sim.vars.get("ro1_name").map(|s| s.as_str()), Some("Julian"));
        assert_eq!(sim.vars.get("ro1_subj").map(|s| s.as_str()), Some("he"));

        let julian_body = sim.current_body();
        assert!(
            julian_body.contains("\"Elena?\" Julian's voice filters through the steel door"),
            "Expected Julian interpolated quote, got:\n{}",
            julian_body
        );
        let julian_choices = sim.visible_choices();
        assert_eq!(
            julian_choices[0].label,
            "Unlock the door immediately and let Julian in"
        );
    }

    #[test]
    fn dead_weight_renders_every_node_without_errors() {
        let deadwgt_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples")
            .join("sd-card")
            .join("RUSTMIX")
            .join("APPS")
            .join("STORY")
            .join("STORIES")
            .join("DEADWGT.TXT");
        let content = std::fs::read_to_string(deadwgt_path).unwrap();
        let mut game = StoryGame::from_story_text(&content).unwrap();

        // 1. Verify start node initial render
        let mut canvas = NativeGameCanvas::default();
        game.render_initial(&mut canvas).expect("start render failed");
        assert!(!canvas.commands().is_empty());

        // 2. Iterate through EVERY node in node_order and render
        let nodes = game.node_order.clone();
        assert!(nodes.len() >= 50, "Expected at least 50 nodes, got {}", nodes.len());

        for node_id in &nodes {
            game.current_node = node_id.clone();
            game.page = 0;
            game.selected_choice = 0;
            game.choice_scroll_offset = 0;

            let mut c = NativeGameCanvas::default();
            game.render_initial(&mut c)
                .unwrap_or_else(|e| panic!("Failed to render node {}: {}", node_id, e));
            assert!(
                !c.commands().is_empty(),
                "Node {} produced zero draw commands",
                node_id
            );

            // If node has multi-page text, render each page
            let num_pages = game.wrapped_pages().len();
            for p in 1..num_pages {
                game.page = p;
                let mut page_canvas = NativeGameCanvas::default();
                game.render_initial(&mut page_canvas)
                    .unwrap_or_else(|e| panic!("Failed to render node {} page {}: {}", node_id, p, e));
            }

            // If node has choices, test Up/Down navigation render
            let choices_count = game.visible_choices().len();
            if choices_count > 1 {
                let mut nav_canvas = NativeGameCanvas::default();
                game.apply_button_and_render(ButtonEvent::Down, &mut nav_canvas)
                    .unwrap_or_else(|e| panic!("Failed Down navigation on node {}: {}", node_id, e));
                game.apply_button_and_render(ButtonEvent::Up, &mut nav_canvas)
                    .unwrap_or_else(|e| panic!("Failed Up navigation on node {}: {}", node_id, e));
            }
        }

        // 3. Test settings screen rendering inside dead weight
        let mut settings_canvas = NativeGameCanvas::default();
        game.apply_boot_short_press_and_render(&mut settings_canvas)
            .expect("Settings menu render failed");
        assert!(!settings_canvas.commands().is_empty());
    }
}
