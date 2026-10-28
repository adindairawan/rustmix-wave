//! Native Sokoban Tilt campaign & level selector for debounced portrait tilt events.
//!
//! All levels live on the SD card in `MAIN.LUA` separated by `|`.
//! Rust owns parsing, player and crate state, push validation, goal detection,
//! level progression, level selection menu, rendering and dirty metadata.

use super::{
    canvas::{CanvasTextStyle, NativeGameCanvas},
    dirty_regions::DirtyRect,
};
use crate::{
    buttons::ButtonEvent,
    imu_events::{ImuDetectedEvent, ImuDetectedEventKind, MotionAxis},
};

pub const SOKOBAN_COLUMNS: usize = 9;
pub const SOKOBAN_ROWS: usize = 9;
pub const SOKOBAN_CELL_COUNT: usize = SOKOBAN_COLUMNS * SOKOBAN_ROWS;
pub const SOKOBAN_GRID_X: i32 = 51;
pub const SOKOBAN_GRID_Y: i32 = 176;
pub const SOKOBAN_CELL_SIZE: i32 = 42;
const SOKOBAN_HEADER_RECT: DirtyRect = DirtyRect::new(16, 48, 448, 96);
const SOKOBAN_STATUS_RECT: DirtyRect = DirtyRect::new(16, 606, 448, 142);
const SOKOBAN_BOARD_RECT: DirtyRect = DirtyRect::new(
    SOKOBAN_GRID_X,
    SOKOBAN_GRID_Y,
    SOKOBAN_COLUMNS as i32 * SOKOBAN_CELL_SIZE + 1,
    SOKOBAN_ROWS as i32 * SOKOBAN_CELL_SIZE + 1,
);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ParsedLevel {
    walls: [bool; SOKOBAN_CELL_COUNT],
    goals: [bool; SOKOBAN_CELL_COUNT],
    initial_crates: [bool; SOKOBAN_CELL_COUNT],
    start: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SokobanDirection {
    Up,
    Down,
    Left,
    Right,
}
impl SokobanDirection {
    #[must_use]
    pub const fn from_raw_axis(axis: MotionAxis) -> Option<Self> {
        match axis {
            MotionAxis::PositiveX => Some(Self::Down),
            MotionAxis::NegativeX => Some(Self::Up),
            MotionAxis::PositiveY => Some(Self::Left),
            MotionAxis::NegativeY => Some(Self::Right),
            MotionAxis::PositiveZ | MotionAxis::NegativeZ => None,
        }
    }
    #[must_use]
    pub const fn marker(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
            Self::Left => "left",
            Self::Right => "right",
        }
    }
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Up => "UP",
            Self::Down => "DOWN",
            Self::Left => "LEFT",
            Self::Right => "RIGHT",
        }
    }
    #[must_use]
    pub const fn delta(self) -> (i32, i32) {
        match self {
            Self::Up => (-1, 0),
            Self::Down => (1, 0),
            Self::Left => (0, -1),
            Self::Right => (0, 1),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SokobanOutcome {
    InProgress,
    Won,
}
impl SokobanOutcome {
    #[must_use]
    pub const fn marker(self) -> &'static str {
        match self {
            Self::InProgress => "in-progress",
            Self::Won => "won",
        }
    }
    #[must_use]
    pub const fn completed(self) -> bool {
        matches!(self, Self::Won)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SokobanTiltEventResult {
    pub reason: &'static str,
    pub row: usize,
    pub column: usize,
    pub moves: u32,
    pub pushes: u32,
    pub crates_on_goal: usize,
    pub outcome: SokobanOutcome,
    pub direction: &'static str,
    pub raw_axis: &'static str,
    pub dirty_regions: Vec<DirtyRect>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SokobanTiltGame {
    levels: Vec<ParsedLevel>,
    solved_levels: Vec<bool>,
    current_level: usize,
    menu_open: bool,
    menu_selected: usize,
    walls: [bool; SOKOBAN_CELL_COUNT],
    goals: [bool; SOKOBAN_CELL_COUNT],
    initial_crates: [bool; SOKOBAN_CELL_COUNT],
    crates: [bool; SOKOBAN_CELL_COUNT],
    start: usize,
    player: usize,
    moves: u32,
    pushes: u32,
    status: String,
    outcome: SokobanOutcome,
}

impl SokobanTiltGame {
    fn parse_single_map(encoded: &str) -> Result<ParsedLevel, String> {
        let rows = encoded.trim().split('/').collect::<Vec<_>>();
        if rows.len() != SOKOBAN_ROWS
            || rows
                .iter()
                .any(|row| row.chars().count() != SOKOBAN_COLUMNS)
        {
            return Err("sokoban v0.18.6 expects 9x9 slash-separated maps".into());
        }
        let mut walls = [false; SOKOBAN_CELL_COUNT];
        let mut goals = [false; SOKOBAN_CELL_COUNT];
        let mut crates = [false; SOKOBAN_CELL_COUNT];
        let mut start = None;
        for (row, text) in rows.iter().enumerate() {
            for (column, ch) in text.chars().enumerate() {
                let index = row * SOKOBAN_COLUMNS + column;
                match ch {
                    '#' => walls[index] = true,
                    '.' => {}
                    'S' if start.is_none() => start = Some(index),
                    'S' => return Err("sokoban map contains multiple starts".into()),
                    'C' => crates[index] = true,
                    'G' => goals[index] = true,
                    '*' => {
                        crates[index] = true;
                        goals[index] = true;
                    }
                    _ => return Err(format!("sokoban map contains unsupported cell {ch:?}")),
                }
            }
        }
        let start = start.ok_or_else(|| "sokoban map is missing S".to_string())?;
        let crate_count = crates.iter().filter(|v| **v).count();
        let goal_count = goals.iter().filter(|v| **v).count();
        if crate_count == 0 || crate_count != goal_count {
            return Err("sokoban requires the same non-zero number of crates and goals".into());
        }
        if walls[start] || crates[start] {
            return Err("sokoban start cell is blocked".into());
        }
        Ok(ParsedLevel {
            walls,
            goals,
            initial_crates: crates,
            start,
        })
    }

    /// Parses one or more 9x9 maps separated by `|` purely from the SD card string.
    pub fn from_map(encoded: &str) -> Result<Self, String> {
        let mut levels = Vec::new();
        for part in encoded.split('|') {
            let trimmed = part.trim();
            if !trimmed.is_empty() {
                levels.push(Self::parse_single_map(trimmed)?);
            }
        }
        if levels.is_empty() {
            return Err("sokoban requires at least one 9x9 map in MAIN.LUA".into());
        }
        let first = levels[0];
        let total = levels.len();
        Ok(Self {
            levels,
            solved_levels: vec![false; total],
            current_level: 0,
            menu_open: false,
            menu_selected: 0,
            walls: first.walls,
            goals: first.goals,
            initial_crates: first.initial_crates,
            crates: first.initial_crates,
            start: first.start,
            player: first.start,
            moves: 0,
            pushes: 0,
            status: "Tilt to move  Push crates onto G".into(),
            outcome: SokobanOutcome::InProgress,
        })
    }

    fn load_level(&mut self, index: usize, status_msg: String) {
        let index = index % self.levels.len();
        let lvl = self.levels[index];
        self.current_level = index;
        self.menu_selected = index;
        self.menu_open = false;
        self.walls = lvl.walls;
        self.goals = lvl.goals;
        self.initial_crates = lvl.initial_crates;
        self.crates = lvl.initial_crates;
        self.start = lvl.start;
        self.player = lvl.start;
        self.moves = 0;
        self.pushes = 0;
        self.outcome = SokobanOutcome::InProgress;
        self.status = status_msg;
    }

    #[must_use]
    pub const fn player_row(&self) -> usize {
        self.player / SOKOBAN_COLUMNS
    }
    #[must_use]
    pub const fn player_column(&self) -> usize {
        self.player % SOKOBAN_COLUMNS
    }
    #[must_use]
    pub const fn moves(&self) -> u32 {
        self.moves
    }
    #[must_use]
    pub const fn pushes(&self) -> u32 {
        self.pushes
    }
    #[must_use]
    pub const fn outcome(&self) -> SokobanOutcome {
        self.outcome
    }
    #[must_use]
    pub fn crates_on_goal(&self) -> usize {
        self.crates
            .iter()
            .zip(self.goals.iter())
            .filter(|(crate_cell, goal)| **crate_cell && **goal)
            .count()
    }

    pub fn render_initial(&self, canvas: &mut NativeGameCanvas) -> Result<(), String> {
        self.render_commands(canvas)?;
        canvas.reset_dirty_regions();
        canvas.invalidate_rect(DirtyRect::new(0, 0, 480, 800));
        canvas.request_refresh();
        Ok(())
    }

    pub fn apply_motion_and_render(
        &mut self,
        event: ImuDetectedEvent,
        canvas: &mut NativeGameCanvas,
    ) -> Result<Option<SokobanTiltEventResult>, String> {
        if self.menu_open {
            return Ok(None);
        }
        let ImuDetectedEventKind::Tilt(axis) = event.kind else {
            return Ok(None);
        };
        let Some(direction) = SokobanDirection::from_raw_axis(axis) else {
            return Ok(None);
        };
        let (row_delta, column_delta) = direction.delta();
        let old_player = self.player;
        let mut pushed = false;
        let reason = if self.outcome.completed() {
            if self.current_level + 1 < self.levels.len() {
                self.status = "Solved! Press SELECT for next level".into();
            } else {
                self.status = "Puzzle solved  SELECT reset/menu".into();
            }
            "game-finished"
        } else if let Some(next) = shifted_cell(self.player, row_delta, column_delta) {
            if self.walls[next] {
                self.status = format!("{} blocked by wall", direction.label());
                "wall-blocked"
            } else if self.crates[next] {
                if let Some(crate_next) = shifted_cell(next, row_delta, column_delta) {
                    if !self.walls[crate_next] && !self.crates[crate_next] {
                        self.crates[next] = false;
                        self.crates[crate_next] = true;
                        self.player = next;
                        self.moves = self.moves.saturating_add(1);
                        self.pushes = self.pushes.saturating_add(1);
                        pushed = true;
                        if self.crates_on_goal() == self.goals.iter().filter(|v| **v).count() {
                            self.outcome = SokobanOutcome::Won;
                            self.solved_levels[self.current_level] = true;
                            if self.current_level + 1 < self.levels.len() {
                                self.status = format!(
                                    "Lv{} Solved ({}m)! SELECT -> Lv{}",
                                    self.current_level + 1,
                                    self.moves,
                                    self.current_level + 2
                                );
                            } else {
                                self.status = format!(
                                    "Solved in {} moves  SELECT menu",
                                    self.moves
                                );
                            }
                            "goal-reached"
                        } else {
                            self.status = format!("{} push  Keep solving", direction.label());
                            "crate-push"
                        }
                    } else {
                        self.status = format!("{} crate blocked", direction.label());
                        "crate-blocked"
                    }
                } else {
                    self.status = format!("{} crate blocked", direction.label());
                    "crate-blocked"
                }
            } else {
                self.player = next;
                self.moves = self.moves.saturating_add(1);
                self.status = format!("{} moved  Push C onto G", direction.label());
                "tilt-move"
            }
        } else {
            self.status = format!("{} edge blocked", direction.label());
            "edge-blocked"
        };
        self.render_commands(canvas)?;
        canvas.reset_dirty_regions();
        let dirty_regions = if pushed || reason == "goal-reached" {
            vec![SOKOBAN_BOARD_RECT, SOKOBAN_STATUS_RECT]
        } else if old_player == self.player {
            vec![SOKOBAN_STATUS_RECT]
        } else {
            vec![
                cell_rect(old_player),
                cell_rect(self.player),
                SOKOBAN_STATUS_RECT,
            ]
        };
        for rect in &dirty_regions {
            canvas.invalidate_rect(*rect);
        }
        canvas.request_refresh();
        Ok(Some(self.result(
            reason,
            direction.marker(),
            axis.marker(),
            dirty_regions,
        )))
    }

    pub fn apply_button_and_render(
        &mut self,
        event: ButtonEvent,
        canvas: &mut NativeGameCanvas,
    ) -> Result<Option<SokobanTiltEventResult>, String> {
        let reason = if self.menu_open {
            match event {
                ButtonEvent::Up => {
                    self.menu_selected = self
                        .menu_selected
                        .checked_sub(1)
                        .unwrap_or(self.levels.len() - 1);
                    self.status = format!(
                        "Select Level {} of {}",
                        self.menu_selected + 1,
                        self.levels.len()
                    );
                    "menu-up"
                }
                ButtonEvent::Down => {
                    self.menu_selected = (self.menu_selected + 1) % self.levels.len();
                    self.status = format!(
                        "Select Level {} of {}",
                        self.menu_selected + 1,
                        self.levels.len()
                    );
                    "menu-down"
                }
                ButtonEvent::Select => {
                    let target = self.menu_selected;
                    self.load_level(
                        target,
                        format!("Level {} started  Tilt to move", target + 1),
                    );
                    "level-selected"
                }
            }
        } else {
            match event {
                ButtonEvent::Up => {
                    if self.levels.len() <= 1 {
                        return Ok(None);
                    }
                    let prev = self
                        .current_level
                        .checked_sub(1)
                        .unwrap_or(self.levels.len() - 1);
                    self.load_level(prev, format!("Switched to Level {}", prev + 1));
                    "level-prev"
                }
                ButtonEvent::Down => {
                    if self.levels.len() <= 1 {
                        return Ok(None);
                    }
                    let next = (self.current_level + 1) % self.levels.len();
                    self.load_level(next, format!("Switched to Level {}", next + 1));
                    "level-next"
                }
                ButtonEvent::Select => {
                    if self.outcome.completed() && self.levels.len() > 1 {
                        if self.current_level + 1 < self.levels.len() {
                            let next = self.current_level + 1;
                            self.load_level(next, format!("Level {}  Tilt to move", next + 1));
                            "level-advance"
                        } else {
                            self.menu_open = true;
                            self.menu_selected = 0;
                            self.status = "All Levels Solved! Pick any level".into();
                            "menu-open"
                        }
                    } else if self.moves == 0 && self.levels.len() > 1 {
                        self.menu_open = true;
                        self.menu_selected = self.current_level;
                        self.status = "UP/DOWN choose level  SELECT play".into();
                        "menu-open"
                    } else {
                        self.crates = self.initial_crates;
                        self.player = self.start;
                        self.moves = 0;
                        self.pushes = 0;
                        self.outcome = SokobanOutcome::InProgress;
                        if self.levels.len() > 1 {
                            self.status = "Reset! Press SELECT again for Level Menu".into();
                        } else {
                            self.status = "Puzzle reset  Tilt to move".into();
                        }
                        "reset"
                    }
                }
            }
        };

        self.render_commands(canvas)?;
        canvas.reset_dirty_regions();
        let dirty_regions = vec![SOKOBAN_HEADER_RECT, SOKOBAN_BOARD_RECT, SOKOBAN_STATUS_RECT];
        for rect in &dirty_regions {
            canvas.invalidate_rect(*rect);
        }
        canvas.request_refresh();
        Ok(Some(self.result(reason, "none", "none", dirty_regions)))
    }

    fn result(
        &self,
        reason: &'static str,
        direction: &'static str,
        raw_axis: &'static str,
        dirty_regions: Vec<DirtyRect>,
    ) -> SokobanTiltEventResult {
        SokobanTiltEventResult {
            reason,
            row: self.player_row(),
            column: self.player_column(),
            moves: self.moves,
            pushes: self.pushes,
            crates_on_goal: self.crates_on_goal(),
            outcome: self.outcome,
            direction,
            raw_axis,
            dirty_regions,
        }
    }

    fn render_commands(&self, canvas: &mut NativeGameCanvas) -> Result<(), String> {
        canvas.clear_frame();
        canvas.text(
            24,
            82,
            format!(
                "Sokoban Tilt   Lv {}/{}",
                self.current_level + 1,
                self.levels.len()
            ),
            CanvasTextStyle::Heading,
        )?;

        if self.menu_open {
            canvas.text(
                24,
                126,
                "SD LEVEL SELECT (UP/DN + SELECT)".into(),
                CanvasTextStyle::Detail,
            )?;
            canvas.rect(
                SOKOBAN_GRID_X,
                SOKOBAN_GRID_Y,
                SOKOBAN_COLUMNS as i32 * SOKOBAN_CELL_SIZE,
                SOKOBAN_ROWS as i32 * SOKOBAN_CELL_SIZE,
                false,
            )?;
            // Show up to 7 levels at a time with a scrolling window
            let visible_rows = 7usize;
            let start_idx = if self.levels.len() <= visible_rows || self.menu_selected < 3 {
                0
            } else if self.menu_selected + 4 > self.levels.len() {
                self.levels.len() - visible_rows
            } else {
                self.menu_selected - 3
            };

            for (row_offset, idx) in (start_idx..(start_idx + visible_rows).min(self.levels.len())).enumerate() {
                let row_y = SOKOBAN_GRID_Y + 46 + (row_offset as i32 * 48);
                let cursor = if idx == self.menu_selected { ">" } else { " " };
                let badge = if self.solved_levels[idx] {
                    "[SOLVED]"
                } else {
                    "[OPEN]"
                };
                let style = if idx == self.menu_selected {
                    CanvasTextStyle::Heading
                } else {
                    CanvasTextStyle::Body
                };
                canvas.text(
                    SOKOBAN_GRID_X + 24,
                    row_y,
                    format!("{cursor} Level {}   {badge}", idx + 1),
                    style,
                )?;
            }
        } else {
            canvas.text(
                24,
                126,
                "UP/DN switch level  SELECT reset/menu".into(),
                CanvasTextStyle::Detail,
            )?;
            canvas.grid(
                SOKOBAN_GRID_X,
                SOKOBAN_GRID_Y,
                SOKOBAN_COLUMNS as u8,
                SOKOBAN_ROWS as u8,
                SOKOBAN_CELL_SIZE,
                SOKOBAN_CELL_SIZE,
            )?;
            for index in 0..SOKOBAN_CELL_COUNT {
                let (x, y) = cell_origin(index);
                if self.walls[index] {
                    canvas.rect(
                        x + 1,
                        y + 1,
                        SOKOBAN_CELL_SIZE - 2,
                        SOKOBAN_CELL_SIZE - 2,
                        true,
                    )?;
                } else {
                    if self.goals[index] {
                        canvas.text(x + 13, y + 30, "G".into(), CanvasTextStyle::Heading)?;
                    }
                    if self.crates[index] {
                        canvas.rect(
                            x + 7,
                            y + 7,
                            SOKOBAN_CELL_SIZE - 14,
                            SOKOBAN_CELL_SIZE - 14,
                            false,
                        )?;
                        canvas.text(x + 13, y + 30, "C".into(), CanvasTextStyle::Heading)?;
                    }
                }
            }
            let (px, py) = cell_origin(self.player);
            canvas.rect(
                px + 13,
                py + 13,
                SOKOBAN_CELL_SIZE - 26,
                SOKOBAN_CELL_SIZE - 26,
                true,
            )?;
        }

        canvas.text(
            24,
            634,
            format!(
                "LV {}/{}  MOVES {}  PUSHES {}  {}",
                self.current_level + 1,
                self.levels.len(),
                self.moves,
                self.pushes,
                self.outcome.marker().to_ascii_uppercase()
            ),
            CanvasTextStyle::Heading,
        )?;
        canvas.text(24, 682, self.status.clone(), CanvasTextStyle::Body)?;
        canvas.text(
            24,
            734,
            "TILT move  UP/DN level  SELECT next/menu".into(),
            CanvasTextStyle::Body,
        )?;
        Ok(())
    }
}

fn shifted_cell(index: usize, row_delta: i32, column_delta: i32) -> Option<usize> {
    let row = index / SOKOBAN_COLUMNS;
    let column = index % SOKOBAN_COLUMNS;
    let next_row = row as i32 + row_delta;
    let next_column = column as i32 + column_delta;
    if !(0..SOKOBAN_ROWS as i32).contains(&next_row)
        || !(0..SOKOBAN_COLUMNS as i32).contains(&next_column)
    {
        return None;
    }
    Some(next_row as usize * SOKOBAN_COLUMNS + next_column as usize)
}

fn cell_origin(index: usize) -> (i32, i32) {
    let row = index / SOKOBAN_COLUMNS;
    let column = index % SOKOBAN_COLUMNS;
    (
        SOKOBAN_GRID_X + column as i32 * SOKOBAN_CELL_SIZE,
        SOKOBAN_GRID_Y + row as i32 * SOKOBAN_CELL_SIZE,
    )
}

fn cell_rect(index: usize) -> DirtyRect {
    let (x, y) = cell_origin(index);
    DirtyRect::new(x, y, SOKOBAN_CELL_SIZE + 1, SOKOBAN_CELL_SIZE + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    const MAP: &str =
        "#########/#S......#/#.###...#/#.......#/#...C...#/#.......#/#...G...#/#.......#/#########";
    const MULTI_MAP: &str =
        "#########/#S......#/#.###...#/#.......#/#...C...#/#.......#/#...G...#/#.......#/#########|#########/####..###/##.G..###/##..C..##/##.C.S.##/##.G...##/#########/#########/#########";

    fn tilt(axis: MotionAxis) -> ImuDetectedEvent {
        ImuDetectedEvent {
            kind: ImuDetectedEventKind::Tilt(axis),
            at_ms: 1,
        }
    }
    #[test]
    fn portrait_negative_y_moves_logical_right() {
        let mut game = SokobanTiltGame::from_map(MAP).unwrap();
        let mut canvas = NativeGameCanvas::default();
        let result = game
            .apply_motion_and_render(tilt(MotionAxis::NegativeY), &mut canvas)
            .unwrap()
            .unwrap();
        assert_eq!(result.reason, "tilt-move");
        assert_eq!(result.direction, "right");
        assert_eq!(result.raw_axis, "-y");
        assert_eq!((result.row, result.column), (1, 2));
    }
    #[test]
    fn push_to_goal_can_win_and_advance_sd_levels() {
        let mut game = SokobanTiltGame::from_map(MULTI_MAP).unwrap();
        let mut canvas = NativeGameCanvas::default();
        game.player = 3 * SOKOBAN_COLUMNS + 4;
        let first = game
            .apply_motion_and_render(tilt(MotionAxis::PositiveX), &mut canvas)
            .unwrap()
            .unwrap();
        assert_eq!(first.reason, "crate-push");
        let second = game
            .apply_motion_and_render(tilt(MotionAxis::PositiveX), &mut canvas)
            .unwrap()
            .unwrap();
        assert_eq!(second.reason, "goal-reached");
        assert!(second.outcome.completed());

        // Pressing Select after winning advances to SD Level 2 (index 1)
        let advance = game
            .apply_button_and_render(ButtonEvent::Select, &mut canvas)
            .unwrap()
            .unwrap();
        assert_eq!(advance.reason, "level-advance");
        assert_eq!(game.current_level, 1);
    }
    #[test]
    fn blocked_wall_uses_status_only_dirty() {
        let mut game = SokobanTiltGame::from_map(MAP).unwrap();
        let mut canvas = NativeGameCanvas::default();
        let result = game
            .apply_motion_and_render(tilt(MotionAxis::PositiveY), &mut canvas)
            .unwrap()
            .unwrap();
        assert_eq!(result.reason, "wall-blocked");
        assert_eq!(result.dirty_regions, vec![SOKOBAN_STATUS_RECT]);
    }
}