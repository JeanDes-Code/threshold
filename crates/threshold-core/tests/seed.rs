//! The Adventure Seed's agreement fields, the stable block and its cap, and
//! open items on `pending_` keys (ADR 0005).

mod support;

use std::path::Path;

use support::{new_npc, proposal};
use threshold_core::{
    checks::{CheckKind, validate},
    context::{
        ADVENTURE_BLOCK_MAX_CHARS, adventure_block_chars, narration_prompt, proposal_prompt,
    },
    domain::{
        AdventureSeed, Canon, OpenMode, StartSeed, Verdict, WorldSetup, is_open_key, is_state_key,
    },
    fixture::load_adventure_seed,
    setup::{SetupError, WORLD_OP_ID, commit_world},
    store::{CampaignId, Store, WorldView},
    turn::create_campaign,
    world::pc_knowledge,
};

fn town_seed() -> AdventureSeed {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/town");
    load_adventure_seed(&dir.join("adventure-seed.toml")).unwrap()
}

/// A seed that sets every agreement field.
fn ferry_seed() -> AdventureSeed {
    toml::from_str(
        r#"
premise = "A ferry crossing at night"
boundaries = ["No harm to animals"]
decided = [{ subject = "Wren", key = "location", value = "north quay", text = "Wren waits on the north quay." }]
proposed = []
story_language = "en"
themes = ["smuggling", "old debts"]
gm_agreement = "Keep scenes short. Let NPCs lie."
resolution = "Roll two dice on every risky crossing."
min_age = 21

[pc]
name = "Wren"
age = 29
other_names = [{ name = "Jay", known_to = [] }]
concept = "A courier"

[[entities]]
name = "Jean"
kind = "npc"
age = 40

[start]
place = "The landing stage"
present = ["Jean"]
situation = "The last ferry is late."
when = "Midnight"

[[open_details]]
text = "Who paid the ferryman"
mode = "discover"

[[open_details]]
text = "The cargo manifest"
mode = "to_agree"
"#,
    )
    .unwrap()
}

fn campaign(seed: &AdventureSeed) -> (Store, CampaignId) {
    let mut store = Store::open_in_memory().unwrap();
    let campaign = create_campaign(&mut store, seed, 11).unwrap();
    (store, campaign)
}

fn world(text: &str) -> WorldSetup {
    toml::from_str(text).unwrap()
}

fn id(view: &WorldView, name: &str) -> u64 {
    view.entities
        .iter()
        .find(|e| e.name == name)
        .unwrap_or_else(|| panic!("no entity {name}"))
        .id
}

#[test]
fn old_seed_toml_still_parses() {
    let seed = town_seed();
    assert!(seed.themes.is_empty());
    assert_eq!(seed.gm_agreement, "");
    assert_eq!(seed.resolution, "");
    assert_eq!(seed.min_age, None);
    assert_eq!(seed.start, StartSeed::default());
    assert!(seed.open_details.is_empty());
}

#[test]
fn new_seed_fields_parse() {
    let seed = ferry_seed();
    assert_eq!(seed.themes, ["smuggling", "old debts"]);
    assert_eq!(seed.min_age, Some(21));
    assert_eq!(seed.start.present, ["Jean"]);
    assert_eq!(seed.open_details[0].mode, OpenMode::Discover);
    assert_eq!(seed.open_details[1].mode, OpenMode::ToAgree);
}

#[test]
fn seed_schema_caps_themes_and_agreement() {
    let schema = serde_json::to_value(schemars::schema_for!(AdventureSeed)).unwrap();
    let props = &schema["properties"];
    assert_eq!(props["themes"]["maxItems"], 8, "{props}");
    assert_eq!(props["themes"]["items"]["maxLength"], 40, "{props}");
    assert_eq!(props["gm_agreement"]["maxLength"], 1_200, "{props}");
}

#[test]
fn stable_block_holds_the_agreed_fields() {
    let seed = ferry_seed();
    let (store, campaign) = campaign(&seed);
    let view = store.snapshot(campaign).unwrap();
    let gm = proposal_prompt(&view, &[], "I wait.").stable;
    let narrator = narration_prompt(&view, &[], &[], &[], &[], "I wait.").stable;

    for block in [&gm, &narrator] {
        assert!(block.contains("A ferry crossing at night"), "{block}");
        assert!(block.contains("Wren (age 29; also called Jay)"), "{block}");
        assert!(block.contains("Story language: en"), "{block}");
        assert!(block.contains("No harm to animals"), "{block}");
        assert_eq!(
            block
                .matches("Every character is at least 21 years old.")
                .count(),
            1,
            "{block}"
        );
        assert!(block.contains("smuggling, old debts"), "{block}");
        assert!(
            block.contains("Keep scenes short. Let NPCs lie."),
            "{block}"
        );
        for absent in [
            "Roll two dice",
            "landing stage",
            "The last ferry is late.",
            "Midnight",
            "Who paid the ferryman",
            "cargo manifest",
        ] {
            assert!(!block.contains(absent), "{absent} in {block}");
        }
    }
    // The narrator never gets a handle; the GM gets the PC's.
    assert!(gm.contains(&format!("PC: E{} Wren", id(&view, "Wren"))));

    // The measured length is the narrator's block, under the cap.
    assert_eq!(adventure_block_chars(&seed), narrator.chars().count());
    assert!(adventure_block_chars(&seed) <= ADVENTURE_BLOCK_MAX_CHARS);

    // Without `min_age`, no age line.
    let (store, campaign) = self::campaign(&town_seed());
    let view = store.snapshot(campaign).unwrap();
    let block = narration_prompt(&view, &[], &[], &[], &[], "I wait.").stable;
    assert!(!block.contains("years old"), "{block}");
    assert!(block.contains("Wren (age 29)"), "{block}");
}

#[test]
fn open_entry_commits_on_a_pending_key() {
    assert!(is_open_key("pending_shopping_trip"));
    assert!(is_open_key("Pending shopping trip"));
    assert!(!is_open_key("shopping_trip"));
    assert!(is_state_key("pending_shopping_trip"));

    let (mut store, campaign) = campaign(&town_seed());
    let setup = world(
        r#"
[[facts]]
subject = "Hessa"
key = "shopping_trip"
value = "the market"
text = "Hessa plans a shopping trip to the market."
known_by = ["Wren", "Hessa"]
open = true
"#,
    );
    commit_world(&mut store, campaign, &setup).unwrap();

    let view = store.snapshot(campaign).unwrap();
    let hessa = id(&view, "Hessa");
    let open = view
        .live_fact(hessa, "pending_shopping_trip")
        .expect("the open item is a fact on the pending key");
    // The PC holds it, and still it is never Persistent.
    assert_eq!(open.canon, Canon::Established);
    assert!(
        pc_knowledge(&view)
            .iter()
            .any(|b| b.key == "pending_shopping_trip")
    );
    // The outcome slot stays empty.
    assert!(view.live_fact(hessa, "shopping_trip").is_none());
}

#[test]
fn settled_entry_with_the_prefix_is_refused() {
    for key in ["pending_trip", "Pending trip"] {
        let (mut store, campaign) = campaign(&town_seed());
        let before = store.snapshot(campaign).unwrap();
        let setup = world(&format!(
            r#"
[[facts]]
subject = "Hessa"
key = "{key}"
value = "the market"
text = "Hessa plans a trip."
"#
        ));

        let result = commit_world(&mut store, campaign, &setup);

        assert!(matches!(result, Err(SetupError::Invalid(_))), "{result:?}");
        assert!(
            store
                .turn_by_campaign_op(campaign, &WORLD_OP_ID.into())
                .unwrap()
                .is_none()
        );
        assert_eq!(store.snapshot(campaign).unwrap(), before);
    }
}

#[test]
fn open_entry_with_a_hand_written_prefix_is_refused() {
    for key in ["pending_trip", "Pending trip"] {
        let (mut store, campaign) = campaign(&town_seed());
        let before = store.snapshot(campaign).unwrap();
        let setup = world(&format!(
            r#"
[[facts]]
subject = "Hessa"
key = "{key}"
value = "the market"
text = "Hessa plans a trip."
open = true
"#
        ));

        let result = commit_world(&mut store, campaign, &setup);

        assert!(matches!(result, Err(SetupError::Invalid(_))), "{result:?}");
        assert_eq!(store.snapshot(campaign).unwrap(), before);
    }
}

#[test]
fn settled_known_fact_is_held_by_the_pc() {
    let (mut store, campaign) = campaign(&town_seed());
    let setup = world(
        r#"
[[facts]]
subject = "Hessa"
key = "owes"
value = "Wren"
text = "Hessa owes Wren a favor."
known_by = ["Wren"]
"#,
    );
    commit_world(&mut store, campaign, &setup).unwrap();

    let view = store.snapshot(campaign).unwrap();
    let fact = view.live_fact(id(&view, "Hessa"), "owes").unwrap();
    assert_eq!(fact.canon, Canon::Persistent);
    assert_eq!(fact.seen_by, [id(&view, "Wren")]);
    assert!(pc_knowledge(&view).iter().any(|b| b.key == "owes"));
}

#[test]
fn new_npc_below_min_age_is_a_content_boundary_rejection() {
    let (store, campaign) = campaign(&ferry_seed());
    let view = store.snapshot(campaign).unwrap();

    // Not intimate: `min_age` holds for every new NPC.
    let mut p = proposal(Verdict::Allowed);
    p.new_npcs = vec![new_npc(1, "Kett", 20)];
    let errs = validate(&p, &view).expect_err("below min_age");
    assert!(
        errs.iter().any(|r| r.check == CheckKind::ContentBoundary),
        "{errs:?}"
    );

    let mut p = proposal(Verdict::Allowed);
    p.new_npcs = vec![new_npc(1, "Kett", 21)];
    validate(&p, &view).expect("min_age itself is allowed");

    // Without `min_age`, nothing changes.
    let (store, campaign) = self::campaign(&town_seed());
    let view = store.snapshot(campaign).unwrap();
    let mut p = proposal(Verdict::Allowed);
    p.new_npcs = vec![new_npc(1, "Kett", 9)];
    validate(&p, &view).expect("no min_age, no age rule outside intimate scenes");
}
