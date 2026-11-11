//! The time a Turn takes (bug pass 5, M3): the minutes a wait Intent
//! states, and the most minutes a model may give each action.

use threshold_core::{
    domain::ActionKind,
    rules::{ACT_MAX_MIN, MOVE_MAX_MIN, TALK_MAX_MIN, max_minutes},
    world::intent_minutes,
};

/// 08:00 on Day 1.
const MORNING: u64 = 8 * 60;

#[test]
fn a_wait_intent_states_its_minutes() {
    for (intent, minutes) in [
        ("I wait here for an hour.", 60),
        ("I wait here for 30 minutes.", 30),
        ("I sit by the hearth and rest for 30 minutes.", 30),
        (
            "I take a room upstairs at the inn and sleep for eight hours.",
            480,
        ),
        ("I wait for half an hour.", 30),
        ("I wait an hour and a half.", 90),
        ("I wait one and a half hours.", 90),
        ("I wait for a minute.", 1),
        ("I wait two hours.", 120),
        ("I wait 90 minutes by the door.", 90),
        ("I doze for forty five minutes.", 45),
        ("I wait twenty minutes.", 20),
        ("I wait two hours and ten minutes.", 130),
        ("I wait a quarter of an hour.", 15),
        ("I nap for 3 hrs.", 180),
        ("I work at the mill for four days.", 4 * 1440),
    ] {
        assert_eq!(intent_minutes(intent, MORNING), Some(minutes), "{intent}");
    }
}

#[test]
fn until_a_time_of_day_counts_from_the_clock() {
    // 21:00 to 06:00 the next day.
    assert_eq!(intent_minutes("I sleep until dawn.", 21 * 60), Some(540));
    assert_eq!(intent_minutes("I wait until noon.", MORNING), Some(240));
    assert_eq!(
        intent_minutes("I rest here till nightfall.", MORNING),
        Some(11 * 60)
    );
    assert_eq!(
        intent_minutes("I sleep until the morning.", 22 * 60),
        Some(600)
    );
    // The clock of a later day counts the same.
    assert_eq!(
        intent_minutes("I wait until dusk.", 1440 + 17 * 60),
        Some(60)
    );
}

#[test]
fn a_wait_with_no_duration_states_none() {
    for intent in [
        "I wait here.",
        "I wait a while by the fire.",
        "I sleep.",
        "I wait until Orsel comes back.",
        "I keep watch through the night.",
    ] {
        assert_eq!(intent_minutes(intent, MORNING), None, "{intent}");
    }
}

#[test]
fn each_action_has_a_longest_time() {
    assert_eq!(max_minutes(ActionKind::Talk), TALK_MAX_MIN);
    assert_eq!(max_minutes(ActionKind::Act), ACT_MAX_MIN);
    assert_eq!(max_minutes(ActionKind::Observe), ACT_MAX_MIN);
    assert_eq!(max_minutes(ActionKind::Move), MOVE_MAX_MIN);
    assert_eq!((TALK_MAX_MIN, ACT_MAX_MIN, MOVE_MAX_MIN), (15, 30, 60));
}
