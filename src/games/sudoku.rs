//! Native Sudoku multi-level campaign & level selector for the SD Lua event bridge.
//!
//! All 81-character puzzles live on the SD card in `MAIN.LUA` separated by `|`.
//! Rust owns board state, conflict checks, level progression, level selection menu,
//! dirty-cell invalidation and all redraw commands.

use crate::buttons::ButtonEvent;

use super::{
    canvas::{CanvasTextStyle, NativeGameCanvas},
    dirty_regions::DirtyRect,
};

pub const SUDOKU_CELL_COUNT: usize = 81;
pub const SUDOKU_GRID_X: i32 = 51;
pub const SUDOKU_GRID_Y: i32 = 176;
pub const SUDOKU_CELL_SIZE: i32 = 42;
const SUDOKU_HEADER_RECT: DirtyRect = DirtyRect::new(16, 42, 448, 84);
const SUDOKU_BOARD_RECT: DirtyRect = DirtyRect::new(
    SUDOKU_GRID_X - 2,
    SUDOKU_GRID_Y - 2,
    9 * SUDOKU_CELL_SIZE + 5,
    9 * SUDOKU_CELL_SIZE + 5,
);
const SUDOKU_STATUS_RECT: DirtyRect = DirtyRect::new(16, 612, 448, 148);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ParsedPuzzle {
    initial_board: [u8; SUDOKU_CELL_COUNT],
    givens: [bool; SUDOKU_CELL_COUNT],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SudokuMode {
    Navigate,
    Edit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SudokuMovementAxis {
    Horizontal,
    Vertical,
}

impl SudokuMovementAxis {
    #[must_use]
    pub const fn marker(self) -> &'static str {
        match self {
            Self::Horizontal => "horizontal",
            Self::Vertical => "vertical",
        }
    }

    #[must_use]
    pub const fn short_marker(self) -> &'static str {
        match self {
            Self::Horizontal => "H",
            Self::Vertical => "V",
        }
    }

    #[must_use]
    pub const fn toggled(self) -> Self {
        match self {
            Self::Horizontal => Self::Vertical,
            Self::Vertical => Self::Horizontal,
        }
    }
}

impl SudokuMode {
    #[must_use]
    pub const fn marker(self) -> &'static str {
        match self {
            Self::Navigate => "nav",
            Self::Edit => "edit",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SudokuEventResult {
    pub reason: &'static str,
    pub row: usize,
    pub column: usize,
    pub mode: SudokuMode,
    pub axis: SudokuMovementAxis,
    pub candidate: u8,
    pub completed: bool,
    pub dirty_regions: Vec<DirtyRect>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SudokuGame {
    puzzles: Vec<ParsedPuzzle>,
    solved_puzzles: Vec<bool>,
    current_level: usize,
    menu_open: bool,
    menu_selected: usize,
    board: [u8; SUDOKU_CELL_COUNT],
    givens: [bool; SUDOKU_CELL_COUNT],
    cursor: usize,
    mode: SudokuMode,
    axis: SudokuMovementAxis,
    candidate: u8,
    status: String,
    completed: bool,
}

impl SudokuGame {
    fn parse_single_puzzle(source: &str) -> Result<ParsedPuzzle, String> {
        let trimmed = source.trim();
        if trimmed.len() != SUDOKU_CELL_COUNT {
            return Err(format!(
                "sudoku puzzle must contain {SUDOKU_CELL_COUNT} cells"
            ));
        }
        let mut board = [0_u8; SUDOKU_CELL_COUNT];
        let mut givens = [false; SUDOKU_CELL_COUNT];
        for (index, byte) in trimmed.bytes().enumerate() {
            board[index] = match byte {
                b'0' | b'.' => 0,
                b'1'..=b'9' => byte - b'0',
                _ => return Err("sudoku puzzle accepts only digits, zero or dot".into()),
            };
            givens[index] = board[index] != 0;
        }
        validate_initial_board(&board)?;
        Ok(ParsedPuzzle {
            initial_board: board,
            givens,
        })
    }

    /// Parses one or more 81-char puzzles separated by `|` purely from the SD card string.
    pub fn from_puzzle(source: &str) -> Result<Self, String> {
        let mut puzzles = Vec::new();
        for part in source.split('|') {
            let trimmed = part.trim();
            if !trimmed.is_empty() {
                puzzles.push(Self::parse_single_puzzle(trimmed)?);
            }
        }
        if puzzles.is_empty() {
            return Err(format!(
                "sudoku puzzle must contain {SUDOKU_CELL_COUNT} cells"
            ));
        }

        let first = puzzles[0];
        let total = puzzles.len();
        let completed = is_complete(&first.initial_board);
        let mut solved_puzzles = vec![false; total];
        if completed {
            solved_puzzles[0] = true;
        }

        Ok(Self {
            puzzles,
            solved_puzzles,
            current_level: 0,
            menu_open: false,
            menu_selected: 0,
            board: first.initial_board,
            givens: first.givens,
            cursor: first_editable_cell(&first.givens).unwrap_or(0),
            mode: SudokuMode::Navigate,
            axis: SudokuMovementAxis::Horizontal,
            candidate: 1,
            status: if completed {
                "Puzzle complete".into()
            } else if total > 1 {
                "AXIS H: move  SELECT edit (Clue=Menu)".into()
            } else {
                "AXIS H: UP/DOWN move  SELECT edit".into()
            },
            completed,
        })
    }

    fn load_level(&mut self, index: usize, status_msg: String) {
        let index = index % self.puzzles.len();
        let puz = self.puzzles[index];
        self.current_level = index;
        self.menu_selected = index;
        self.menu_open = false;
        self.board = puz.initial_board;
        self.givens = puz.givens;
        self.cursor = first_editable_cell(&puz.givens).unwrap_or(0);
        self.mode = SudokuMode::Navigate;
        self.candidate = 1;
        self.completed = is_complete(&self.board);
        if self.completed {
            self.solved_puzzles[index] = true;
        }
        self.status = status_msg;
    }

    #[must_use]
    pub const fn mode(&self) -> SudokuMode {
        self.mode
    }

    #[must_use]
    pub const fn cursor_row(&self) -> usize {
        self.cursor / 9
    }

    #[must_use]
    pub const fn cursor_column(&self) -> usize {
        self.cursor % 9
    }

    #[must_use]
    pub const fn movement_axis(&self) -> SudokuMovementAxis {
        self.axis
    }

    #[must_use]
    pub const fn candidate(&self) -> u8 {
        self.candidate
    }

    #[must_use]
    pub const fn completed(&self) -> bool {
        self.completed
    }

    #[must_use]
    pub fn board(&self) -> &[u8; SUDOKU_CELL_COUNT] {
        &self.board
    }

    pub fn render_initial(&self, canvas: &mut NativeGameCanvas) -> Result<(), String> {
        self.render_commands(canvas)?;
        canvas.reset_dirty_regions();
        canvas.invalidate_rect(DirtyRect::new(0, 0, 480, 800));
        canvas.request_refresh();
        Ok(())
    }

    pub fn apply_button_and_render(
        &mut self,
        event: ButtonEvent,
        canvas: &mut NativeGameCanvas,
    ) -> Result<SudokuEventResult, String> {
        let old_cursor = self.cursor;
        let was_menu_open = self.menu_open;
        let old_level = self.current_level;

        let reason = if self.menu_open {
            match event {
                ButtonEvent::Up => {
                    self.menu_selected = self
                        .menu_selected
                        .checked_sub(1)
                        .unwrap_or(self.puzzles.len() - 1);
                    self.status = format!(
                        "Select Level {} of {}",
                        self.menu_selected + 1,
                        self.puzzles.len()
                    );
                    "menu-up"
                }
                ButtonEvent::Down => {
                    self.menu_selected = (self.menu_selected + 1) % self.puzzles.len();
                    self.status = format!(
                        "Select Level {} of {}",
                        self.menu_selected + 1,
                        self.puzzles.len()
                    );
                    "menu-down"
                }
                ButtonEvent::Select => {
                    let target = self.menu_selected;
                    self.load_level(
                        target,
                        format!(
                            "Level {} loaded  AXIS {}: move",
                            target + 1,
                            self.axis.short_marker()
                        ),
                    );
                    "level-selected"
                }
            }
        } else {
            match self.mode {
                SudokuMode::Navigate => self.apply_navigation_button(event),
                SudokuMode::Edit => self.apply_edit_button(event),
            }
        };

        self.render_commands(canvas)?;
        canvas.reset_dirty_regions();
        let mut dirty_regions = if was_menu_open || self.menu_open || old_level != self.current_level {
            vec![SUDOKU_HEADER_RECT, SUDOKU_BOARD_RECT, SUDOKU_STATUS_RECT]
        } else {
            vec![
                cell_rect(old_cursor),
                cell_rect(self.cursor),
                SUDOKU_STATUS_RECT,
            ]
        };
        dirty_regions.dedup();
        for rect in &dirty_regions {
            canvas.invalidate_rect(*rect);
        }
        canvas.request_refresh();
        Ok(SudokuEventResult {
            reason,
            row: self.cursor_row(),
            column: self.cursor_column(),
            mode: self.mode,
            axis: self.axis,
            candidate: self.candidate,
            completed: self.completed,
            dirty_regions,
        })
    }

    pub fn apply_boot_short_press_and_render(
        &mut self,
        canvas: &mut NativeGameCanvas,
    ) -> Result<SudokuEventResult, String> {
        if self.menu_open {
            self.menu_open = false;
            self.status = format!(
                "Menu closed  AXIS {}: UP/DOWN move",
                self.axis.short_marker()
            );
            self.render_commands(canvas)?;
            canvas.reset_dirty_regions();
            let dirty_regions = vec![SUDOKU_HEADER_RECT, SUDOKU_BOARD_RECT, SUDOKU_STATUS_RECT];
            for rect in &dirty_regions {
                canvas.invalidate_rect(*rect);
            }
            canvas.request_refresh();
            return Ok(SudokuEventResult {
                reason: "menu-close",
                row: self.cursor_row(),
                column: self.cursor_column(),
                mode: self.mode,
                axis: self.axis,
                candidate: self.candidate,
                completed: self.completed,
                dirty_regions,
            });
        }

        let reason = match self.mode {
            SudokuMode::Navigate => {
                self.axis = self.axis.toggled();
                self.status = format!(
                    "AXIS {}: UP/DOWN move  SELECT edit",
                    self.axis.short_marker()
                );
                "axis-toggle"
            }
            SudokuMode::Edit => {
                self.mode = SudokuMode::Navigate;
                self.candidate = self.board[self.cursor];
                self.status = format!(
                    "Edit canceled  AXIS {}: UP/DOWN move",
                    self.axis.short_marker()
                );
                "edit-cancel"
            }
        };
        self.render_commands(canvas)?;
        canvas.reset_dirty_regions();
        let dirty_regions = match reason {
            "axis-toggle" => vec![SUDOKU_STATUS_RECT],
            _ => vec![cell_rect(self.cursor), SUDOKU_STATUS_RECT],
        };
        for rect in &dirty_regions {
            canvas.invalidate_rect(*rect);
        }
        canvas.request_refresh();
        Ok(SudokuEventResult {
            reason,
            row: self.cursor_row(),
            column: self.cursor_column(),
            mode: self.mode,
            axis: self.axis,
            candidate: self.candidate,
            completed: self.completed,
            dirty_regions,
        })
    }

    fn apply_navigation_button(&mut self, event: ButtonEvent) -> &'static str {
        match event {
            ButtonEvent::Up => {
                self.cursor = previous_cell(self.cursor, self.axis);
                self.status = format!(
                    "AXIS {}: UP/DOWN move  SELECT edit",
                    self.axis.short_marker()
                );
                "cursor-move"
            }
            ButtonEvent::Down => {
                self.cursor = next_cell(self.cursor, self.axis);
                self.status = format!(
                    "AXIS {}: UP/DOWN move  SELECT edit",
                    self.axis.short_marker()
                );
                "cursor-move"
            }
            ButtonEvent::Select => {
                if self.completed && self.puzzles.len() > 1 {
                    if self.current_level + 1 < self.puzzles.len() {
                        let next = self.current_level + 1;
                        self.load_level(
                            next,
                            format!("Level {} started  SELECT edit", next + 1),
                        );
                        "level-advance"
                    } else {
                        self.menu_open = true;
                        self.menu_selected = 0;
                        self.status = "All Levels Solved! Pick any level".into();
                        "menu-open"
                    }
                } else if self.givens[self.cursor] {
                    if self.puzzles.len() > 1 {
                        self.menu_open = true;
                        self.menu_selected = self.current_level;
                        self.status = "UP/DN choose level  SELECT play  BOOT resume".into();
                        "menu-open"
                    } else {
                        self.status = "Fixed clue: choose another cell".into();
                        "fixed-clue"
                    }
                } else {
                    self.mode = SudokuMode::Edit;
                    self.candidate = self.board[self.cursor];
                    self.status = "EDIT: UP/DOWN value  SELECT save".into();
                    "edit-enter"
                }
            }
        }
    }

    fn apply_edit_button(&mut self, event: ButtonEvent) -> &'static str {
        match event {
            ButtonEvent::Up => {
                self.candidate = if self.candidate == 0 {
                    9
                } else {
                    self.candidate - 1
                };
                self.status = candidate_status(self.candidate);
                "candidate-change"
            }
            ButtonEvent::Down => {
                self.candidate = if self.candidate >= 9 {
                    0
                } else {
                    self.candidate + 1
                };
                self.status = candidate_status(self.candidate);
                "candidate-change"
            }
            ButtonEvent::Select => {
                if self.candidate != 0 && conflicts(&self.board, self.cursor, self.candidate) {
                    self.status = "Conflict: value already used".into();
                    "conflict"
                } else {
                    self.board[self.cursor] = self.candidate;
                    self.mode = SudokuMode::Navigate;
                    self.completed = is_complete(&self.board);
                    if self.completed {
                        self.solved_puzzles[self.current_level] = true;
                    }
                    self.status = if self.completed {
                        if self.current_level + 1 < self.puzzles.len() {
                            format!(
                                "Lv{} Complete! Press SELECT -> Lv{}",
                                self.current_level + 1,
                                self.current_level + 2
                            )
                        } else if self.puzzles.len() > 1 {
                            "All Levels Complete! Press SELECT for Menu".into()
                        } else {
                            "Puzzle complete".into()
                        }
                    } else if self.candidate == 0 {
                        "Cell cleared".into()
                    } else {
                        "Value saved".into()
                    };
                    "commit"
                }
            }
        }
    }

    fn render_commands(&self, canvas: &mut NativeGameCanvas) -> Result<(), String> {
        canvas.clear_frame();
        canvas.text(
            24,
            66,
            format!(
                "Sudoku   Lv {}/{}",
                self.current_level + 1,
                self.puzzles.len()
            ),
            CanvasTextStyle::Heading,
        )?;

        if self.menu_open {
            canvas.text(
                24,
                104,
                "SD LEVEL SELECT (UP/DN + SELECT)".into(),
                CanvasTextStyle::Detail,
            )?;
            canvas.rect(
                SUDOKU_GRID_X,
                SUDOKU_GRID_Y,
                9 * SUDOKU_CELL_SIZE,
                9 * SUDOKU_CELL_SIZE,
                false,
            )?;
            let visible_rows = 7usize;
            let start_idx = if self.puzzles.len() <= visible_rows || self.menu_selected < 3 {
                0
            } else if self.menu_selected + 4 > self.puzzles.len() {
                self.puzzles.len() - visible_rows
            } else {
                self.menu_selected - 3
            };

            for (row_offset, idx) in
                (start_idx..(start_idx + visible_rows).min(self.puzzles.len())).enumerate()
            {
                let row_y = SUDOKU_GRID_Y + 46 + (row_offset as i32 * 48);
                let cursor = if idx == self.menu_selected { ">" } else { " " };
                let badge = if self.solved_puzzles[idx] {
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
                    SUDOKU_GRID_X + 24,
                    row_y,
                    format!("{cursor} Level {}   {badge}", idx + 1),
                    style,
                )?;
            }
        } else {
            canvas.text(
                24,
                104,
                if self.puzzles.len() > 1 {
                    "SELECT on clue opens Level Menu".into()
                } else {
                    "SD Lua event bridge / native board".into()
                },
                CanvasTextStyle::Detail,
            )?;
            canvas.grid(
                SUDOKU_GRID_X,
                SUDOKU_GRID_Y,
                9,
                9,
                SUDOKU_CELL_SIZE,
                SUDOKU_CELL_SIZE,
            )?;
            for offset in [3, 6] {
                let x = SUDOKU_GRID_X + offset * SUDOKU_CELL_SIZE;
                let y = SUDOKU_GRID_Y + offset * SUDOKU_CELL_SIZE;
                canvas.line(
                    x - 1,
                    SUDOKU_GRID_Y,
                    x - 1,
                    SUDOKU_GRID_Y + 9 * SUDOKU_CELL_SIZE,
                )?;
                canvas.line(
                    x + 1,
                    SUDOKU_GRID_Y,
                    x + 1,
                    SUDOKU_GRID_Y + 9 * SUDOKU_CELL_SIZE,
                )?;
                canvas.line(
                    SUDOKU_GRID_X,
                    y - 1,
                    SUDOKU_GRID_X + 9 * SUDOKU_CELL_SIZE,
                    y - 1,
                )?;
                canvas.line(
                    SUDOKU_GRID_X,
                    y + 1,
                    SUDOKU_GRID_X + 9 * SUDOKU_CELL_SIZE,
                    y + 1,
                )?;
            }
            for (index, value) in self.board.iter().copied().enumerate() {
                if value == 0 {
                    continue;
                }
                let row = index / 9;
                let column = index % 9;
                canvas.text(
                    SUDOKU_GRID_X + column as i32 * SUDOKU_CELL_SIZE + 14,
                    SUDOKU_GRID_Y + row as i32 * SUDOKU_CELL_SIZE + 30,
                    value.to_string(),
                    if self.givens[index] {
                        CanvasTextStyle::Heading
                    } else {
                        CanvasTextStyle::Body
                    },
                )?;
            }
            let cursor = cell_rect(self.cursor);
            canvas.rect(cursor.x, cursor.y, cursor.width, cursor.height, false)?;
            canvas.rect(
                cursor.x + 2,
                cursor.y + 2,
                cursor.width - 4,
                cursor.height - 4,
                false,
            )?;
        }

        canvas.text(
            24,
            648,
            format!(
                "LV {}/{}  R{} C{}  MODE {}  AXIS {}  VAL {}",
                self.current_level + 1,
                self.puzzles.len(),
                self.cursor_row() + 1,
                self.cursor_column() + 1,
                self.mode.marker().to_ascii_uppercase(),
                self.axis.short_marker(),
                display_candidate(self.candidate)
            ),
            CanvasTextStyle::Body,
        )?;
        canvas.text(24, 722, footer, CanvasTextStyle::Detail)?;
        let footer = if self.menu_open {
            "UP/DN level  SELECT load  BOOT short close"
        } else {
            match self.mode {
                SudokuMode::Navigate => "BOOT short axis  SELECT clue=Menu  Hold BOOT back",
                SudokuMode::Edit => "BOOT short cancel  SELECT save",
            }
        };
        canvas.text(24, 742, footer.to_string(), CanvasTextStyle::Detail)?;
        canvas.request_refresh();
        Ok(())
    }
}

fn validate_initial_board(board: &[u8; SUDOKU_CELL_COUNT]) -> Result<(), String> {
    for (index, value) in board.iter().copied().enumerate() {
        if value != 0 && conflicts(board, index, value) {
            return Err(format!(
                "sudoku puzzle contains a conflict at cell {}",
                index + 1
            ));
        }
    }
    Ok(())
}

fn conflicts(board: &[u8; SUDOKU_CELL_COUNT], index: usize, value: u8) -> bool {
    let row = index / 9;
    let column = index % 9;
    for peer in 0..9 {
        let row_index = row * 9 + peer;
        let column_index = peer * 9 + column;
        if row_index != index && board[row_index] == value
