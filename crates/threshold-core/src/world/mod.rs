//! The World Clock, World Events and Beliefs.
//!
//! `advance` turns the passing of time into ledger ops; it never commits.
//! Beliefs are derived from `knowledge` rows and never stored (ADR 0003).
//! `pc_sight` gives the world as the PC knows it, for the Player screens.

mod arrival;
mod beliefs;
mod clock;
mod sight;
mod wait;

pub use arrival::{
    arrival, hidden, known_places, openly_here, pc_witnesses, with_arrival, with_witnessed_events,
};
pub use beliefs::{Belief, beliefs, open_item_ref, pc_knowledge, pc_open_items};
pub use clock::{MAX_EVENTS_PER_TURN, advance, written_slots};
pub use sight::{
    Known, Moment, OpenItem, PcSight, SCENE_GAP_MIN, SightLine, SightTurn, pc_can_name, pc_sight,
    tagged, unnameable_names,
};
pub use wait::{WAIT_DEFAULT_MIN, WAIT_MAX_MIN, wait_minutes};
