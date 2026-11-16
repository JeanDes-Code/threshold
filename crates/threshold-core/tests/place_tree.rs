//! The place tree in the Seed review (ADR 0008, the trust boundary of
//! 2026-11-16): the draft run proposes an Inside Link for each Place, the
//! fact check sets each link the facts do not support to `Unknown`, the
//! Player confirms the tree, and the World Build writes the confirmed
//! links. The fixture has the geography of QA run 3 under neutral names: a
//! crossing on a river, a ferry house in the crossing, an inn, and a child
//! who lives in the crossing.

use std::{fs, path::PathBuf};

use serde_json::{Value, json};
use threshold_core::{
    RecordedGm,
    build::{
        Control, DraftState, EntryRef, GapKind, PlaceLine, ReviewError, SeedDraft, age_lines,
        apply_edit, approve, build_world, confirm_age, confirm_inside, confirmed_ages,
        draft_from_text, gaps, place_lines, rename,
    },
    domain::{EntityKind, Inside},
    mature::{Blocked, clearance},
    setup::create_campaign_file,
    store::{Store, WorldView},
};

const CROSSING: &str = "Brackwater Crossing";
const RIVER: &str = "Fenn";
const FERRY_HOUSE: &str = "the ferry house";
const INN: &str = "the Lantern Inn";
const CHILD: &str = "Pell Durrow";
/// A model that answers "none" for every place (QA run 3).
const ALL_NONE: [(&str, &str); 4] = [
    (CROSSING, "none"),
    (RIVER, "none"),
    (FERRY_HOUSE, "none"),
    (INN, "none"),
];

/// An empty folder of its own under the system temp dir, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("threshold-tree-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn gm(outputs: &[Value]) -> RecordedGm {
    RecordedGm::new(outputs.iter().map(Value::to_string).collect(), vec![])
}

/// One section of about 450 characters.
fn source() -> String {
    let mut body = String::from("# The crossing\n\n");
    while body.chars().count() < 450 {
        body.push_str(
            "Edda Marsh keeps the ferry house in Brackwater Crossing, on the river Fenn. ",
        );
    }
    body
}

fn roster_out() -> Value {
    let thing = |name: &str, kind: &str| json!({"name": name, "kind": kind, "other_names": []});
    json!({
        "pc": {
            "name": "Edda Marsh", "age": 34, "age_stated": true,
            "other_names": [], "concept": "A ferry keeper."
        },
        "characters": [
            {"name": CHILD, "age": 11, "age_stated": true, "other_names": []}
        ],
        "others": [
            thing(CROSSING, "place"),
            thing(RIVER, "place"),
            thing(FERRY_HOUSE, "place"),
            thing(INN, "place"),
            thing("the ferry", "item"),
        ]
    })
}

fn seed_out(start: &str) -> Value {
    json!({
        "premise": "A ferry keeper works the night crossing.",
        "story_language": "en",
        "themes": [],
        "gm_agreement": "",
        "resolution": "",
        "min_age": null,
        "start": {
            "place": start,
            "present": [],
            "situation": "The last crossing of the night.",
            "when": "A spring night"
        },
        "open_details": []
    })
}

fn fact(subject: &str, value: &str, text: &str) -> Value {
    json!({
        "subject": subject, "key": "location", "value": value, "text": text,
        "open": false, "pc_knows": true
    })
}

/// What the PC knows: the sentence of QA run 3 on the ferry (an item),
/// the crossing on its river, and where the child lives.
fn knowledge_out(child_at: &str) -> Value {
    json!({"facts": [
        fact("the ferry", CROSSING, "The ferry house is in Brackwater Crossing."),
        fact(CROSSING, RIVER, "Brackwater Crossing lies on the river Fenn."),
        fact(CHILD, child_at, &format!("{CHILD} lives at {child_at}.")),
    ]})
}

fn tree_out(links: &[(&str, &str)]) -> Value {
    let list: Vec<Value> = links
        .iter()
        .map(|(place, inside)| json!({"place": place, "inside": inside}))
        .collect();
    json!({ "places": list })
}

/// The draft of the fixture: the PC starts at `start`, the child lives at
/// `child_at`, and the model answers `tree` for the place tree.
fn drafted(start: &str, child_at: &str, tree: &[(&str, &str)]) -> SeedDraft {
    let gm = gm(&[
        roster_out(),
        seed_out(start),
        knowledge_out(child_at),
        tree_out(tree),
    ]);
    let mut progress = |_| {};
    let mut log = |_| {};
    let mut ctl = Control::new(&mut progress, &|| false, &mut log);
    let draft = draft_from_text(&source(), &gm, &mut ctl).unwrap();
    assert_eq!(gm.calls().len(), 4);
    draft
}

/// The Player confirms every age, then every Inside Link as the review
/// shows it, then approves.
fn confirm_as_shown(draft: &mut SeedDraft) {
    for line in age_lines(draft) {
        confirm_age(draft, &line.entry()).unwrap();
    }
    for line in place_lines(draft) {
        confirm_inside(draft, &line.entry(), line.inside.clone()).unwrap();
    }
    approve(draft).unwrap();
}

/// The World Build of the fixture (one hidden-facts call, the child's
/// state, the events, the start state) and the Campaign file it writes.
fn built(draft: &SeedDraft, name: &str) -> WorldView {
    let npc = json!({
        "goals": [{"text": "Pell wants to see the far bank.", "against": null}],
        "secret": null,
        "attitudes": [],
        "voice_card": {
            "habits": "Short questions.",
            "vocabulary": "boat, rope",
            "samples": ["Can I row?", "Is it deep?"]
        }
    });
    let gm = gm(&[
        json!({"facts": []}),
        npc,
        json!({"events": []}),
        json!({"placements": []}),
    ]);
    let mut progress = |_| {};
    let mut log = |_| {};
    let mut ctl = Control::new(&mut progress, &|| false, &mut log);
    let world = build_world(draft, &gm, &mut ctl).unwrap();
    // No place tree call in the World Build.
    assert!(
        gm.calls()
            .iter()
            .all(|c| c.schema["properties"].get("places").is_none())
    );
    let ages = confirmed_ages(draft).unwrap();
    let dir = TempDir::new(name);
    let path = dir.0.join("campaign.sqlite");
    create_campaign_file(&path, &draft.seed, &world, 7, Some(&ages)).unwrap();
    let store = Store::open_existing(&path).unwrap();
    store.snapshot(store.campaigns().unwrap()[0]).unwrap()
}

fn id(view: &WorldView, name: &str) -> u64 {
    view.entity_by_name(name).unwrap().id
}

fn link(view: &WorldView, name: &str) -> Inside {
    view.entity_by_name(name).unwrap().inside
}

fn line<'a>(lines: &'a [PlaceLine], name: &str) -> &'a PlaceLine {
    lines.iter().find(|l| l.name == name).unwrap()
}

fn entry(draft: &SeedDraft, name: &str) -> EntryRef {
    line(&place_lines(draft), name).entry()
}

/// The gate's answer with no one else named: blocked by the child, or
/// cleared.
fn blocked_by_child(view: &WorldView) -> bool {
    match clearance(view, &[]) {
        Err(Blocked { who }) => who == vec![id(view, CHILD)],
        Ok(_) => false,
    }
}

// ---- The fact check ---------------------------------------------------------------

/// QA run 3: a model answered "none" for every place, so the ferry house
/// and the crossing looked like two separate top-level places, and the
/// child in the crossing was "away" from the PC in the ferry house. The
/// fact check sets each link the facts contradict to `Unknown`; the Player
/// confirms the tree as shown, and the gate refuses.
#[test]
fn a_model_that_answers_none_for_every_place_leaves_the_gate_refusing() {
    let all_none = [
        (CROSSING, "none"),
        (RIVER, "none"),
        (FERRY_HOUSE, "none"),
        (INN, "none"),
    ];
    let mut draft = drafted(FERRY_HOUSE, CROSSING, &all_none);
    let lines = place_lines(&draft);
    // "The ferry house is in Brackwater Crossing." (the text rule) and
    // "Brackwater Crossing lies on the river Fenn." (the subject rule, and
    // rule 2 for the river) all disagree with the model.
    for name in [FERRY_HOUSE, CROSSING, RIVER] {
        let l = line(&lines, name);
        assert_eq!(
            (&l.inside, l.doubted, l.confirmed),
            (&Inside::Unknown, true, false),
            "{name}"
        );
    }
    // No fact says where the inn is, and none can back "none": the inn
    // starts on "Choose…", with the model's guess as a hint.
    let inn = line(&lines, INN);
    assert_eq!(
        (&inn.inside, inn.doubted, &inn.suggested),
        (&Inside::Unknown, false, &Some(Inside::Top))
    );

    confirm_as_shown(&mut draft);
    let view = built(&draft, "all-none");
    assert_eq!(link(&view, FERRY_HOUSE), Inside::Unknown);
    assert!(blocked_by_child(&view), "{:?}", clearance(&view, &[]));
}

/// The review starts only a link that a fact backs on the model's choice
/// (ADR 0008, 2026-11-16). The fact check cannot doubt a wrong link that
/// no fact contradicts, so every other link starts on "Choose…", with the
/// model's guess as a hint. The approval gaps do not change.
#[test]
fn only_a_link_a_fact_backs_starts_on_the_model_choice() {
    let model = [
        (CROSSING, RIVER),
        (RIVER, "none"),
        (FERRY_HOUSE, CROSSING),
        (INN, CROSSING),
    ];
    let draft = drafted(INN, CROSSING, &model);
    let lines = place_lines(&draft);
    let shown = |name: &str| {
        let l = line(&lines, name);
        (l.inside.clone(), l.backed, l.suggested.clone(), l.doubted)
    };
    let place = |p: &str| Inside::Place(p.to_owned());
    // Backed parents: "Brackwater Crossing lies on the river Fenn." (the
    // subject rule) and "The ferry house is in Brackwater Crossing." (the
    // text rule).
    assert_eq!(shown(CROSSING), (place(RIVER), true, None, false));
    assert_eq!(shown(FERRY_HOUSE), (place(CROSSING), true, None, false));
    // An unbacked Top, and an unbacked parent: no fact puts the inn anywhere.
    assert_eq!(
        shown(RIVER),
        (Inside::Unknown, false, Some(Inside::Top), false)
    );
    assert_eq!(
        shown(INN),
        (Inside::Unknown, false, Some(place(CROSSING)), false)
    );
    // Every Place still waits for the Player, backed or not.
    let waiting: Vec<String> = gaps(&draft)
        .into_iter()
        .filter(|g| g.kind == GapKind::PlaceUnconfirmed)
        .map(|g| g.at)
        .collect();
    assert_eq!(waiting.len(), 4, "{waiting:?}");
}

/// The model put the ferry house and the crossing both in the river: two
/// siblings, so "away" from each other. The facts put the ferry house in
/// the crossing, so its link goes to `Unknown`, and the gate refuses.
#[test]
fn a_wrong_parent_from_the_model_leaves_the_gate_refusing() {
    let river_parent = [
        (CROSSING, RIVER),
        (RIVER, "none"),
        (FERRY_HOUSE, RIVER),
        (INN, "none"),
    ];
    let mut draft = drafted(FERRY_HOUSE, CROSSING, &river_parent);
    let lines = place_lines(&draft);
    let ferry = line(&lines, FERRY_HOUSE);
    assert_eq!((&ferry.inside, ferry.doubted), (&Inside::Unknown, true));
    // The crossing in the river agrees with its fact, and so does the
    // river above it.
    let crossing = line(&lines, CROSSING);
    assert_eq!(
        (&crossing.inside, crossing.doubted),
        (&Inside::Place(RIVER.to_owned()), false)
    );
    assert!(!line(&lines, RIVER).doubted);

    confirm_as_shown(&mut draft);
    let view = built(&draft, "river");
    assert_eq!(link(&view, FERRY_HOUSE), Inside::Unknown);
    assert!(blocked_by_child(&view), "{:?}", clearance(&view, &[]));
}

/// The Player confirms the true tree, whatever the model said: the ferry
/// house and the inn sit in the crossing. A child in the ferry house is
/// away from the PC in the inn (two places of one crossing), so the gate
/// clears. The same tree with the PC in the ferry house and the child
/// anywhere in the crossing refuses.
#[test]
fn a_player_confirmed_tree_clears_a_sibling_place_and_nothing_more() {
    let all_none = [
        (CROSSING, "none"),
        (RIVER, "none"),
        (FERRY_HOUSE, "none"),
        (INN, "none"),
    ];
    let set_true_tree = |draft: &mut SeedDraft| {
        true_tree(draft);
        confirm_as_shown(draft);
    };

    let mut sibling = drafted(INN, FERRY_HOUSE, &all_none);
    set_true_tree(&mut sibling);
    let view = built(&sibling, "sibling");
    assert_eq!(link(&view, INN), Inside::Place(id(&view, CROSSING)));
    assert!(clearance(&view, &[]).is_ok(), "{:?}", clearance(&view, &[]));

    let mut inside = drafted(FERRY_HOUSE, CROSSING, &all_none);
    set_true_tree(&mut inside);
    let view = built(&inside, "inside");
    assert!(blocked_by_child(&view), "{:?}", clearance(&view, &[]));
}

/// The tree that the facts support, whatever the model said: the crossing
/// on the river, the ferry house and the inn in the crossing.
fn true_tree(draft: &mut SeedDraft) {
    for (place, inside) in [
        (CROSSING, Inside::Place(RIVER.to_owned())),
        (RIVER, Inside::Top),
        (FERRY_HOUSE, Inside::Place(CROSSING.to_owned())),
        (INN, Inside::Place(CROSSING.to_owned())),
    ] {
        let entry = entry(draft, place);
        confirm_inside(draft, &entry, inside).unwrap();
    }
}

/// A `location` fact written in play: a copy of a live fact with a new
/// subject, value and text.
fn play_fact(view: &mut WorldView, subject: &str, value: &str, text: &str) {
    let mut fact = view.live_facts[0].clone();
    fact.id = view.live_facts.iter().map(|f| f.id).max().unwrap() + 1;
    fact.slot.subject = id(view, subject);
    "location".clone_into(&mut fact.slot.key);
    value.clone_into(&mut fact.slot.value);
    text.clone_into(&mut fact.text);
    view.live_facts.push(fact);
}

/// Review M1 (2026-11-16): the gate reads the stored tree through the fact
/// check. The Player confirms the ferry house at top level, and no live
/// fact says otherwise: the child in the crossing is away. Then a fact
/// written in play puts the ferry house in the crossing (QA run 3 wrote
/// that sentence on the ferry). The confirmed link gives way to doubt, and
/// the gate refuses.
#[test]
fn a_play_fact_that_contradicts_a_confirmed_top_makes_the_gate_refuse() {
    let mut draft = drafted(FERRY_HOUSE, CROSSING, &ALL_NONE);
    true_tree(&mut draft);
    let ferry = entry(&draft, FERRY_HOUSE);
    confirm_inside(&mut draft, &ferry, Inside::Top).unwrap();
    confirm_as_shown(&mut draft);
    let mut view = built(&draft, "play-fact");
    assert_eq!(link(&view, FERRY_HOUSE), Inside::Top);
    // The seed's own sentence is left out, so the confirmed tree stands.
    let ferry_item = id(&view, "the ferry");
    view.live_facts
        .retain(|f| !(f.slot.subject == ferry_item && f.slot.key == "location"));
    assert!(clearance(&view, &[]).is_ok(), "{:?}", clearance(&view, &[]));

    play_fact(
        &mut view,
        "the ferry",
        CROSSING,
        "The ferry house is in Brackwater Crossing.",
    );
    assert!(blocked_by_child(&view), "{:?}", clearance(&view, &[]));
}

/// A character's travel names the place it left. "Pell Durrow went from the
/// Lantern Inn to the ferry house." must not read as "the inn is in the
/// ferry house": the gate leaves facts about characters out of the fact
/// check, and a true tree still clears a child in a sibling place.
#[test]
fn a_character_travel_fact_does_not_doubt_a_true_tree() {
    let mut draft = drafted(INN, FERRY_HOUSE, &ALL_NONE);
    true_tree(&mut draft);
    confirm_as_shown(&mut draft);
    let mut view = built(&draft, "travel");
    let child = id(&view, CHILD);
    view.live_facts
        .retain(|f| !(f.slot.subject == child && f.slot.key == "location"));
    play_fact(
        &mut view,
        CHILD,
        FERRY_HOUSE,
        "Pell Durrow went from the Lantern Inn to the ferry house.",
    );
    assert!(clearance(&view, &[]).is_ok(), "{:?}", clearance(&view, &[]));
}

// ---- The review ---------------------------------------------------------------------

/// Approve waits for every Inside Link, as it waits for every age: a doubted
/// link and a proposed one both need the Player.
#[test]
fn the_review_blocks_approval_until_the_tree_is_confirmed() {
    let model = [
        (CROSSING, RIVER),
        (RIVER, "none"),
        (FERRY_HOUSE, CROSSING),
        (INN, "none"),
    ];
    let mut draft = drafted(INN, CROSSING, &model);
    for line in age_lines(&draft) {
        confirm_age(&mut draft, &line.entry()).unwrap();
    }
    let waiting = |draft: &SeedDraft| -> Vec<String> {
        gaps(draft)
            .into_iter()
            .filter(|g| g.kind == GapKind::PlaceUnconfirmed)
            .map(|g| g.at)
            .collect()
    };
    assert_eq!(
        waiting(&draft),
        [
            "entities[1].inside",
            "entities[2].inside",
            "entities[3].inside",
            "entities[4].inside"
        ]
    );
    for line in place_lines(&draft) {
        assert!(approve(&mut draft).is_err());
        assert_eq!(draft.state, DraftState::Draft);
        confirm_inside(&mut draft, &line.entry(), line.inside.clone()).unwrap();
    }
    assert_eq!(waiting(&draft), Vec::<String>::new());
    assert_eq!(approve(&mut draft), Ok(()));

    // A file that says `approved` with a link the Player never confirmed
    // goes back to `Draft`, and builds nothing.
    draft.place_links[0].confirmed = false;
    assert!(approve(&mut draft).is_err());
    assert_eq!(draft.state, DraftState::Draft);
    assert!(confirmed_ages(&draft).is_err());
}

/// The Player's changes reach the Campaign file: a new parent, "not sure",
/// and a rename of the parent, which the link follows.
#[test]
fn review_edits_reach_the_stored_links() {
    let all_none = [
        (CROSSING, "none"),
        (RIVER, "none"),
        (FERRY_HOUSE, "none"),
        (INN, "none"),
    ];
    let mut draft = drafted(INN, CROSSING, &all_none);
    let ferry = entry(&draft, FERRY_HOUSE);
    confirm_inside(&mut draft, &ferry, Inside::Place(CROSSING.to_owned())).unwrap();
    let inn = entry(&draft, INN);
    confirm_inside(&mut draft, &inn, Inside::Unknown).unwrap();
    let crossing = entry(&draft, CROSSING);
    rename(&mut draft, &crossing, "Brackwater").unwrap();
    let line = line(&place_lines(&draft), FERRY_HOUSE).clone();
    assert_eq!(
        (line.inside, line.confirmed),
        (Inside::Place("Brackwater".to_owned()), true)
    );
    // The fact check doubted the crossing (it lies on the river); the
    // Player puts it at top level.
    let renamed = entry(&draft, "Brackwater");
    confirm_inside(&mut draft, &renamed, Inside::Top).unwrap();
    confirm_as_shown(&mut draft);

    let view = built(&draft, "edits");
    assert_eq!(
        link(&view, FERRY_HOUSE),
        Inside::Place(id(&view, "Brackwater"))
    );
    assert_eq!(link(&view, INN), Inside::Unknown);
    assert_eq!(link(&view, "Brackwater"), Inside::Top);
}

/// A confirmation is a review edit (review minor, 2026-11-16): on a built
/// draft, `confirm_inside` and `confirm_age` return it to `Draft` and drop
/// the old Campaign file, so the review no longer says "Change something
/// to build a new one".
#[test]
fn a_confirmation_on_a_built_draft_is_an_edit() {
    let mut draft = drafted(INN, CROSSING, &ALL_NONE);
    true_tree(&mut draft);
    confirm_as_shown(&mut draft);
    let as_built = |draft: &mut SeedDraft| {
        draft.state = DraftState::Built;
        draft.campaign_file = Some("campaign.sqlite".to_owned());
    };

    as_built(&mut draft);
    let inn = entry(&draft, INN);
    confirm_inside(&mut draft, &inn, Inside::Unknown).unwrap();
    assert_eq!(
        (draft.state, draft.campaign_file.as_deref()),
        (DraftState::Draft, None)
    );

    as_built(&mut draft);
    let child = age_lines(&draft)
        .into_iter()
        .find(|l| l.name == CHILD)
        .unwrap();
    confirm_age(&mut draft, &child.entry()).unwrap();
    assert_eq!(
        (draft.state, draft.campaign_file.as_deref()),
        (DraftState::Draft, None)
    );
}

/// `confirm_inside` takes a Place, and a parent that is another Place of
/// the draft. It refuses a Place inside itself and a loop; the draft does
/// not change.
#[test]
fn confirm_inside_refuses_a_loop_and_a_parent_that_is_no_place() {
    let model = [
        (CROSSING, "none"),
        (RIVER, "none"),
        (FERRY_HOUSE, CROSSING),
        (INN, "none"),
    ];
    let mut draft = drafted(INN, CROSSING, &model);
    let before = draft.clone();
    let crossing = entry(&draft, CROSSING);
    let ferry = entry(&draft, FERRY_HOUSE);
    assert_eq!(
        confirm_inside(&mut draft, &crossing, Inside::Place(CROSSING.to_owned())),
        Err(ReviewError::Loop)
    );
    assert_eq!(
        confirm_inside(&mut draft, &crossing, Inside::Place(FERRY_HOUSE.to_owned())),
        Err(ReviewError::Loop)
    );
    for parent in ["the ferry", CHILD, "Nowhere"] {
        assert_eq!(
            confirm_inside(&mut draft, &ferry, Inside::Place(parent.to_owned())),
            Err(ReviewError::UnknownName),
            "{parent}"
        );
    }
    let child = EntryRef::entity(0, CHILD, EntityKind::Npc);
    assert_eq!(
        confirm_inside(&mut draft, &child, Inside::Top),
        Err(ReviewError::UnknownName)
    );
    let stale = EntryRef::entity(1, "Old name", EntityKind::Place);
    assert_eq!(
        confirm_inside(&mut draft, &stale, Inside::Top),
        Err(ReviewError::UnknownName)
    );
    assert_eq!(draft, before);
}

/// A save that renames a Place or its parent by hand drops the
/// confirmation: the link names the old entry, so the Player chooses again.
#[test]
fn an_edit_that_renames_a_place_asks_for_a_new_choice() {
    let model = [
        (CROSSING, "none"),
        (RIVER, "none"),
        (FERRY_HOUSE, CROSSING),
        (INN, "none"),
    ];
    let mut draft = drafted(INN, CROSSING, &model);
    confirm_as_shown(&mut draft);
    let mut seed = draft.seed.clone();
    let crossing = seed
        .entities
        .iter_mut()
        .find(|e| e.name == CROSSING)
        .unwrap();
    crossing.name = "Brackwater".into();
    let knowledge = draft.knowledge.clone();
    apply_edit(&mut draft, seed, knowledge);
    let lines = place_lines(&draft);
    let ferry = line(&lines, FERRY_HOUSE);
    assert_eq!((&ferry.inside, ferry.confirmed), (&Inside::Unknown, false));
    assert!(!line(&lines, "Brackwater").confirmed);
    assert!(line(&lines, INN).confirmed);
}

// ---- QA run 4: the Source Text backs a link --------------------------------------

/// The Source Text of QA run 4, as the Player pasted it.
const QA4_TEXT: &str = "The Lantern Road — campaign notes

Brindle Ford is a small river village. Three places in the village matter. The Lantern Inn is a two-storey inn on the village green of Brindle Ford, with rooms upstairs. The ferry house sits at the river's edge in Brindle Ford, where the ferry crosses. The Cooper house is a narrow cottage on the lane behind the green, also in Brindle Ford.

Wexmoor is a market town a day's ride north of Brindle Ford. The Salt Exchange is a stone hall in Wexmoor where merchants trade.

Edric Vale (35) is the player character, a courier carrying a sealed message to Wexmoor. He travels with his wife, Ilse Vale (33), a herbalist. They are both adults and have a room upstairs at the Lantern Inn. Tonight they are at the Lantern Inn.

Marta Cooper (41) is a widow who keeps the Cooper house. Her son, Tobin Cooper, is 10 years old. Tobin lives with his mother at the Cooper house and is at home there tonight.

Old Hal (62) runs the ferry and lives in the ferry house. He says the river is too high to cross until morning.

Rook Danner (50) is a salt merchant who waits at the Salt Exchange in Wexmoor for Edric's message. Someone in Brindle Ford wants that message to never arrive.
";

/// The draft of QA run 4 (`draft-1794841722723334641`): the live model's
/// roster, its known facts (every subject is the PC or an NPC, and no key
/// is `location`), and its place tree.
fn qa4_drafted() -> SeedDraft {
    let person = |name: &str, age: u16| json!({"name": name, "age": age, "age_stated": true, "other_names": []});
    let place = |name: &str| json!({"name": name, "kind": "place", "other_names": []});
    let roster = json!({
        "pc": {
            "name": "Edric Vale", "age": 35, "age_stated": true,
            "other_names": [], "concept": "A courier carrying a sealed message to Wexmoor"
        },
        "characters": [
            person("Ilse Vale", 33),
            person("Tobin Cooper", 10),
            person("Old Hal", 62),
            person("Marta Cooper", 41),
            person("Rook Danner", 50),
        ],
        "others": [
            place("Brindle Ford"),
            place("Lantern Inn"),
            place("ferry house"),
            place("Cooper house"),
            place("Wexmoor"),
            place("Salt Exchange"),
        ]
    });
    let mut seed = seed_out("Lantern Inn");
    seed["start"]["present"] = json!(["Ilse Vale"]);
    let known = |subject: &str, key: &str, value: &str, text: &str| {
        json!({
            "subject": subject, "key": key, "value": value, "text": text,
            "open": false, "pc_knows": true
        })
    };
    let knowledge = json!({"facts": [
        known("Edric Vale", "occupation", "courier", "Edric Vale is a courier."),
        known("Edric Vale", "spouse", "Ilse Vale", "Edric Vale travels with his wife, Ilse Vale."),
        known("Edric Vale", "current_location", "Lantern Inn", "Edric Vale and Ilse Vale are staying at the Lantern Inn in Brindle Ford."),
        known("Edric Vale", "recipient", "Rook Danner", "Rook Danner is waiting at the Salt Exchange in Wexmoor for Edric's message."),
        known("Edric Vale", "village_description", "small river village", "Brindle Ford is a small river village."),
        known("Edric Vale", "location_description", "narrow cottage on the lane behind the de", "The Cooper house is a narrow cottage on the lane behind the green, also in Brindle Ford."),
        known("Edric Vale", "destination_info", "market town a day's ride north", "Wexmoor is a market town a day's ride north of Brindle Ford."),
        known("Edric Vale", "destination_location", "stone hall in Wexmoor", "The Salt Exchange is a stone hall in Wexmoor where merchants trade."),
        known("Edric Vale", "neighbor_info", "at home tonight", "Tobin Cooper is at home tonight."),
        known("Edric Vale", "ferry_operator", "runs the ferry, lives in ferry house", "Old Hal runs the ferry and lives in the ferry house."),
    ]});
    let tree = tree_out(&[
        ("Brindle Ford", "none"),
        ("Lantern Inn", "Brindle Ford"),
        ("ferry house", "Brindle Ford"),
        ("Cooper house", "Brindle Ford"),
        ("Wexmoor", "none"),
        ("Salt Exchange", "Wexmoor"),
    ]);
    let gm = gm(&[roster, seed, knowledge, tree]);
    let mut progress = |_| {};
    let mut log = |_| {};
    let mut ctl = Control::new(&mut progress, &|| false, &mut log);
    let draft = draft_from_text(QA4_TEXT, &gm, &mut ctl).unwrap();
    assert_eq!(gm.calls().len(), 4);
    draft
}

/// QA run 4: all six links started on "Choose…", even the ones the Source
/// Text states ("The Cooper house is ... also in Brindle Ford."). The live
/// model wrote no `location` fact: it gave every known fact to the PC, under
/// keys such as `destination_location`. A sentence of the Source Text that
/// names the Place and the proposed parent, and no other Place, now backs
/// the link. A top-level guess is never backed.
#[test]
fn a_sentence_of_the_source_text_backs_a_link() {
    let draft = qa4_drafted();
    let lines = place_lines(&draft);
    let place = |p: &str| Inside::Place(p.to_owned());
    for (name, parent) in [
        ("Lantern Inn", "Brindle Ford"),
        ("ferry house", "Brindle Ford"),
        ("Cooper house", "Brindle Ford"),
        ("Salt Exchange", "Wexmoor"),
    ] {
        let l = line(&lines, name);
        assert_eq!(
            (&l.inside, l.backed, l.doubted, l.confirmed),
            (&place(parent), true, false, false),
            "{name}"
        );
    }
    for name in ["Brindle Ford", "Wexmoor"] {
        let l = line(&lines, name);
        assert_eq!(
            (&l.inside, l.backed, &l.suggested),
            (&Inside::Unknown, false, &Some(Inside::Top)),
            "{name}"
        );
    }
    // Backed or not, every Place still waits for the Player.
    let waiting = gaps(&draft)
        .into_iter()
        .filter(|g| g.kind == GapKind::PlaceUnconfirmed)
        .count();
    assert_eq!(waiting, 6);
}

/// A sentence backs only the link it states: the Place and the parent,
/// and no third Place. "Edda Marsh keeps the ferry house in Brackwater
/// Crossing, on the river Fenn." names three Places, so it backs neither
/// "the ferry house in the Fenn" nor "the ferry house in the crossing";
/// a guess the text never states stays a hint.
#[test]
fn a_sentence_that_names_a_third_place_backs_nothing() {
    let model = [
        (CROSSING, "none"),
        (RIVER, "none"),
        (FERRY_HOUSE, RIVER),
        (INN, CROSSING),
    ];
    let mut gm_outputs = vec![roster_out(), seed_out(INN)];
    // No known fact: only the Source Text could back a link.
    gm_outputs.push(json!({"facts": []}));
    gm_outputs.push(tree_out(&model));
    let gm = gm(&gm_outputs);
    let mut progress = |_| {};
    let mut log = |_| {};
    let mut ctl = Control::new(&mut progress, &|| false, &mut log);
    let draft = draft_from_text(&source(), &gm, &mut ctl).unwrap();
    let lines = place_lines(&draft);
    for name in [FERRY_HOUSE, INN] {
        let l = line(&lines, name);
        assert_eq!((&l.inside, l.backed), (&Inside::Unknown, false), "{name}");
    }
}

/// A sentence of the Source Text never hides a fact that contradicts the
/// link. The text says the inn stands on the river; a known fact puts it in
/// the crossing. The model's "the inn in the Fenn" is doubted, not backed.
#[test]
fn a_source_sentence_does_not_hide_a_contradicting_fact() {
    let mut text = String::from("# The crossing\n\n");
    while text.chars().count() < 450 {
        text.push_str("The Lantern Inn stands on the river Fenn. ");
    }
    let model = [
        (CROSSING, "none"),
        (RIVER, "none"),
        (FERRY_HOUSE, "none"),
        (INN, RIVER),
    ];
    let gm = gm(&[
        roster_out(),
        seed_out(INN),
        json!({"facts": [fact("the ferry", CROSSING, "The Lantern Inn is in Brackwater Crossing.")]}),
        tree_out(&model),
    ]);
    let mut progress = |_| {};
    let mut log = |_| {};
    let mut ctl = Control::new(&mut progress, &|| false, &mut log);
    let draft = draft_from_text(&text, &gm, &mut ctl).unwrap();
    let lines = place_lines(&draft);
    let inn = line(&lines, INN);
    assert_eq!(
        (&inn.inside, inn.doubted, inn.backed),
        (&Inside::Unknown, true, false)
    );
}
