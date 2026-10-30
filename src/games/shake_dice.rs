//! Native Shake Dice & Coin Flip game using QMI8658 shake events.

use crate::{
    buttons::ButtonEvent,
    imu_events::{ImuDetectedEvent, ImuDetectedEventKind},
};

use super::{
    canvas::{CanvasTextStyle, NativeGameCanvas},
    dirty_regions::DirtyRect,
};

const DICE_BOARD_RECT: DirtyRect = DirtyRect::new(16, 170, 448, 390);
const DICE_STATUS_RECT: DirtyRect = DirtyRect::new(16, 570, 448, 190);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DieType {
    Coin,
    D4,
    D6,
    D8,
    D10,
    D12,
    D20,
}

impl DieType {
    pub const ALL: [Self; 7] = [
        Self::Coin,
        Self::D4,
        Self::D6,
        Self::D8,
        Self::D10,
        Self::D12,
        Self::D20,
    ];

    #[must_use]
    pub const fn next_up(self) -> Self {
        match self {
            Self::Coin => Self::D4,
            Self::D4 => Self::D6,
            Self::D6 => Self::D8,
            Self::D8 => Self::D10,
            Self::D10 => Self::D12,
            Self::D12 => Self::D20,
            Self::D20 => Self::Coin,
        }
    }

    #[must_use]
    pub const fn next_down(self) -> Self {
        match self {
            Self::Coin => Self::D20,
            Self::D4 => Self::Coin,
            Self::D6 => Self::D4,
            Self::D8 => Self::D6,
            Self::D10 => Self::D8,
            Self::D12 => Self::D10,
            Self::D20 => Self::D12,
        }
    }

    #[must_use]
    pub const fn sides(self) -> u8 {
        match self {
            Self::Coin => 2,
            Self::D4 => 4,
            Self::D6 => 6,
            Self::D8 => 8,
            Self::D10 => 10,
            Self::D12 => 12,
            Self::D20 => 20,
        }
    }

    #[must_use]
    pub const fn marker(self) -> &'static str {
        match self {
            Self::Coin => "coin",
            Self::D4 => "d4",
            Self::D6 => "d6",
            Self::D8 => "d8",
            Self::D10 => "d10",
            Self::D12 => "d12",
            Self::D20 => "d20",
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Coin => "COIN (50/50)",
            Self::D4 => "D4 (1-4)",
            Self::D6 => "D6 (1-6)",
            Self::D8 => "D8 (1-8)",
            Self::D10 => "D10 (1-10)",
            Self::D12 => "D12 (1-12)",
            Self::D20 => "D20 (1-20)",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShakeDiceEventResult {
    pub reason: &'static str,
    pub die: DieType,
    pub value: u8,
    pub rolls: u32,
    pub dirty_regions: Vec<DirtyRect>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShakeDiceGame {
    die: DieType,
    last_roll: Option<u8>,
    rolls: u32,
    history: Vec<String>,
    rng_state: u32,
    status: String,
}

impl ShakeDiceGame {
    pub fn from_seed(seed: u32) -> Result<Self, String> {
        let rng_state = if seed == 0 { 0xD1CE_2026 } else { seed };
        Ok(Self {
            die: DieType::D6, // Default is d6
            last_roll: None,
            rolls: 0,
            history: Vec::new(),
            rng_state,
            status: "Shake board or press SELECT to roll!".into(),
        })
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
    ) -> Result<Option<ShakeDiceEventResult>, String> {
        let reason = match event {
            ButtonEvent::Up => {
                self.die = self.die.next_up();
                self.last_roll = None;
                self.status = format!("Selected {}  Shake or SELECT to roll", self.die.label());
                "die-up"
            }
            ButtonEvent::Down => {
                self.die = self.die.next_down();
                self.last_roll = None;
                self.status = format!("Selected {}  Shake or SELECT to roll", self.die.label());
                "die-down"
            }
            ButtonEvent::Select => {
                self.roll_now(1);
                "button-roll"
            }
        };

        self.render_commands(canvas)?;
        canvas.reset_dirty_regions();
        let dirty_regions = vec![DICE_BOARD_RECT, DICE_STATUS_RECT];
        for rect in &dirty_regions {
            canvas.invalidate_rect(*rect);
        }
        canvas.request_refresh();

        Ok(Some(ShakeDiceEventResult {
            reason,
            die: self.die,
            value: self.last_roll.unwrap_or(0),
            rolls: self.rolls,
            dirty_regions,
        }))
    }

    pub fn apply_motion_and_render(
        &mut self,
        event: ImuDetectedEvent,
        canvas: &mut NativeGameCanvas,
    ) -> Result<Option<ShakeDiceEventResult>, String> {
        if !matches!(event.kind, ImuDetectedEventKind::Shake { .. }) {
            return Ok(None);
        }
        self.roll_now(event.at_ms as u32);
        self.render_commands(canvas)?;
        canvas.reset_dirty_regions();
        let dirty_regions = vec![DICE_BOARD_RECT, DICE_STATUS_RECT];
        for rect in &dirty_regions {
            canvas.invalidate_rect(*rect);
        }
        canvas.request_refresh();

        Ok(Some(ShakeDiceEventResult {
            reason: "shake-roll",
            die: self.die,
            value: self.last_roll.unwrap_or(0),
            rolls: self.rolls,
            dirty_regions,
        }))
    }

    fn roll_now(&mut self, entropy: u32) {
        self.rng_state ^= entropy.wrapping_mul(0x9E37_79B9).max(1);
        let value = (self.next_random() % u32::from(self.die.sides())) as u8 + 1;
        self.last_roll = Some(value);
        self.rolls = self.rolls.saturating_add(1);

        let entry = match self.die {
            DieType::Coin => {
                if value == 1 {
                    "COIN:HEADS".to_string()
                } else {
                    "COIN:TAILS".to_string()
                }
            }
            _ => format!("{}:{}", self.die.marker().to_ascii_uppercase(), value),
        };
        if self.history.len() >= 5 {
            self.history.remove(0);
        }
        self.history.push(entry);

        self.status = match (self.die, value) {
            (DieType::Coin, 1) => "Flipped HEADS!  Shake again".into(),
            (DieType::Coin, _) => "Flipped TAILS!  Shake again".into(),
            (DieType::D20, 20) => "CRITICAL HIT! Natural 20!".into(),
            (DieType::D20, 1) => "CRITICAL MISS! Natural 1!".into(),
            _ => format!(
                "Rolled {} on {}!  Shake again",
                value,
                self.die.marker().to_ascii_uppercase()
            ),
        };
    }

    fn next_random(&mut self) -> u32 {
        let mut x = self.rng_state;
        if x == 0 {
            x = 0xA341_316C;
        }
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng_state = x;
        x
    }

    fn render_commands(&self, canvas: &mut NativeGameCanvas) -> Result<(), String> {
        canvas.clear_frame();
        canvas.text(24, 82, "Shake Dice & Coin".into(), CanvasTextStyle::Heading)?;
        canvas.text(
            24,
            126,
            "UP/DOWN change die  SHAKE or SELECT to roll".into(),
            CanvasTextStyle::Detail,
        )?;

        // Selector strip showing all available dice
        canvas.text(
            24,
            195,
            format!("ACTIVE MODE:  <  {}  >", self.die.label()),
            CanvasTextStyle::Heading,
        )?;
        canvas.text(
            24,
            232,
            "Modes: COIN | D4 | D6 | D8 | D10 | D12 | D20".into(),
            CanvasTextStyle::Detail,
        )?;

        // Draw centered 200x200 die box using a 1x1 grid cell
        canvas.grid(140, 270, 1, 1, 200, 200)?;

        let main_result = match (self.die, self.last_roll) {
            (_, None) => "READY".to_string(),
            (DieType::Coin, Some(1)) => "HEADS".to_string(),
            (DieType::Coin, Some(_)) => "TAILS".to_string(),
            (_, Some(v)) => format!("[ {v} ]"),
        };

        canvas.text(182, 365, main_result, CanvasTextStyle::Heading)?;

        let sub_label = match (self.die, self.last_roll) {
            (_, None) => "Shake device now".to_string(),
            (DieType::Coin, Some(v)) => format!("Coin toss ({v}/2)"),
            (die, Some(v)) => format!("Rolled {v} out of {}", die.sides()),
        };
        canvas.text(168, 415, sub_label, CanvasTextStyle::Body)?;

        let history_line = if self.history.is_empty() {
            "History: --".to_string()
        } else {
            format!("Recent: {}", self.history.join("  "))
        };
        canvas.text(24, 530, history_line, CanvasTextStyle::Detail)?;

        canvas.text(
            24,
            610,
            format!(
                "TOTAL ROLLS: {}   CURRENT: {}",
                self.rolls,
                self.die.marker().to_ascii_uppercase()
            ),
            CanvasTextStyle::Heading,
        )?;
        canvas.text(24, 664, self.status.clone(), CanvasTextStyle::Body)?;
        canvas.text(
            24,
            724,
            "UP/DOWN die  SHAKE/SELECT roll  Hold BOOT back".into(),
            CanvasTextStyle::Body,
        )?;
        Ok(())
    }
}