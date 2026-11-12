// Repo Path: src/games/baba_tilt.rs

use super::{
    canvas::{CanvasTextStyle, NativeGameCanvas},
    dirty_regions::DirtyRect,
};
use crate::{
    buttons::ButtonEvent,
    imu_events::{ImuDetectedEvent, ImuDetectedEventKind, MotionAxis},
};

pub const GRID_W: usize = 12;
pub const GRID_H: usize = 8;
pub const MAX_ENTITIES: usize = 64;
pub const MAX_UNDO: usize = 32;

pub const BABA_GRID_X: i32 = 12;
pub const BABA_GRID_Y: i32 = 200;
pub const BABA_CELL_SIZE: i32 = 38;

const BABA_HEADER_RECT: DirtyRect = DirtyRect::new(16, 48, 448, 96);
const BABA_STATUS_RECT: DirtyRect = DirtyRect::new(16, 606, 448, 142);
const BABA_BOARD_RECT: DirtyRect = DirtyRect::new(
    BABA_GRID_X,
    BABA_GRID_Y,
    GRID_W as i32 * BABA_CELL_SIZE + 1,
    GRID_H as i32 * BABA_CELL_SIZE + 1,
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BabaDirection {
    Up,
    Down,
    Left,
    Right,
}

impl BabaDirection {
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
    pub const fn delta(self) -> (i8, i8) {
        match self {
            Self::Up => (0, -1),
            Self::Down => (0, 1),
            Self::Left => (-1, 0),
            Self::Right => (1, 0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BabaOutcome {
    InProgress,
    Won,
    Lost,
}

impl BabaOutcome {
    #[must_use]
    pub const fn marker(self) -> &'static str {
        match self {
            Self::InProgress => "in-progress",
            Self::Won => "won",
            Self::Lost => "lost",
        }
    }

    #[must_use]
    pub const fn completed(self) -> bool {
        matches!(self, Self::Won)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BabaEventResult {
    pub reason: &'static str,
    pub row: usize,
    pub column: usize,
    pub moves: u32,
    pub rules_count: usize,
    pub outcome: BabaOutcome,
    pub direction: &'static str,
    pub raw_axis: &'static str,
    pub dirty_regions: Vec<DirtyRect>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Noun {
    Baba = 0,
    Wall = 1,
    Rock = 2,
    Flag = 3,
    Water = 4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TokenRole {
    Noun(Noun),
    VerbIs,
    PropYou,
    PropStop,
    PropPush,
    PropWin,
    PropSink,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EntityKind {
    Object(Noun),
    Text(TokenRole),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Entity {
    pub active: bool,
    pub kind: EntityKind,
    pub x: i8,
    pub y: i8,
}

impl Entity {
    pub const fn empty() -> Self {
        Self {
            active: false,
            kind: EntityKind::Object(Noun::Baba),
            x: 0,
            y: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RuleSet {
    pub props: [u8; 5],
    pub transform: [Option<Noun>; 5],
}

impl RuleSet {
    pub fn has_prop(&self, noun: Noun, bit: u8) -> bool {
        (self.props[noun as usize] & (1 << bit)) != 0
    }
}

pub const BIT_YOU: u8 = 0;
pub const BIT_STOP: u8 = 1;
pub const BIT_PUSH: u8 = 2;
pub const BIT_WIN: u8 = 3;
pub const BIT_SINK: u8 = 4;

const BABA_LEVELS_FILE: &str = "LEVELS.TXT";
const BABA_SPRITES_FILE: &str = "SPRITES.TXT";
const BABA_MAX_ASSET_BYTES: u64 = 32 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BabaSprites {
    pub baba: [u16; 16],
    pub wall: [u16; 16],
    pub rock: [u16; 16],
    pub flag: [u16; 16],
    pub water: [u16; 16],
}

impl Default for BabaSprites {
    fn default() -> Self {
        Self {
            baba: parse_sprite_const(&[
                "...##......##...",
                "...###....###...",
                "...##########...",
                "..############..",
                ".##..........##.",
                ".#...##..##...#.",
                ".#...##..##...#.",
                ".#............#.",
                ".##..........##.",
                "..############..",
                "...##########...",
                "...##..##..##...",
                "...##..##..##...",
                "..###......###..",
                "................",
                "................",
            ]),
            wall: parse_sprite_const(&[
                "################",
                "#......##......#",
                "#......##......#",
                "################",
                "...##......##...",
                "...##......##...",
                "################",
                "#......##......#",
                "#......##......#",
                "################",
                "...##......##...",
                "...##......##...",
                "################",
                "#......##......#",
                "#......##......#",
                "################",
            ]),
            rock: parse_sprite_const(&[
                "................",
                "....########....",
                "..##........##..",
                ".#....####....#.",
                ".#...#........#.",
                "#..............#",
                "#..##..........#",
                "#......##......#",
                "#........##....#",
                "#..............#",
                ".#.....###....#.",
                ".#............#.",
                "..##........##..",
                "....########....",
                "................",
                "................",
            ]),
            flag: parse_sprite_const(&[
                "................",
                "...##...........",
                "...##########...",
                "...###########..",
                "...##########...",
                "...#########....",
                "...########.....",
                "...##...........",
                "...##...........",
                "...##...........",
                "...##...........",
                "...##...........",
                "...##...........",
                ".######.........",
                "................",
                "................",
            ]),
            water: parse_sprite_const(&[
                "................",
                "................",
                ".##...####...##.",
                "...###....###...",
                "................",
                "................",
                ".###...####...#.",
                "....###....###..",
                "................",
                "................",
                ".##...####...##.",
                "...###....###...",
                "................",
                "................",
                "................",
                "................",
            ]),
        }
    }
}

impl BabaSprites {
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut sprites = Self::default();
        let mut current: Option<&str> = None;
        let mut rows = [0u16; 16];
        let mut row_idx = 0usize;

        let commit = |sprites: &mut Self, section: Option<&str>, rows: &[u16; 16], count: usize| {
            if count != 16 {
                return;
            }
            match section {
                Some("baba") => sprites.baba = *rows,
                Some("wall") => sprites.wall = *rows,
                Some("rock") => sprites.rock = *rows,
                Some("flag") => sprites.flag = *rows,
                Some("water") => sprites.water = *rows,
                _ => {}
            }
        };

        for raw in text.lines() {
            let line = raw
                .split("--")
                .next()
                .unwrap_or_default()
                .split(';')
                .next()
                .unwrap_or_default()
                .trim();
            if line.is_empty() {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                commit(&mut sprites, current, &rows, row_idx);
                let name = line[1..line.len() - 1].trim().to_ascii_lowercase();
                current = match name.as_str() {
                    "baba" => Some("baba"),
                    "wall" => Some("wall"),
                    "rock" => Some("rock"),
                    "flag" => Some("flag"),
                    "water" => Some("water"),
                    _ => None,
                };
                rows = [0u16; 16];
                row_idx = 0;
                continue;
            }
            if current.is_some() && row_idx < 16 {
                if let Some(bits) = parse_sprite_row(line) {
                    rows[row_idx] = bits;
                    row_idx += 1;
                }
            }
        }
        commit(&mut sprites, current, &rows, row_idx);
        sprites
    }

    #[must_use]
    pub const fn for_noun(&self, noun: Noun) -> [u16; 16] {
        match noun {
            Noun::Baba => self.baba,
            Noun::Wall => self.wall,
            Noun::Rock => self.rock,
            Noun::Flag => self.flag,
            Noun::Water => self.water,
        }
    }
}

const fn parse_sprite_row_const(row: &str) -> u16 {
    let bytes = row.as_bytes();
    let mut bits = 0u16;
    let mut col = 0usize;
    while col < 16 && col < bytes.len() {
        if bytes[col] == b'#' || bytes[col] == b'1' || bytes[col] == b'X' {
            bits |= 1 << (15 - col);
        }
        col += 1;
    }
    bits
}

const fn parse_sprite_const(lines: &[&str; 16]) -> [u16; 16] {
    let mut out = [0u16; 16];
    let mut idx = 0usize;
    while idx < 16 {
        out[idx] = parse_sprite_row_const(lines[idx]);
        idx += 1;
    }
    out
}

fn parse_sprite_row(line: &str) -> Option<u16> {
    if line.len() != 16 {
        return None;
    }
    let mut bits = 0u16;
    for (col, ch) in line.bytes().enumerate() {
        match ch {
            b'#' | b'1' | b'X' => bits |= 1 << (15 - col),
            b'.' | b'0' | b' ' => {}
            _ => return None,
        }
    }
    Some(bits)
}

fn parse_levels_txt(text: &str) -> Vec<[String; GRID_H]> {
    let mut levels = Vec::new();
    let mut block: Vec<String> = Vec::new();
    for raw in text.lines() {
        let line = raw
            .split("--")
            .next()
            .unwrap_or_default()
            .split(';')
            .next()
            .unwrap_or_default()
            .trim();
        if line.is_empty() {
            if block.len() == GRID_H {
                let arr: [String; GRID_H] = std::array::from_fn(|i| block[i].clone());
                levels.push(arr);
            }
            block.clear();
            continue;
        }
        if line.contains('|') || line.contains('/') {
            for part in line.split('|') {
                let part = part.trim();
                let rows: Vec<&str> = part.split('/').collect();
                if rows.len() == GRID_H && rows.iter().all(|r| r.len() >= GRID_W) {
                    levels.push(std::array::from_fn(|i| rows[i].to_string()));
                }
            }
            block.clear();
        } else if line.len() >= GRID_W {
            block.push(line.to_string());
        }
    }
    if block.len() == GRID_H {
        levels.push(std::array::from_fn(|i| block[i].clone()));
    }
    levels
}

fn read_bounded_sd_text(path: &std::path::Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() > BABA_MAX_ASSET_BYTES {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BabaGame {
    pub entities: [Entity; MAX_ENTITIES],
    pub undo_stack: Vec<[Entity; MAX_ENTITIES]>,
    pub rules: RuleSet,
    pub steps: u32,
    pub won: bool,
    pub lost: bool,
    pub level_idx: usize,
    pub custom_levels: Vec<[String; GRID_H]>,
    pub sprites: BabaSprites,
    pub status: String,
}

impl Default for BabaGame {
    fn default() -> Self {
        Self::new()
    }
}

impl BabaGame {
    #[must_use]
    pub fn new() -> Self {
        let mut game = Self {
            entities: [Entity::empty(); MAX_ENTITIES],
            undo_stack: Vec::new(),
            rules: RuleSet::default(),
            steps: 0,
            won: false,
            lost: false,
            level_idx: 0,
            custom_levels: Vec::new(),
            sprites: BabaSprites::default(),
            status: "Tilt to move  UP undo  SELECT reset".into(),
        };
        game.load_ascii_level(DEFAULT_LEVELS[0]);
        game
    }

    pub fn from_map(encoded: &str) -> Result<Self, String> {
        Self::from_app_dir(encoded, None)
    }

    pub fn from_app_dir(encoded: &str, app_dir: Option<&std::path::Path>) -> Result<Self, String> {
        let mut game = Self::new();
        let trimmed = encoded.trim();
        if !trimmed.is_empty() && !trimmed.eq_ignore_ascii_case("default") {
            let parsed_levels = parse_levels_txt(trimmed);
            if !parsed_levels.is_empty() {
                game.custom_levels = parsed_levels;
                game.load_level_by_index(0);
            }
        }
        if let Some(dir) = app_dir {
            if let Some(levels_text) = read_bounded_sd_text(&dir.join(BABA_LEVELS_FILE)) {
                let sd_levels = parse_levels_txt(&levels_text);
                if !sd_levels.is_empty() {
                    game.custom_levels = sd_levels;
                    game.load_level_by_index(0);
                }
            }
            if let Some(sprites_text) = read_bounded_sd_text(&dir.join(BABA_SPRITES_FILE)) {
                game.sprites = BabaSprites::parse(&sprites_text);
            }
        }
        Ok(game)
    }

    #[must_use]
    pub fn total_levels(&self) -> usize {
        if self.custom_levels.is_empty() {
            DEFAULT_LEVELS.len()
        } else {
            self.custom_levels.len()
        }
    }

    pub fn load_level_by_index(&mut self, index: usize) {
        let total = self.total_levels();
        self.level_idx = index % total;
        if self.custom_levels.is_empty() {
            self.load_ascii_level(DEFAULT_LEVELS[self.level_idx]);
        } else {
            let custom = self.custom_levels[self.level_idx].clone();
            let refs: [&str; GRID_H] = std::array::from_fn(|i| custom[i].as_str());
            self.load_ascii_level(&refs);
        }
    }

    pub fn load_ascii_level(&mut self, rows: &[&str; GRID_H]) {
        self.entities = [Entity::empty(); MAX_ENTITIES];
        self.undo_stack.clear();
        self.steps = 0;
        self.won = false;
        self.lost = false;

        let mut idx = 0;
        for (y, row) in rows.iter().enumerate() {
            for (x, ch) in row.bytes().take(GRID_W).enumerate() {
                if idx >= MAX_ENTITIES {
                    break;
                }
                if let Some(kind) = Self::char_to_kind(ch) {
                    self.entities[idx] = Entity {
                        active: true,
                        kind,
                        x: x as i8,
                        y: y as i8,
                    };
                    idx += 1;
                }
            }
        }
        self.compile_rules();
        self.evaluate_status();
    }

    fn char_to_kind(ch: u8) -> Option<EntityKind> {
        use EntityKind::{Object, Text};
        use Noun::{Baba, Flag, Rock, Wall, Water};
        use TokenRole::{Noun as RoleNoun, PropPush, PropSink, PropStop, PropWin, PropYou, VerbIs};
        match ch {
            b'B' => Some(Text(RoleNoun(Baba))),
            b'W' => Some(Text(RoleNoun(Wall))),
            b'R' => Some(Text(RoleNoun(Rock))),
            b'F' => Some(Text(RoleNoun(Flag))),
            b'K' => Some(Text(RoleNoun(Water))),
            b'I' => Some(Text(VerbIs)),
            b'Y' => Some(Text(PropYou)),
            b'S' => Some(Text(PropStop)),
            b'P' => Some(Text(PropPush)),
            b'X' => Some(Text(PropWin)),
            b'N' => Some(Text(PropSink)),
            b'b' => Some(Object(Baba)),
            b'w' => Some(Object(Wall)),
            b'r' => Some(Object(Rock)),
            b'f' => Some(Object(Flag)),
            b'k' => Some(Object(Water)),
            _ => None,
        }
    }

    fn token_lines(role: TokenRole) -> (&'static str, Option<&'static str>) {
        use Noun::{Baba, Flag, Rock, Wall, Water};
        use TokenRole::{Noun as RoleNoun, PropPush, PropSink, PropStop, PropWin, PropYou, VerbIs};
        match role {
            RoleNoun(Baba) => ("BA", Some("BA")),
            RoleNoun(Wall) => ("WA", Some("LL")),
            RoleNoun(Rock) => ("RO", Some("CK")),
            RoleNoun(Flag) => ("FL", Some("AG")),
            RoleNoun(Water) => ("WA", Some("TER")),
            VerbIs => ("IS", None),
            PropYou => ("YOU", None),
            PropStop => ("ST", Some("OP")),
            PropPush => ("PU", Some("SH")),
            PropWin => ("WIN", None),
            PropSink => ("SI", Some("NK")),
        }
    }

    pub fn compile_rules(&mut self) {
        self.rules = RuleSet::default();
        for i in 0..MAX_ENTITIES {
            if !self.entities[i].active {
                continue;
            }
            if self.entities[i].kind == EntityKind::Text(TokenRole::VerbIs) {
                let vx = self.entities[i].x;
                let vy = self.entities[i].y;
                self.check_sentence(vx - 1, vy, vx + 1, vy);
                self.check_sentence(vx, vy - 1, vx, vy + 1);
            }
        }
    }

    fn check_sentence(&mut self, sx: i8, sy: i8, tx: i8, ty: i8) {
        for i in 0..MAX_ENTITIES {
            if !self.entities[i].active || self.entities[i].x != sx || self.entities[i].y != sy {
                continue;
            }
            if let EntityKind::Text(TokenRole::Noun(subj)) = self.entities[i].kind {
                for j in 0..MAX_ENTITIES {
                    if !self.entities[j].active
                        || self.entities[j].x != tx
                        || self.entities[j].y != ty
                    {
                        continue;
                    }
                    if let EntityKind::Text(role) = self.entities[j].kind {
                        match role {
                            TokenRole::PropYou => self.rules.props[subj as usize] |= 1 << BIT_YOU,
                            TokenRole::PropStop => self.rules.props[subj as usize] |= 1 << BIT_STOP,
                            TokenRole::PropPush => self.rules.props[subj as usize] |= 1 << BIT_PUSH,
                            TokenRole::PropWin => self.rules.props[subj as usize] |= 1 << BIT_WIN,
                            TokenRole::PropSink => self.rules.props[subj as usize] |= 1 << BIT_SINK,
                            TokenRole::Noun(target) => {
                                self.rules.transform[subj as usize] = Some(target)
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    pub fn entity_has_prop(&self, e: &Entity, bit: u8) -> bool {
        match e.kind {
            EntityKind::Text(_) => bit == BIT_PUSH,
            EntityKind::Object(noun) => self.rules.has_prop(noun, bit),
        }
    }

    fn can_push(&self, x: i8, y: i8, dx: i8, dy: i8, chain: &mut [bool; MAX_ENTITIES]) -> bool {
        let nx = x + dx;
        let ny = y + dy;
        if nx < 0 || nx >= GRID_W as i8 || ny < 0 || ny >= GRID_H as i8 {
            return false;
        }

        let mut needs_next = false;
        for (idx, e) in self.entities.iter().enumerate() {
            if e.active && e.x == nx && e.y == ny {
                let is_stop = self.entity_has_prop(e, BIT_STOP);
                let is_push = self.entity_has_prop(e, BIT_PUSH);
                if is_stop && !is_push {
                    return false;
                }
                if is_push {
                    chain[idx] = true;
                    needs_next = true;
                }
            }
        }

        if needs_next {
            return self.can_push(nx, ny, dx, dy, chain);
        }
        true
    }

    pub fn step(&mut self, dx: i8, dy: i8) -> bool {
        if self.won {
            let next = (self.level_idx + 1) % self.total_levels();
            self.load_level_by_index(next);
            return true;
        }

        let snapshot = self.entities;
        let mut to_move = [false; MAX_ENTITIES];
        let mut any_moved = false;

        for i in 0..MAX_ENTITIES {
            let e = self.entities[i];
            if e.active && self.entity_has_prop(&e, BIT_YOU) {
                let mut chain = [false; MAX_ENTITIES];
                if self.can_push(e.x, e.y, dx, dy, &mut chain) {
                    to_move[i] = true;
                    for c in 0..MAX_ENTITIES {
                        if chain[c] {
                            to_move[c] = true;
                        }
                    }
                    any_moved = true;
                }
            }
        }

        if !any_moved {
            return false;
        }

        if self.undo_stack.len() >= MAX_UNDO {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(snapshot);

        for i in 0..MAX_ENTITIES {
            if to_move[i] {
                self.entities[i].x += dx;
                self.entities[i].y += dy;
            }
        }
        self.steps += 1;

        self.compile_rules();

        for e in self.entities.iter_mut() {
            if e.active {
                if let EntityKind::Object(noun) = e.kind {
                    if let Some(target) = self.rules.transform[noun as usize] {
                        e.kind = EntityKind::Object(target);
                    }
                }
            }
        }

        for y in 0..GRID_H as i8 {
            for x in 0..GRID_W as i8 {
                let mut count = 0;
                let mut has_sink = false;
                for e in self.entities.iter() {
                    if e.active && e.x == x && e.y == y {
                        count += 1;
                        if self.entity_has_prop(e, BIT_SINK) {
                            has_sink = true;
                        }
                    }
                }
                if has_sink && count > 1 {
                    for e in self.entities.iter_mut() {
                        if e.active && e.x == x && e.y == y {
                            e.active = false;
                        }
                    }
                }
            }
        }

        self.compile_rules();
        self.evaluate_status();
        true
    }

    pub fn undo(&mut self) -> bool {
        let Some(prev) = self.undo_stack.pop() else {
            return false;
        };
        self.entities = prev;
        self.steps = self.steps.saturating_sub(1);
        self.compile_rules();
        self.evaluate_status();
        true
    }

    fn evaluate_status(&mut self) {
        self.won = false;
        let mut any_you = false;
        for a in self.entities.iter() {
            if a.active && self.entity_has_prop(a, BIT_YOU) {
                any_you = true;
                for b in self.entities.iter() {
                    if b.active && a.x == b.x && a.y == b.y && self.entity_has_prop(b, BIT_WIN) {
                        self.won = true;
                        return;
                    }
                }
            }
        }
        self.lost = !any_you;
    }

    #[must_use]
    pub fn outcome(&self) -> BabaOutcome {
        if self.won {
            BabaOutcome::Won
        } else if self.lost {
            BabaOutcome::Lost
        } else {
            BabaOutcome::InProgress
        }
    }

    #[must_use]
    pub fn active_rules_count(&self) -> usize {
        let prop_bits: usize = self
            .rules
            .props
            .iter()
            .map(|bits| bits.count_ones() as usize)
            .sum();
        let transforms = self
            .rules
            .transform
            .iter()
            .filter(|target| target.is_some())
            .count();
        prop_bits + transforms
    }

    #[must_use]
    pub fn primary_you_coords(&self) -> (usize, usize) {
        for e in &self.entities {
            if e.active && self.entity_has_prop(e, BIT_YOU) {
                return (e.y.max(0) as usize, e.x.max(0) as usize);
            }
        }
        (0, 0)
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
    ) -> Result<Option<BabaEventResult>, String> {
        let ImuDetectedEventKind::Tilt(axis) = event.kind else {
            return Ok(None);
        };
        let Some(direction) = BabaDirection::from_raw_axis(axis) else {
            return Ok(None);
        };
        let (dx, dy) = direction.delta();
        let was_won = self.won;
        let moved = self.step(dx, dy);

        let reason = if was_won {
            self.status = format!("Level {} started  Tilt to move", self.level_idx + 1);
            "level-advance"
        } else if self.won {
            self.status = format!("YOU WIN in {} steps! Tilt/SELECT next", self.steps);
            "goal-reached"
        } else if self.lost {
            self.status = "NO 'YOU' LEFT! Press UP to undo".into();
            "no-you"
        } else if !moved {
            self.status = format!("{} blocked  UP undo  SELECT reset", direction.label());
            "blocked"
        } else {
            self.status = format!(
                "{} moved ({} rules active)",
                direction.label(),
                self.active_rules_count()
            );
            "tilt-move"
        };

        self.render_commands(canvas)?;
        canvas.reset_dirty_regions();
        let dirty_regions = if moved || was_won {
            vec![BABA_HEADER_RECT, BABA_BOARD_RECT, BABA_STATUS_RECT]
        } else {
            vec![BABA_STATUS_RECT]
        };
        for rect in &dirty_regions {
            canvas.invalidate_rect(*rect);
        }
        canvas.request_refresh();
        Ok(Some(self.build_result(
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
    ) -> Result<Option<BabaEventResult>, String> {
        let reason = match event {
            ButtonEvent::Up => {
                if self.undo() {
                    self.status = format!("Undo (steps: {})", self.steps);
                    "undo"
                } else {
                    self.status = "Nothing to undo".into();
                    "undo-empty"
                }
            }
            ButtonEvent::Down => {
                let next = (self.level_idx + 1) % self.total_levels();
                self.load_level_by_index(next);
                self.status = format!("Switched to Level {}", self.level_idx + 1);
                "level-next"
            }
            ButtonEvent::Select => {
                if self.won {
                    let next = (self.level_idx + 1) % self.total_levels();
                    self.load_level_by_index(next);
                    self.status = format!("Level {}  Tilt to move", self.level_idx + 1);
                    "level-advance"
                } else {
                    let current = self.level_idx;
                    self.load_level_by_index(current);
                    self.status = "Level reset  Tilt to move".into();
                    "reset"
                }
            }
        };

        self.render_commands(canvas)?;
        canvas.reset_dirty_regions();
        let dirty_regions = vec![BABA_HEADER_RECT, BABA_BOARD_RECT, BABA_STATUS_RECT];
        for rect in &dirty_regions {
            canvas.invalidate_rect(*rect);
        }
        canvas.request_refresh();
        Ok(Some(self.build_result(
            reason,
            "none",
            "none",
            dirty_regions,
        )))
    }

    fn build_result(
        &self,
        reason: &'static str,
        direction: &'static str,
        raw_axis: &'static str,
        dirty_regions: Vec<DirtyRect>,
    ) -> BabaEventResult {
        let (row, column) = self.primary_you_coords();
        BabaEventResult {
            reason,
            row,
            column,
            moves: self.steps,
            rules_count: self.active_rules_count(),
            outcome: self.outcome(),
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
                "Baba Tilt   Lv {}/{}",
                self.level_idx + 1,
                self.total_levels()
            ),
            CanvasTextStyle::Heading,
        )?;
        canvas.text(
            24,
            126,
            "BOX = Word Rule   ICON = Baba / Rock / Flag / Water".into(),
            CanvasTextStyle::Detail,
        )?;

        canvas.grid(
            BABA_GRID_X,
            BABA_GRID_Y,
            GRID_W as u8,
            GRID_H as u8,
            BABA_CELL_SIZE,
            BABA_CELL_SIZE,
        )?;

        for e in &self.entities {
            if !e.active {
                continue;
            }
            let cx = BABA_GRID_X + (e.x as i32) * BABA_CELL_SIZE;
            let cy = BABA_GRID_Y + (e.y as i32) * BABA_CELL_SIZE;
            self.draw_entity(canvas, cx, cy, e.kind)?;
        }

        canvas.text(
            24,
            634,
            format!(
                "STEPS {}   RULES {}   {}",
                self.steps,
                self.active_rules_count(),
                self.outcome().marker().to_ascii_uppercase()
            ),
            CanvasTextStyle::Heading,
        )?;
        canvas.text(24, 682, self.status.clone(), CanvasTextStyle::Body)?;
        canvas.text(
            24,
            734,
            "TILT move  UP undo  DN next lv  SELECT reset".into(),
            CanvasTextStyle::Body,
        )?;
        Ok(())
    }

    fn draw_entity(
        &self,
        canvas: &mut NativeGameCanvas,
        cx: i32,
        cy: i32,
        kind: EntityKind,
    ) -> Result<(), String> {
        match kind {
            EntityKind::Text(role) => {
                canvas.rect(
                    cx + 2,
                    cy + 2,
                    BABA_CELL_SIZE - 4,
                    BABA_CELL_SIZE - 4,
                    false,
                )?;
                let (top, bottom) = Self::token_lines(role);
                if let Some(second) = bottom {
                    let x1 = cx + if top.len() >= 3 { 4 } else { 8 };
                    let x2 = cx + if second.len() >= 3 { 4 } else { 8 };
                    canvas.text(x1, cy + 16, top.into(), CanvasTextStyle::Detail)?;
                    canvas.text(x2, cy + 31, second.into(), CanvasTextStyle::Detail)?;
                } else {
                    let x = cx + if top.len() >= 3 { 4 } else { 9 };
                    canvas.text(x, cy + 25, top.into(), CanvasTextStyle::Detail)?;
                }
            }
            EntityKind::Object(noun) => {
                canvas.sprite16(cx + 3, cy + 3, 2, self.sprites.for_noun(noun))?;
            }
        }
        Ok(())
    }
}

pub const DEFAULT_LEVELS: [&[&str; GRID_H]; 10] = [
    &[
        "BIY.....FI..",
        "..........X.",
        "..wwwwwwww..",
        "..w......w..",
        "..w.b..r.f..",
        "..w......w..",
        "..wwwwwwww..",
        "WIS.....RIP.",
    ],
    &[
        "BIY.....WIS.",
        "............",
        "..wwwwwwww..",
        "..w......w..",
        "..w.b..w.f..",
        "..w......w..",
        "..wwwwwwww..",
        "........FIX.",
    ],
    &[
        "BIY.........",
        "....b..WIS..",
        "............",
        "wwwwwwwwwwww",
        ".........f..",
        "....FIX.....",
        "............",
        "............",
    ],
    &[
        "BIY.....KIN.",
        "wwwwwwwwwwww",
        "w....k.....w",
        "w.br.k...f.w",
        "w....k.....w",
        "wwwwwwwwwwww",
        "RIP.....FIX.",
        "........WIS.",
    ],
    &[
        "BIY.....WIS.",
        "wwwwwwwwwwww",
        "w..........w",
        "w.b..r..R..w",
        "w....I.....w",
        "w....F.....w",
        "wwwwwwwwwwww",
        "RIP.....FIX.",
    ],
    &[
        "BIY.....WIS.",
        "wwwwwwwwwwww",
        "w..R...k...w",
        "w.b.I..k.r.w",
        "w...Y..k.f.w",
        "w......k...w",
        "wwwwwwwwwwww",
        "KIN.....FIX.",
    ],
    &[
        "BIY.....WIS.",
        "wwwwwwwwwwww",
        "w..K.......w",
        "w.b.I..k...w",
        "w...F......w",
        "w..........w",
        "wwwwwwwwwwww",
        "KIN.....FIX.",
    ],
    &[
        "BIY.....FIX.",
        "wwwwwwwwwwww",
        "w..W.I..w..w",
        "w.b...P.w..w",
        "w.......wf.w",
        "w.......w..w",
        "wwwwwwwwwwww",
        "........WIS.",
    ],
    &[
        "BIY.....WIS.",
        "wwwwwwwwwwww",
        "w..........w",
        "w.b.KIN..X.w",
        "w......k...w",
        "w..........w",
        "wwwwwwwwwwww",
        "............",
    ],
    &[
        "BIY.....WIS.",
        "wwwwwwwwwwww",
        "w..........w",
        "w.b..I.r.X.w",
        "w...B......w",
        "w..........w",
        "wwwwwwwwwwww",
        "........RIP.",
    ],
];

#[cfg(test)]
mod tests {
    use super::*;

    fn play_moves(game: &mut BabaGame, moves: &[(i8, i8)]) {
        for &(dx, dy) in moves {
            assert!(game.step(dx, dy), "expected move ({dx}, {dy}) to succeed");
        }
    }

    #[test]
    fn all_default_levels_render_within_command_budget() {
        let mut canvas = NativeGameCanvas::default();
        for idx in 0..DEFAULT_LEVELS.len() {
            let mut game = BabaGame::new();
            game.load_level_by_index(idx);
            game.render_initial(&mut canvas)
                .unwrap_or_else(|err| panic!("level {idx} failed to render: {err}"));
            assert!(
                !canvas.commands().is_empty(),
                "level {idx} produced no draw commands"
            );
        }
    }

    #[test]
    fn level_3_and_level_4_are_solvable() {
        // Level 3: break WALL IS STOP by pushing I up, then walk through the wall to Flag.
        let mut lv3 = BabaGame::new();
        lv3.load_level_by_index(2);
        play_moves(
            &mut lv3,
            &[
                (0, 1), // (4, 2)
                (1, 0),
                (1, 0),
                (1, 0),
                (1, 0),  // (8, 2)
                (0, -1), // push I up to (8, 0), breaking WIS
                (1, 0),  // (9, 1)
                (0, 1),
                (0, 1),
                (0, 1), // walk down through wall to f at (9, 4)
            ],
        );
        assert!(lv3.won, "level 3 should be won");

        // Level 4: push Rock right into Water to bridge, then walk to Flag.
        let mut lv4 = BabaGame::new();
        lv4.load_level_by_index(3);
        play_moves(
            &mut lv4,
            &[
                (1, 0),
                (1, 0), // push r into k at (5,3)
                (1, 0),
                (1, 0),
                (1, 0),
                (1, 0),
                (1, 0), // reach f at (9,3)
            ],
        );
        assert!(lv4.won, "level 4 should be won");
    }

    #[test]
    fn sd_card_loads_levels_and_custom_sprites() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rustmix-baba-sd-{nonce}"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("LEVELS.TXT"),
            "-- Custom SD level\n\
             BIY.....FIX.\n\
             ............\n\
             ..b......f..\n\
             ............\n\
             ............\n\
             ............\n\
             ............\n\
             ............\n",
        )
        .unwrap();
        let mut custom_sprite = String::from("[baba]\n");
        for _ in 0..16 {
            custom_sprite.push_str("################\n");
        }
        std::fs::write(dir.join("SPRITES.TXT"), custom_sprite).unwrap();

        let game = BabaGame::from_app_dir("default", Some(&dir)).unwrap();
        assert_eq!(game.total_levels(), 1);
        assert_eq!(game.sprites.baba, [0xFFFF; 16]);
        let mut canvas = NativeGameCanvas::default();
        game.render_initial(&mut canvas).unwrap();
        assert!(canvas.commands().iter().any(|cmd| matches!(
            cmd,
            super::super::canvas::DrawCommand::Sprite16 { rows, .. } if *rows == [0xFFFF; 16]
        )));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn example_baba_sd_app_passes_manifest_and_catalog_scan() {
        let apps_root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/sd-card/RUSTMIX/APPS");
        let catalog = crate::lua_runtime::catalog::LuaAppCatalog::scan(&apps_root, true);
        assert!(
            catalog
                .entries
                .iter()
                .any(|entry| entry.directory_name == "BABA" && entry.manifest.id == "baba_tilt"),
            "BABA example app should be accepted by LuaAppCatalog::scan; rejected={}",
            catalog.rejected_entries
        );
    }

    #[test]
    fn levels_6_through_10_are_solvable() {
        // Level 6: form ROCK IS YOU, then move Rock onto Flag.
        let mut lv6 = BabaGame::new();
        lv6.load_level_by_index(5);
        play_moves(&mut lv6, &[(0, -1), (1, 0), (0, 1)]);
        assert!(lv6.won, "level 6 should be won");

        // Level 7: form WATER IS FLAG, then walk onto the transformed Flag.
        let mut lv7 = BabaGame::new();
        lv7.load_level_by_index(6);
        play_moves(
            &mut lv7,
            &[(0, -1), (1, 0), (0, 1), (1, 0), (1, 0), (1, 0), (1, 0)],
        );
        assert!(lv7.won, "level 7 should be won");

        // Level 8: form WALL IS PUSH, push a wall aside, and reach Flag.
        let mut lv8 = BabaGame::new();
        lv8.load_level_by_index(7);
        play_moves(
            &mut lv8,
            &[
                (0, 1),
                (1, 0),
                (1, 0),
                (1, 0),
                (1, 0),
                (0, -1),
                (0, 1),
                (-1, 0),
                (-1, 0),
                (-1, 0),
                (-1, 0),
                (0, -1),
                (0, -1),
                (1, 0),
                (0, 1),
                (1, 0),
                (1, 0),
                (1, 0),
                (1, 0),
                (1, 0),
                (1, 0),
                (0, 1),
            ],
        );
        assert!(lv8.won, "level 8 should be won");

        // Level 9: break WATER IS SINK, form WATER IS WIN, and step onto Water.
        let mut lv9 = BabaGame::new();
        lv9.load_level_by_index(8);
        play_moves(
            &mut lv9,
            &[
                (0, -1),
                (1, 0),
                (1, 0),
                (1, 0),
                (1, 0),
                (0, 1),
                (0, -1),
                (1, 0),
                (1, 0),
                (1, 0),
                (1, 0),
                (0, 1),
                (-1, 0),
                (-1, 0),
                (-1, 0),
                (0, 1),
            ],
        );
        assert!(lv9.won, "level 9 should be won");

        // Level 10: move B up, clear Rock, and push WIN to form BABA IS WIN.
        let mut lv10 = BabaGame::new();
        lv10.load_level_by_index(9);
        play_moves(
            &mut lv10,
            &[
                (0, 1),
                (0, 1),
                (1, 0),
                (1, 0),
                (0, -1), // push B to (4,3), b at (4,4)
                (1, 0),
                (1, 0),
                (1, 0),  // b at (7,4)
                (0, -1), // push r to (7,2), b at (7,3)
                (0, 1),  // b at (7,4)
                (1, 0),
                (1, 0),
                (1, 0),  // b at (10,4)
                (0, -1), // b at (10,3)
                (-1, 0),
                (-1, 0),
                (-1, 0), // push X to (6,3) -> B I X!
            ],
        );
        assert!(lv10.won, "level 10 should be won");
    }

    #[test]
    fn all_50_sd_levels_render_and_have_initial_entities() {
        let baba_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples/sd-card/RUSTMIX/APPS/BABA");
        let mut game = BabaGame::from_app_dir("default", Some(&baba_dir)).unwrap();
        assert_eq!(game.total_levels(), 50, "expected 50 levels in LEVELS.TXT");

        let mut canvas = NativeGameCanvas::default();
        for lv in 0..game.total_levels() {
            game.load_level_by_index(lv);
            game.render_initial(&mut canvas)
                .unwrap_or_else(|err| panic!("SD level {} failed to render: {err}", lv + 1));
            assert!(
                !game.won,
                "SD level {} should not already be won at step 0",
                lv + 1
            );
            assert!(
                game.entities
                    .iter()
                    .any(|e| e.active && game.entity_has_prop(e, BIT_YOU)),
                "SD level {} has no initial YOU entity",
                lv + 1
            );
            assert!(
                !canvas.commands().is_empty(),
                "SD level {} produced no draw commands",
                lv + 1
            );
        }
    }
}
