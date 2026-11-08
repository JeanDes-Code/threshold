use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::{domain::CheckDraft, store::WorldView};

pub trait Ruleset {
    fn difficulty(&self, check: &CheckDraft, view: &WorldView) -> u8;
    fn roll(&self, difficulty: u8, seed: u64) -> RollResult;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
