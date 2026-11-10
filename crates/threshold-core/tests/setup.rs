//! `setup`: a `WorldSetup` becomes the `__world__` Setup Turn, and a
//! Campaign file appears only once it is complete.

use std::{
    fs,
    path::{Path, PathBuf},
};

mod support;

use support::{e, fact, proposal, slot};
use threshold_core::{
    checks::{CheckKind, validate},
    domain::{AdventureSeed, Canon, FactSource, Seen, Verdict, WorldSetup},
    fixture::load_adventure_seed,
    setup::{SetupError, WORLD_OP_ID, commit_world, create_campaign_file},
    store::{CampaignId, Store, WorldView},
    turn::create_campaign,
    world::{beliefs, pc_knowledge},
};

fn town_seed() -> AdventureSeed {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/town");
    load_adventure_seed(&dir.join("adventure-seed.toml")).unwrap()
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

/// An empty folder of its own under the system temp dir, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("threshold-setup-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn files(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(&self.0)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn world_toml_facts_section_commits() {
    let (mut store, campaign) = campaign(&town_seed());
    let setup = world(
        r#"
[[facts]]
subject = "Orsel"
key = "ledger_place"
value = "under the archive floor"
text = "Orsel hides the true tithe ledger under the archive floor."
known_by = ["Hessa"]

[[facts]]
subject = "Doul"
key = "plan"
value = "search the scribe's room"
text = "Doul plans to search the scribe's room."
open = true
"#,
    );
    assert!(!setup.facts[0].open);
    assert!(setup.facts[1].known_by.is_empty());

    commit_world(&mut store, campaign, &setup).unwrap();

    let view = store.snapshot(campaign).unwrap();
    let (orsel, doul, hessa) = (id(&view, "Orsel"), id(&view, "Doul"), id(&view, "Hessa"));
    let ledger = view.live_fact(orsel, "ledger_place").unwrap();
    assert_eq!(ledger.source, FactSource::AdventureSeed);
    assert_eq!(ledger.seen_by, [hessa]);
    assert!(
        beliefs(&view, hessa)
            .iter()
            .any(|b| b.key == "ledger_place")
    );
    assert!(pc_knowledge(&view).iter().all(|b| b.key != "ledger_place"));

    // No `known_by`: GM only. An open entry commits on its `pending_` key.
    assert!(view.live_fact(doul, "plan").is_none());
    let plan = view.live_fact(doul, "pending_plan").unwrap();
    assert!(plan.seen_by.is_empty());
    assert!(view.knowledge.iter().all(|k| k.fact_id != Some(plan.id)));
}

#[test]
fn only_a_fact_the_pc_holds_commits_as_persistent() {
    let (mut store, campaign) = campaign(&town_seed());
    let setup = world(
        r#"
[[facts]]
subject = "Orsel"
key = "ledger_place"
value = "under the archive floor"
text = "Orsel hides the true tithe ledger under the archive floor."
known_by = ["Hessa"]

[[facts]]
subject = "Doul"
key = "plan"
value = "search the scribe's room"
text = "Doul plans to search the scribe's room."

[[facts]]
subject = "Hessa"
key = "trade"
value = "archivist"
text = "Hessa keeps the tithe archive."
known_by = ["Wren", "Hessa"]

[[facts]]
subject = "Tobin"
key = "errand"
value = "fetch the seal"
text = "Tobin will fetch the seal."
known_by = ["Wren"]
open = true
"#,
    );
    commit_world(&mut store, campaign, &setup).unwrap();

    let view = store.snapshot(campaign).unwrap();
    let canon = |name: &str, key: &str| view.live_fact(id(&view, name), key).unwrap().canon;
    // A hidden fact and a secret stay open to change in play.
    assert_eq!(canon("Orsel", "ledger_place"), Canon::Established);
    assert_eq!(canon("Doul", "plan"), Canon::Established);
    // A settled fact the PC knows is Persistent; an open item never is.
    assert_eq!(canon("Hessa", "trade"), Canon::Persistent);
    assert_eq!(canon("Tobin", "pending_errand"), Canon::Established);

    // The PC talks Doul out of his plan: the play write is no bend.
    let doul = id(&view, "Doul");
    let mut p = proposal(Verdict::Allowed);
    p.facts = vec![fact(
        slot(e(doul), "plan", "leave the scribe alone"),
        Seen::Pc,
        vec![],
    )];
    if let Err(errs) = validate(&p, &view) {
        panic!("{errs:?}");
    }
    // A settled fact the PC knows still cannot bend.
    let hessa = id(&view, "Hessa");
    p.facts = vec![fact(slot(e(hessa), "trade", "baker"), Seen::Pc, vec![])];
    let errs = validate(&p, &view).unwrap_err();
    assert!(
        errs.iter().any(|r| r.check == CheckKind::FactBends),
        "{errs:?}"
    );
}

#[test]
fn an_event_effect_on_an_open_key_is_refused() {
    let (mut store, campaign) = campaign(&town_seed());
    let before = store.snapshot(campaign).unwrap();
    let setup = world(
        r#"
[[events]]
at = 60
actor = "Doul"
text = "Doul files a motion."

[[events.effects]]
subject = "Doul"
key = "pending_motion"
value = "filed"
"#,
    );
    let result = commit_world(&mut store, campaign, &setup);
    assert!(matches!(result, Err(SetupError::Invalid(_))), "{result:?}");
    assert_eq!(store.snapshot(campaign).unwrap(), before);
}

#[test]
fn unknown_name_fails_and_writes_nothing() {
    let (mut store, campaign) = campaign(&town_seed());
    let before = store.snapshot(campaign).unwrap();
    let setup = world(
        r#"
[[facts]]
subject = "Orsel"
key = "ledger_place"
value = "under the archive floor"
text = "Orsel hides the true tithe ledger under the archive floor."

[[attitudes]]
from = "Orsel"
to = "Nobody"
attitude = -2
"#,
    );

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

#[test]
fn campaign_file_appears_only_when_complete() {
    let seed = town_seed();
    let good = world(
        r#"
[[attitudes]]
from = "Orsel"
to = "Wren"
attitude = -1
"#,
    );

    let dir = TempDir::new("complete");
    let path = dir.0.join("campaign-1.sqlite");
    create_campaign_file(&path, &seed, &good, 11).unwrap();
    assert!(path.is_file());
    assert!(!dir.0.join("campaign-1.sqlite.part").exists());
    // No journal or other side file stays behind.
    assert_eq!(dir.files(), ["campaign-1.sqlite"]);
    let store = Store::open_existing(&path).unwrap();
    let campaigns = store.campaigns().unwrap();
    assert_eq!(campaigns.len(), 1);
    assert!(
        store
            .turn_by_campaign_op(campaigns[0], &WORLD_OP_ID.into())
            .unwrap()
            .is_some()
    );
    // A Campaign file is never overwritten.
    assert!(create_campaign_file(&path, &seed, &good, 12).is_err());
    assert_eq!(dir.files(), ["campaign-1.sqlite"]);

    let bad = world(
        r#"
[[attitudes]]
from = "Orsel"
to = "Nobody"
attitude = -1
"#,
    );
    let dir = TempDir::new("failed");
    let path = dir.0.join("campaign-2.sqlite");
    let result = create_campaign_file(&path, &seed, &bad, 11);
    assert!(matches!(result, Err(SetupError::Invalid(_))), "{result:?}");
    assert!(!path.exists());
    assert!(!dir.0.join("campaign-2.sqlite.part").exists());
    assert!(dir.files().is_empty(), "{:?}", dir.files());
}

#[test]
fn names_resolve_through_other_names() {
    let seed: AdventureSeed = toml::from_str(
        r#"
premise = "A ferry crossing at night"
boundaries = []
decided = []
proposed = []
story_language = "en"

[pc]
name = "Wren"
age = 29
concept = "A courier"

[[entities]]
name = "Jean"
kind = "npc"
age = 40
other_names = [{ name = "Jade", known_to = ["Wren"] }]
"#,
    )
    .unwrap();
    let (mut store, campaign) = campaign(&seed);
    let setup = world(
        r#"
[[facts]]
subject = "Jade"
key = "debt"
value = "the ferryman"
text = "Jade owes the ferryman."
known_by = ["Jade"]

[[attitudes]]
from = "Jade"
to = "Wren"
attitude = 2

[[goals]]
npc = "Jade"
text = "Cross before dawn"

[[voice_cards]]
npc = "Jade"
habits = "Quiet"
vocabulary = "tide, fare"
samples = ["The tide waits for no one.", "Fare first."]
"#,
    );

    commit_world(&mut store, campaign, &setup).unwrap();

    let view = store.snapshot(campaign).unwrap();
    let jean = id(&view, "Jean");
    let debt = view.live_fact(jean, "debt").unwrap();
    assert_eq!(debt.seen_by, [jean]);
    assert!(
        view.attitudes
            .iter()
            .any(|a| a.from == jean && a.attitude == 2)
    );
    assert!(view.goals_active.iter().any(|g| g.npc_id == jean));
    assert!(view.voice_card(jean).is_some());
}
