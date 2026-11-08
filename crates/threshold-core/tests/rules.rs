use threshold_core::{
    domain::{CheckDraft, Factor, Handle, SlotDraft},
    rules::{D10, Ruleset, turn_seed},
    store::WorldView,
};

fn view() -> WorldView {
    WorldView {
        clock_min: 0,
        entities: vec![],
        live_facts: vec![],
        facts: vec![],
        claims: vec![],
        obstacles_open: vec![],
        events_scheduled: vec![],
        goals_active: vec![],
        attitudes: vec![],
        knowledge: vec![],
        recent_turns: vec![],
        turn_seq: 0,
    }
}

fn check(helps: &[bool]) -> CheckDraft {
    let slot = SlotDraft {
        subject: Handle::Entity(1),
        key: "state".into(),
        value: "changed".into(),
    };
    CheckDraft {
        against: None,
        win: slot.clone(),
        lose: slot,
        factors: helps
            .iter()
            .map(|&helps| Factor {
                fact: Handle::Fact(1),
                helps,
            })
            .collect(),
    }
}

#[test]
fn same_seed_same_roll() {
    for seed in 0..1_000 {
        assert_eq!(D10.roll(6, seed), D10.roll(6, seed));
    }
}

#[test]
fn golden_rolls_are_stable() {
    for (seed, expected) in [(0, 7), (1, 6), (42, 3), (u64::MAX, 9)] {
        assert_eq!(D10.roll(6, seed).value, expected);
    }
}

#[test]
fn difficulty_from_factors() {
    let view = view();
    assert_eq!(D10.difficulty(&check(&[]), &view), 6);
    assert_eq!(D10.difficulty(&check(&[true, true, false]), &view), 5);
    assert_eq!(D10.difficulty(&check(&[false; 6]), &view), 10);
    assert_eq!(D10.difficulty(&check(&[true; 6]), &view), 2);
    assert_eq!(
        D10.difficulty(&check(&[true, true, true, true, true, false]), &view),
        2
    );
}

#[test]
fn distribution_is_sane() {
    let mut counts = [0; 10];
    for seed in 0..10_000 {
        let result = D10.roll(6, seed);
        assert!((1..=10).contains(&result.value));
        counts[usize::from(result.value - 1)] += 1;
    }
    for count in counts {
        assert!((800..=1_200).contains(&count), "face count: {count}");
    }
}

#[test]
fn roll_preserves_inputs_and_compares_inclusively() {
    for seed in 0..100 {
        let value = D10.roll(6, seed).value;
        let at_threshold = D10.roll(value, seed);
        assert_eq!(at_threshold.seed, seed);
        assert_eq!(at_threshold.difficulty, value);
        assert!(at_threshold.success);
        assert!(!D10.roll(value + 1, seed).success);
    }
}

#[test]
fn turn_seed_wraps_and_mixes_sequence() {
    assert_eq!(turn_seed(0, 42), 42);
    assert_eq!(turn_seed(1, 0), 0x9E37_79B9_7F4A_7C15);
    assert_eq!(turn_seed(u64::MAX, 1), 0x61C8_8646_80B5_83EA);
}
