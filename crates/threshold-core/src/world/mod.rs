//! The World Clock, World Events and Beliefs.
//!
//! `advance` turns the passing of time into ledger ops; it never commits.
//! Beliefs are derived from `knowledge` rows and never stored (ADR 0003).

mod arrival;
mod beliefs;
mod clock;
mod wait;

pub use arrival::{
    arrival, hidden, openly_here, pc_witnesses, with_arrival, with_witnessed_events,
};
pub use beliefs::{Belief, beliefs, pc_knowledge};
pub use clock::{MAX_EVENTS_PER_TURN, advance, written_slots};
pub use wait::{WAIT_DEFAULT_MIN, WAIT_MAX_MIN, wait_minutes};
