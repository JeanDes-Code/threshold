use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

use crate::{
    domain::{ActionKind, CheckDraft},
    store::WorldView,
};

/// The game minutes a talk takes when the proposal states none.
pub const TALK_DEFAULT_MIN: u32 = 5;

/// The game minutes an act (or an observe) takes when the proposal states
/// none.
pub const ACT_DEFAULT_MIN: u32 = 10;

/// The game minutes a move takes when the proposal states none.
pub const MOVE_DEFAULT_MIN: u32 = 15;

/// The minutes a Turn of `action` takes when the model leaves `minutes` out
/// or sends 0. A wait is not here: the engine owns its length
/// (`world::wait_minutes`).
pub fn default_minutes(action: ActionKind) -> u32 {
    match action {
        ActionKind::Talk => TALK_DEFAULT_MIN,
        ActionKind::Move => MOVE_DEFAULT_MIN,
        ActionKind::Act | ActionKind::Observe | ActionKind::Wait => ACT_DEFAULT_MIN,
    }
}

/// The longest talk, in game minutes (bug pass 5, M3): QA saw a one-line
/// talk take 90 minutes. A model's `minutes` above it is cut to it.
pub const TALK_MAX_MIN: u32 = 15;

/// The longest act or observe, in game minutes.
pub const ACT_MAX_MIN: u32 = 30;

/// The longest move, in game minutes. The place data holds no travel time
/// yet, so no place gives a move more.
pub const MOVE_MAX_MIN: u32 = 60;

/// The most game minutes a Turn of `action` may take from the model's
/// `minutes`. A wait is not here: the engine owns its length
/// (`world::wait_minutes`).
pub fn max_minutes(action: ActionKind) -> u32 {
    match action {
        ActionKind::Talk => TALK_MAX_MIN,
        ActionKind::Move => MOVE_MAX_MIN,
        ActionKind::Act | ActionKind::Observe | ActionKind::Wait => ACT_MAX_MIN,
    }
}

/// The clock time a named time of day stands for, as minutes after
/// midnight: "until dawn" waits until 06:00 (bug pass 5, M3).
pub const NAMED_TIMES: &[(&str, u32)] = &[
    ("dawn", 6 * 60),
    ("sunrise", 6 * 60),
    ("daybreak", 6 * 60),
    ("morning", 8 * 60),
    ("noon", 12 * 60),
    ("midday", 12 * 60),
    ("afternoon", 14 * 60),
    ("evening", 18 * 60),
    ("dusk", 18 * 60),
    ("sunset", 18 * 60),
    ("nightfall", 19 * 60),
    ("dark", 19 * 60),
    ("night", 21 * 60),
    ("midnight", 0),
];

pub trait Ruleset {
    fn difficulty(&self, check: &CheckDraft, view: &WorldView) -> u8;
    fn roll(&self, difficulty: u8, seed: u64) -> RollResult;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RollResult {
    pub seed: u64,
    pub difficulty: u8,
    pub value: u8,
    pub success: bool,
}

pub fn turn_seed(campaign_seed: u64, turn_seq: u64) -> u64 {
    campaign_seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ turn_seq
}

pub struct D10;

impl Ruleset for D10 {
    fn difficulty(&self, check: &CheckDraft, _view: &WorldView) -> u8 {
        let difficulty = check.factors.iter().fold(6_i128, |difficulty, factor| {
            difficulty + if factor.helps { -1 } else { 1 }
        });
        difficulty.clamp(2, 10) as u8
    }

    fn roll(&self, difficulty: u8, seed: u64) -> RollResult {
        let value = ChaCha8Rng::seed_from_u64(seed).random_range(1..=10);
        RollResult {
            seed,
            difficulty,
            value,
            success: value >= difficulty,
        }
    }
}
