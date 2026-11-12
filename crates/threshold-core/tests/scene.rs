//! The Intimate Scene (mature content module, Task 4): the scene state the
//! ledger keeps, the automatic endings of data shape 3.4, the gate before
//! the proposal call (G1), and what each prompt may read of a scene.
//! Neutral fixtures and marker words only.

mod support;

use std::{
    fs,
    path::{Path, PathBuf},
};

use support::*;
use threshold_core::{
    CallKind, RecordedCall, RecordedGm,
    checks::{CheckKind, validate},
    context::{IMPLIED_LINE, SCENE_LINE, opening_prompt},
    domain::{
        ActionKind, Canon, DecisionProposal, DetailLevel, Effect, EntityKind, EntityRef,
        FactSource, LedgerOp, SCENE_PLAN_MAX_CHARS, SceneWhy, Slot, Stance, Trigger, Verdict,
    },
    mature::{INTIMATE_TEMPLATE, SCENE_NARRATION_MAX_TOKENS, neutral_line, stored_intimate},
    rules::D10,
    store::{Store, StoredTurn, TurnId, TurnKind, TurnRecord, WorldView},
    turn::{Engine, TurnError, TurnOutcome, create_campaign},
};

/// A marker word in a scene Narration: it must never reach a prompt that
/// holds no clearance.
const MARKER: &str = "MARKER-SCENE-1";

/// The gate world at `implied` in a Campaign file, played through an
/// `Engine`. Ada, Orsel and Brann are confirmed, and so is Vale (40), an
/// NPC the Adventure Seed hides, on the far road. `edit` commits raw ops as
/// another program would, between two Turns.
struct Rig {
    path: PathBuf,
    campaign: u64,
    gm: RecordedGm,
    engine: Option<Engine>,
    ada: u64,
    brann: u64,
    vale: u64,
    edits: u32,
}

impl Rig {
    fn new(name: &str) -> Self {
        let path = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("scene-{name}-{}.sqlite", std::process::id()));
        let _ = fs::remove_file(&path);
        let mut store = Store::open(&path).unwrap();
        let mut seed = adventure_seed();
        seed.detail = DetailLevel::Implied;
        let mut vale = npc("Vale", 40);
        vale.hidden = true;
        seed.entities.push(vale);
        seed.decided.push(seed_fact("Vale", "location", "far road"));
        let campaign = create_campaign(&mut store, &seed, 7, None).unwrap();
        let view = store.snapshot(campaign).unwrap();
        let id = |name: &str| view.entity_by_name(name).unwrap().id;
        let (ada, orsel, brann, vale) = (id("Ada"), id("Orsel"), id("Brann"), id("Vale"));
        let gm = RecordedGm::new(vec![], vec![]);
        let mut rig = Self {
            path,
            campaign,
            gm,
            engine: None,
            ada,
            brann,
            vale,
            edits: 0,
        };
        rig.commit_raw(
            &mut store,
            [ada, orsel, brann, vale]
                .into_iter()
                .map(|id| LedgerOp::ConfirmAge {
                    entity: EntityRef::Id(id),
                })
                .collect(),
        );
        rig.engine = Some(rig.make_engine(store));
        rig
    }

    fn make_engine(&self, store: Store) -> Engine {
        Engine::new(
            store,
            Box::new(self.gm.clone()),
            Box::new(D10),
            vec![],
            self.campaign,
        )
    }

    fn commit_raw(&mut self, store: &mut Store, ops: Vec<LedgerOp>) {
        self.edits += 1;
        let clock = store.snapshot(self.campaign).unwrap().clock_min;
        store
            .commit(
                TurnRecord {
                    campaign_id: self.campaign,
                    op_id: format!("edit-{}", self.edits),
                    kind: TurnKind::Setup,
                    intent: String::new(),
                    proposal_json: "{}".into(),
                    clock_before: clock,
                    clock_after: clock,
                },
                ops,
            )
            .unwrap();
    }

    fn engine(&mut self) -> &mut Engine {
        self.engine.as_mut().unwrap()
    }

    fn store(&self) -> &Store {
        self.engine.as_ref().unwrap().store()
    }

    fn view(&self) -> WorldView {
        self.store().snapshot(self.campaign).unwrap()
    }

    /// Another program's edit: the Engine lets go of the file, the ops are
    /// committed in a Turn of their own, and a new Engine opens it again.
    fn edit(&mut self, ops: Vec<LedgerOp>) {
        drop(self.engine.take());
        let mut store = Store::open(&self.path).unwrap();
        self.commit_raw(&mut store, ops);
        self.engine = Some(self.make_engine(store));
    }

    /// Closes the file and opens it again, as a restart of the app does.
    fn restart(&mut self) {
        drop(self.engine.take());
        let store = Store::open(&self.path).unwrap();
        self.engine = Some(self.make_engine(store));
    }

    /// Plays one Turn whose proposal is `p` and whose Narration is
    /// `narration`.
    fn play(
        &mut self,
        op: &str,
        p: &DecisionProposal,
        narration: &str,
    ) -> Result<TurnOutcome, TurnError> {
        self.gm.push_proposal(serde_json::to_string(p).unwrap());
        self.gm.push_narration(narration.into());
        self.engine()
            .play_turn(op.into(), "I stay with Brann.", &mut |_| {})
    }

    fn played(&mut self, op: &str, p: &DecisionProposal, narration: &str) -> TurnId {
        self.play(op, p, narration).unwrap().turn_id
    }

    fn scene(&self) -> Option<TurnId> {
        self.view().scene_open
    }

    fn turn(&self, id: TurnId) -> StoredTurn {
        self.store().turn(id).unwrap().unwrap()
    }

    /// The `SetScene` ops a Turn committed.
    fn scene_ops(&self, id: TurnId) -> Vec<(bool, SceneWhy)> {
        self.store()
            .turn_ops(id)
            .unwrap()
            .into_iter()
            .filter_map(|op| match op {
                LedgerOp::SetScene { open, why } => Some((open, why)),
                _ => None,
            })
            .collect()
    }

    /// The calls of one kind the model received, in order.
    fn calls(&self, kind: CallKind) -> Vec<RecordedCall> {
        self.gm
            .calls()
            .into_iter()
            .filter(|c| c.kind == kind)
            .collect()
    }

    fn last_call(&self, kind: CallKind) -> RecordedCall {
        self.calls(kind).pop().expect("a call of this kind")
    }

    /// Adds an NPC at `place` with a raw op, as another program would.
    fn add_npc(&mut self, name: &str, age: u16, confirmed: bool, place: &str) -> u64 {
        self.edit(vec![
            LedgerOp::AddEntity {
                local: Some(1),
                kind: EntityKind::Npc,
                name: name.into(),
                age: Some(age),
                other_names: vec![],
                age_confirmed: confirmed,
            },
            LedgerOp::AddFact {
                slot: Slot {
                    subject: EntityRef::New(1),
                    key: "location".into(),
                    value: place.into(),
                },
                text: format!("{name} stands at {place}."),
                canon: Canon::Established,
                source: FactSource::Proposal,
                seen_by: vec![],
                causes: vec![],
            },
        ]);
        self.view().entity_by_name(name).unwrap().id
    }

    /// A World Event due now that brings `subject` to the gate square. The
    /// PC there sees the arrival unless the Adventure Seed hides `subject`
    /// (`world::pc_witnesses`).
    fn arrival(&mut self, subject: u64) {
        let at = self.view().clock_min;
        self.edit(vec![LedgerOp::ScheduleEvent {
            text: "Someone comes to the gate square.".into(),
            trigger: Trigger::At(at),
            actors: vec![EntityRef::Id(subject)],
            goal_id: None,
            cause_fact_id: None,
            effects: vec![Effect::SetFact {
                slot: Slot {
                    subject: EntityRef::Id(subject),
                    key: "location".into(),
                    value: "gate square".into(),
                },
                text: "They are at the gate square.".into(),
                seen_by: vec![],
            }],
            seen_by: vec![],
            public: false,
        }]);
    }

    /// Adds a place the PC can name, for a move.
    fn place(&mut self, name: &str) -> u64 {
        let ada = self.ada;
        self.edit(vec![LedgerOp::AddEntity {
            local: None,
            kind: EntityKind::Place,
            name: name.into(),
            age: None,
            other_names: vec![],
            age_confirmed: false,
        }]);
        let id = self.view().entity_by_name(name).unwrap().id;
        self.edit(vec![LedgerOp::AddFact {
            slot: Slot {
                subject: EntityRef::Id(id),
                key: "kind".into(),
                value: "yard".into(),
            },
            text: format!("{name} is a yard."),
            canon: Canon::Established,
            source: FactSource::Proposal,
            seen_by: vec![EntityRef::Id(ada)],
            causes: vec![],
        }]);
        id
    }

    /// An intimate proposal: Brann complies.
    fn intimate(&self) -> DecisionProposal {
        let mut p = proposal(Verdict::Allowed);
        p.intimate = true;
        p.minutes = 5;
        p.reactions = vec![reaction(e(self.brann), Stance::Comply, None)];
        p
    }

    /// A plain proposal of five minutes.
    fn plain(&self) -> DecisionProposal {
        let mut p = proposal(Verdict::Allowed);
        p.minutes = 5;
        p
    }

    /// An intimate proposal that moves the PC to `place`.
    fn intimate_move(&self, place: u64) -> DecisionProposal {
        let mut p = self.intimate();
        p.action = ActionKind::Move;
        p.move_to = Some(e(place));
        p
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        drop(self.engine.take());
        let _ = fs::remove_file(&self.path);
    }
}

fn prompt_text(call: &RecordedCall) -> String {
    let p = &call.prompt;
    format!("{}{}{}{}", p.system, p.stable, p.scene, p.recent)
}

// --- Data shape 3.4, one test per row -----------------------------------------

/// Row 1: a closed scene, an intimate Turn that passes G2 and G3.
#[test]
fn an_intimate_turn_opens_a_scene() {
    let mut rig = Rig::new("opens");
    assert_eq!(rig.scene(), None);
    let p = rig.intimate();
    let t1 = rig.played("t1", &p, "Brann nods.");
    assert_eq!(rig.scene(), Some(t1));
    assert_eq!(rig.scene_ops(t1), [(true, SceneWhy::Opened)]);
}

/// Row 2: a closed scene, a plain Turn.
#[test]
fn a_plain_turn_opens_no_scene() {
    let mut rig = Rig::new("plain");
    let p = rig.plain();
    let t1 = rig.played("t1", &p, "Dusk falls.");
    assert_eq!(rig.scene(), None);
    assert!(rig.scene_ops(t1).is_empty());
}

/// Row 3: an open scene, a plain Turn.
#[test]
fn a_plain_turn_closes_the_scene() {
    let mut rig = Rig::new("closes");
    let p = rig.intimate();
    rig.played("t1", &p, "Brann nods.");
    let p = rig.plain();
    let t2 = rig.played("t2", &p, "Dusk falls.");
    assert_eq!(rig.scene(), None);
    assert_eq!(rig.scene_ops(t2), [(false, SceneWhy::NotIntimate)]);
}

/// Row 4: an open scene, an intimate Turn in which the PC moves.
#[test]
fn a_move_closes_the_scene() {
    let mut rig = Rig::new("move");
    let yard = rig.place("Mill Yard");
    let p = rig.intimate();
    rig.played("t1", &p, "Brann nods.");
    let p = rig.intimate_move(yard);
    let t2 = rig.played("t2", &p, "The yard is quiet.");
    assert_eq!(rig.scene(), None);
    assert_eq!(rig.scene_ops(t2), [(false, SceneWhy::Moved)]);
}

/// Row 5: an open scene, an intimate Turn in which a character the PC sees
/// arrives.
#[test]
fn an_arrival_the_pc_sees_closes_the_scene() {
    let mut rig = Rig::new("arrival");
    let kett = rig.add_npc("Kett", 34, true, "far road");
    let p = rig.intimate();
    rig.played("t1", &p, "Brann nods.");
    rig.arrival(kett);
    let p = rig.intimate();
    let t2 = rig.played("t2", &p, "Someone comes.");
    assert!(
        threshold_core::context::seen_present(&rig.view()).contains(&kett),
        "the PC sees Kett"
    );
    assert_eq!(rig.scene(), None);
    assert_eq!(rig.scene_ops(t2), [(false, SceneWhy::Arrival)]);
}

/// Row 6: an open scene, an intimate Turn with no move and no arrival.
#[test]
fn an_intimate_turn_keeps_the_scene_open() {
    let mut rig = Rig::new("keeps");
    let p = rig.intimate();
    let t1 = rig.played("t1", &p, "Brann nods.");
    let t2 = rig.played("t2", &p, "Brann stays.");
    assert_eq!(rig.scene(), Some(t1));
    assert!(rig.scene_ops(t2).is_empty());
}

/// "Another character arrives" reads what the PC perceives: a confirmed
/// adult who arrives unseen does not end the scene. The age rule reads
/// world truth, and the adult passes it.
#[test]
fn a_hidden_adult_arrival_keeps_the_scene_open() {
    let mut rig = Rig::new("hidden-arrival");
    let vale = rig.vale;
    let p = rig.intimate();
    let t1 = rig.played("t1", &p, "Brann nods.");
    rig.arrival(vale);
    let t2 = rig.played("t2", &p, "Brann stays.");
    let view = rig.view();
    assert!(view.present().contains(&vale), "Vale is here");
    assert!(!threshold_core::context::seen_present(&view).contains(&vale));
    assert_eq!(rig.scene(), Some(t1));
    assert!(rig.scene_ops(t2).is_empty());
}

/// A Turn that closes a scene never opens one: the next intimate Turn
/// opens a new scene.
#[test]
fn a_turn_that_closes_a_scene_never_opens_one() {
    let mut rig = Rig::new("never-opens");
    let yard = rig.place("Mill Yard");
    let p = rig.intimate();
    rig.played("t1", &p, "Brann nods.");
    let p = rig.intimate_move(yard);
    let t2 = rig.played("t2", &p, "The yard is quiet.");
    assert_eq!(rig.scene_ops(t2), [(false, SceneWhy::Moved)]);
    assert_eq!(
        rig.scene(),
        None,
        "an intimate Turn that closes stays closed"
    );
    let p = rig.intimate();
    let t3 = rig.played("t3", &p, "Brann follows.");
    assert_eq!(rig.scene(), Some(t3), "a new scene");
    assert_eq!(rig.scene_ops(t3), [(true, SceneWhy::Opened)]);
}

// --- G1 -----------------------------------------------------------------------

/// G1: another program brings an unconfirmed NPC into the place of an open
/// scene. The next Turn is planned as a plain one: its proposal prompt holds
/// no scene line and no scene Narration, and its Commit closes the scene
/// with `why: Gate`.
#[test]
fn a_scene_that_is_not_cleared_is_closed_before_the_proposal_call() {
    let mut rig = Rig::new("g1");
    let p = rig.intimate();
    let t1 = rig.played("t1", &p, &format!("{MARKER} Brann nods."));
    assert_eq!(rig.scene(), Some(t1));
    rig.add_npc("Kett", 34, false, "gate square");
    let p = rig.plain();
    let t2 = rig.played("t2", &p, "Dusk falls.");
    let call = rig.last_call(CallKind::Propose);
    let text = prompt_text(&call);
    assert!(!text.contains(SCENE_LINE), "{}", call.prompt.recent);
    assert!(!text.contains(MARKER), "{}", call.prompt.recent);
    let line = neutral_line(&rig.turn(t1)).unwrap();
    assert!(call.prompt.recent.contains(&line), "{}", call.prompt.recent);
    assert_eq!(rig.scene_ops(t2), [(false, SceneWhy::Gate)]);
    assert_eq!(rig.scene(), None);
}

/// G1 in a Correction: the Correction plans on the state before the Turn it
/// corrects, through the same gate as a Play Turn.
#[test]
fn a_correction_in_a_scene_runs_g1() {
    // The state before the corrected Turn holds a cleared open scene: the
    // Correction's prompt holds the scene line and the scene Turn in full.
    let mut rig = Rig::new("g1-correction");
    let p = rig.intimate();
    rig.played("t1", &p, &format!("{MARKER} Brann nods."));
    let plain = rig.plain();
    rig.played("t2", &plain, "Dusk.");
    assert_eq!(rig.scene(), None, "the plain Turn closed the scene");
    rig.gm.push_proposal(serde_json::to_string(&plain).unwrap());
    rig.gm.push_narration("Dusk again.".into());
    let c = rig
        .engine()
        .correct_last("c1".into(), "I wait.", "slower", &mut |_| {})
        .unwrap();
    let call = rig.last_call(CallKind::Propose);
    assert!(call.prompt.recent.starts_with(SCENE_LINE));
    assert!(call.prompt.recent.contains(MARKER));
    assert_eq!(rig.scene_ops(c.turn_id), [(false, SceneWhy::NotIntimate)]);

    // The state before the corrected Turn holds an open scene that is not
    // cleared: the Correction plans as a plain one and closes it by G1.
    let mut rig = Rig::new("g1-correction-2");
    let p = rig.intimate();
    rig.played("t1", &p, &format!("{MARKER} Brann nods."));
    rig.add_npc("Kett", 34, false, "gate square");
    let plain = rig.plain();
    let t2 = rig.played("t2", &plain, "Dusk.");
    assert_eq!(rig.scene_ops(t2), [(false, SceneWhy::Gate)]);
    rig.gm.push_proposal(serde_json::to_string(&plain).unwrap());
    rig.gm.push_narration("Dusk again.".into());
    let c = rig
        .engine()
        .correct_last("c1".into(), "I wait.", "slower", &mut |_| {})
        .unwrap();
    let call = rig.last_call(CallKind::Propose);
    assert!(!call.prompt.recent.contains(SCENE_LINE));
    assert!(!call.prompt.recent.contains(MARKER));
    assert_eq!(rig.scene_ops(c.turn_id), [(false, SceneWhy::Gate)]);
    assert_eq!(rig.scene(), None);
}

// --- Restart, oracle, Correction ---------------------------------------------

#[test]
fn the_scene_state_survives_a_restart() {
    let mut rig = Rig::new("restart");
    let p = rig.intimate();
    let t1 = rig.played("t1", &p, "Brann nods.");
    rig.restart();
    assert_eq!(rig.scene(), Some(t1));
}

/// The rule the projection must equal (data shape 3.4, "why an op"): the
/// trailing run of standing Play and Correction Turns committed with
/// `intimate` that close nothing; the scene started at its first Turn.
fn last_turn_rule(store: &Store, campaign: u64) -> Option<TurnId> {
    let reverted = store.reverted_turns(campaign).unwrap();
    let standing: Vec<StoredTurn> = store
        .turns(campaign)
        .unwrap()
        .into_iter()
        .filter(|t| matches!(t.record.kind, TurnKind::Play | TurnKind::Correction))
        .filter(|t| !reverted.contains(&t.id))
        .collect();
    let mut since = None;
    for t in standing.iter().rev() {
        let closes = store
            .turn_ops(t.id)
            .unwrap()
            .iter()
            .any(|op| matches!(op, LedgerOp::SetScene { open: false, .. }));
        if !stored_intimate(&t.record.proposal_json) || closes {
            break;
        }
        since = Some(t.id);
    }
    since
}

/// One Correction of the last Play Turn, with `p` as its proposal.
fn correct(rig: &mut Rig, op: &str, p: &DecisionProposal) {
    rig.gm.push_proposal(serde_json::to_string(p).unwrap());
    rig.gm.push_narration("Again.".into());
    rig.engine()
        .correct_last(op.into(), "I stay with Brann.", "again", &mut |_| {})
        .unwrap();
}

/// Oracle: over a scripted run of thirteen Turns (plain, intimate, a seen
/// arrival, a hidden arrival, a move, two Corrections), the projection
/// equals the rule over the standing Turns.
#[test]
fn scene_projection_matches_the_last_turn_rule() {
    let mut rig = Rig::new("oracle");
    let yard = rig.place("Mill Yard");
    let kett = rig.add_npc("Kett", 34, true, "far road");
    let vale = rig.vale;
    let (intimate, plain) = (rig.intimate(), rig.plain());
    let check = |rig: &Rig, step: &str| {
        assert_eq!(
            rig.scene(),
            last_turn_rule(rig.store(), rig.campaign),
            "after {step}"
        );
    };
    rig.played("t1", &plain, "Dusk.");
    check(&rig, "t1 plain");
    let t2 = rig.played("t2", &intimate, "Brann nods.");
    check(&rig, "t2 opens");
    rig.played("t3", &intimate, "Brann stays.");
    check(&rig, "t3 keeps");
    assert_eq!(rig.scene(), Some(t2));
    correct(&mut rig, "c3", &plain);
    check(&rig, "c3 closes by correcting t3");
    assert_eq!(rig.scene(), None);
    let t4 = rig.played("t4", &intimate, "Brann nods.");
    check(&rig, "t4 opens");
    rig.played("t5", &plain, "Dusk.");
    check(&rig, "t5 closes");
    correct(&mut rig, "c5", &intimate);
    check(&rig, "c5 keeps the scene t5 closed");
    assert_eq!(rig.scene(), Some(t4));
    rig.arrival(vale);
    rig.played("t6", &intimate, "Brann stays.");
    check(&rig, "t6 hidden arrival keeps");
    rig.arrival(kett);
    rig.played("t7", &intimate, "Someone comes.");
    check(&rig, "t7 seen arrival closes");
    assert_eq!(rig.scene(), None);
    rig.played("t8", &intimate, "Brann nods.");
    check(&rig, "t8 opens again");
    let p = rig.intimate_move(yard);
    rig.played("t9", &p, "The yard.");
    check(&rig, "t9 move closes");
    rig.played("t10", &intimate, "Brann follows.");
    check(&rig, "t10 opens at the yard");
    rig.played("t11", &plain, "Quiet.");
    check(&rig, "t11 closes");
    assert_eq!(rig.scene(), None);
}

/// A scene Turn, then a plain Turn that closes it. Correcting the plain
/// Turn with an intimate proposal leaves the scene open, from the scene
/// Turn on.
#[test]
fn a_correction_reopens_the_scene_its_turn_closed() {
    let mut rig = Rig::new("reopens");
    let p = rig.intimate();
    let t1 = rig.played("t1", &p, "Brann nods.");
    let plain = rig.plain();
    rig.played("t2", &plain, "Dusk.");
    assert_eq!(rig.scene(), None);
    rig.gm.push_proposal(serde_json::to_string(&p).unwrap());
    rig.gm.push_narration("Brann stays.".into());
    let c = rig
        .engine()
        .correct_last("c1".into(), "I stay with Brann.", "I meant it", &mut |_| {})
        .unwrap();
    assert!(rig.scene_ops(c.turn_id).is_empty());
    assert_eq!(rig.scene(), Some(t1));
}

// --- What each prompt may read ----------------------------------------------

/// In an open cleared scene the proposal prompt holds the core's scene line
/// and the scene Turns in full, and so does the narration prompt.
#[test]
fn a_scene_prompt_holds_the_scene_line_and_full_turns() {
    let mut rig = Rig::new("full");
    let p = rig.intimate();
    rig.played("t1", &p, &format!("{MARKER} Brann nods."));
    rig.played("t2", &p, "Brann stays.");
    let propose = rig.last_call(CallKind::Propose);
    assert!(
        propose.prompt.recent.starts_with(SCENE_LINE),
        "{}",
        propose.prompt.recent
    );
    assert!(propose.prompt.recent.contains(MARKER));
    let narrate = rig.last_call(CallKind::Narrate);
    assert!(narrate.prompt.recent.contains(MARKER));
    assert!(narrate.prompt.recent.contains(IMPLIED_LINE));
    // The scene line names the longer plan limit.
    assert!(SCENE_LINE.contains(&SCENE_PLAN_MAX_CHARS.to_string()));
    assert!(!SCENE_LINE.trim_end().contains('\n'), "one line");
}

/// The prompt prefix (system, stable, scene blocks) does not change when a
/// scene opens or ends: only the `recent` block does.
#[test]
fn opening_a_scene_keeps_the_prompt_prefix() {
    let mut rig = Rig::new("prefix");
    let (intimate, plain) = (rig.intimate(), rig.plain());
    // Before the scene, in it, and after it: the Turns change no fact.
    rig.played("t1", &intimate, "Brann nods.");
    let before = rig.last_call(CallKind::Propose);
    rig.played("t2", &intimate, "Brann stays.");
    let during = rig.last_call(CallKind::Propose);
    rig.played("t3", &plain, "Dusk.");
    let after = rig.last_call(CallKind::Propose);
    for (a, b) in [(&before, &during), (&during, &after)] {
        assert_eq!(a.prompt.system, b.prompt.system);
        assert_eq!(a.prompt.stable, b.prompt.stable);
        assert_eq!(a.prompt.scene, b.prompt.scene);
    }
    assert!(!before.prompt.recent.contains(SCENE_LINE));
    assert!(during.prompt.recent.starts_with(SCENE_LINE));
}

/// After a plain Turn closes the scene, the next prompts hold each scene
/// Turn's neutral line and none of its Narration.
#[test]
fn scene_narration_is_in_no_prompt_after_the_scene() {
    let mut rig = Rig::new("after");
    let p = rig.intimate();
    let t1 = rig.played("t1", &p, &format!("{MARKER} Brann nods."));
    let t2 = rig.played("t2", &p, &format!("{MARKER} Brann stays."));
    let plain = rig.plain();
    rig.played("t3", &plain, "Dusk.");
    let closing_narration = rig.last_call(CallKind::Narrate);
    rig.played("t4", &plain, "Night.");
    let next_proposal = rig.calls(CallKind::Propose).pop().unwrap();
    let next_narration = rig.last_call(CallKind::Narrate);
    let lines = [
        neutral_line(&rig.turn(t1)).unwrap(),
        neutral_line(&rig.turn(t2)).unwrap(),
    ];
    for call in [&closing_narration, &next_proposal, &next_narration] {
        let text = prompt_text(call);
        assert!(!text.contains(MARKER), "{:?}: {}", call.kind, text);
        assert!(!text.contains(SCENE_LINE));
        for line in &lines {
            assert!(text.contains(line.as_str()), "{:?}: {}", call.kind, text);
        }
    }
    // The template is fixed text: minutes, no name and no value.
    assert!(lines[0].contains('5'), "{}", lines[0]);
    assert!(INTIMATE_TEMPLATE.contains("{minutes}"));
    for name in ["Ada", "Brann", "Orsel", "gate"] {
        assert!(!lines[0].contains(name), "{}", lines[0]);
    }
}

/// The plan limit rises to `SCENE_PLAN_MAX_CHARS` in a cleared scene only,
/// in the per-Turn schema and in `validate`.
#[test]
fn the_plan_limit_rises_in_a_cleared_scene_only() {
    let mut rig = Rig::new("plan");
    let p = rig.intimate();
    rig.played("t1", &p, "Brann nods.");
    let outside = rig.last_call(CallKind::Propose).schema;
    rig.played("t2", &p, "Brann stays.");
    let inside = rig.last_call(CallKind::Propose).schema;
    let plan =
        |schema: &serde_json::Value| schema["$defs"]["Reaction"]["properties"]["plan"].clone();
    assert_eq!(plan(&outside)["maxLength"], 90);
    assert!(plan(&outside).get("minLength").is_none());
    assert_eq!(plan(&inside)["maxLength"], SCENE_PLAN_MAX_CHARS);
    assert_eq!(plan(&inside)["minLength"], 1);

    let with_plan = |chars: usize| {
        let mut p = rig.intimate();
        p.reactions[0].plan = "x".repeat(chars);
        p
    };
    let refs = |p: &DecisionProposal, view: &WorldView| {
        validate(p, view)
            .err()
            .unwrap_or_default()
            .iter()
            .filter(|r| r.check == CheckKind::References)
            .count()
    };
    let scene = rig.view();
    assert!(scene.scene_open.is_some());
    assert_eq!(refs(&with_plan(200), &scene), 0);
    assert_eq!(refs(&with_plan(201), &scene), 1);
    assert_eq!(
        refs(&with_plan(0), &scene),
        1,
        "a scene plan is never empty"
    );
    let mut plain = scene.clone();
    plain.scene_open = None;
    assert_eq!(refs(&with_plan(90), &plain), 0);
    assert_eq!(refs(&with_plan(91), &plain), 1);
    assert_eq!(refs(&with_plan(0), &plain), 0);
}

/// A Narration told by implication inside a scene carries the token cap;
/// any other Narration carries none.
#[test]
fn a_scene_narration_is_capped() {
    let mut rig = Rig::new("cap");
    let plain = rig.plain();
    rig.played("t0", &plain, "Dusk.");
    assert_eq!(rig.last_call(CallKind::Narrate).max_tokens, None);
    let p = rig.intimate();
    rig.played("t1", &p, "Brann nods.");
    assert_eq!(
        rig.last_call(CallKind::Narrate).max_tokens,
        Some(SCENE_NARRATION_MAX_TOKENS)
    );
    rig.played("t2", &p, "Brann stays.");
    assert_eq!(rig.last_call(CallKind::Narrate).max_tokens, Some(280));
    rig.played("t3", &plain, "Dusk.");
    assert_eq!(rig.last_call(CallKind::Narrate).max_tokens, None);
}

/// G4 of a `renarrate`: a scene Turn narrated again after the scene ended
/// is told by implication, and the earlier scene Turns reach it as their
/// neutral lines only.
#[test]
fn renarrate_of_an_ended_scene_is_implied() {
    let mut rig = Rig::new("renarrate");
    let p = rig.intimate();
    let t1 = rig.played("t1", &p, &format!("{MARKER} Brann nods."));
    let t2 = rig.played("t2", &p, "Brann stays.");
    let plain = rig.plain();
    rig.played("t3", &plain, "Dusk.");
    rig.gm.push_narration("Brann stays again.".into());
    rig.engine().renarrate(t2, &mut |_| {}).unwrap();
    let call = rig.last_call(CallKind::Narrate);
    assert!(call.prompt.recent.starts_with(IMPLIED_LINE));
    assert!(!prompt_text(&call).contains(MARKER));
    let line = neutral_line(&rig.turn(t1)).unwrap();
    assert!(call.prompt.recent.contains(&line), "{}", call.prompt.recent);
}

/// Path 22: the Opening prompt and the OOC prompt of a Campaign with an
/// open scene hold no scene line, no implied line and no scene Narration.
/// The draft stages and the World Build read no Campaign at all: no file
/// of `src/build/` names a Turn or the scene's prompt lines.
#[test]
fn no_scene_text_outside_a_play_narration() {
    let mut rig = Rig::new("outside");
    let p = rig.intimate();
    let t1 = rig.played("t1", &p, &format!("{MARKER} Brann nods."));
    assert_eq!(rig.scene(), Some(t1));
    let view = rig.view();
    let opening = opening_prompt(&view, &[]);
    rig.gm.push_narration("You stand at the gate.".into());
    rig.engine().ooc("q1".into(), "Where am I?").unwrap();
    let ooc = rig.last_call(CallKind::Narrate);
    for (what, text) in [
        (
            "opening",
            format!(
                "{}{}{}{}",
                opening.system, opening.stable, opening.scene, opening.recent
            ),
        ),
        ("ooc", prompt_text(&ooc)),
    ] {
        for forbidden in [SCENE_LINE, IMPLIED_LINE, MARKER] {
            assert!(
                !text.contains(forbidden),
                "{what} holds {forbidden}: {text}"
            );
        }
    }
    let line = neutral_line(&rig.turn(t1)).unwrap();
    assert!(
        prompt_text(&ooc).contains(&line),
        "the OOC sheet gives the line"
    );

    let build = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/build");
    for entry in fs::read_dir(&build).unwrap() {
        let path = entry.unwrap().path();
        let text = fs::read_to_string(&path).unwrap();
        for word in [
            "SCENE_LINE",
            "IMPLIED_LINE",
            "recent_turns",
            "turns_window",
            "turn_text",
            "Cleared",
        ] {
            assert!(!text.contains(word), "{} names {word}", path.display());
        }
    }
}

/// The scene Turns keep one proposal call and one narration call each.
#[test]
fn a_scene_turn_makes_one_call_of_each_kind() {
    let mut rig = Rig::new("calls");
    let p = rig.intimate();
    rig.played("t1", &p, "Brann nods.");
    rig.played("t2", &p, "Brann stays.");
    let kinds: Vec<CallKind> = rig.gm.calls().iter().map(|c| c.kind).collect();
    assert_eq!(
        kinds,
        [
            CallKind::Propose,
            CallKind::Narrate,
            CallKind::Propose,
            CallKind::Narrate
        ]
    );
}
