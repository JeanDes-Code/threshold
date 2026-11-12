//! The gate world at `implied` in a Campaign file, played through an
//! `Engine`: the rig of the Intimate Scene and Fade to Black tests.

use std::{
    fs,
    path::{Path, PathBuf},
};
use threshold_core::domain::Intimacy;

use super::*;
use threshold_core::{
    CallKind, Gm, RecordedCall, RecordedGm,
    domain::{
        ActionKind, Canon, DecisionProposal, DetailLevel, Effect, EntityKind, EntityRef,
        FactSource, LedgerOp, SceneWhy, Slot, Stance, Trigger, Verdict,
    },
    pack::ContentPack,
    rules::D10,
    store::{Store, StoredTurn, TurnId, TurnKind, TurnRecord, WorldView},
    turn::{Engine, TurnControl, TurnError, TurnOutcome, create_campaign},
    world::{PcSight, pc_sight},
};

/// The gate world at `implied` in a Campaign file, played through an
/// `Engine`. Ada, Orsel and Brann are confirmed, and so is Vale (40), an
/// NPC the Adventure Seed hides, on the far road. `edit` commits raw ops as
/// another program would, between two Turns. `packs` are the Content Packs
/// every Engine of the rig gets.
pub struct Rig {
    pub path: PathBuf,
    pub campaign: u64,
    pub gm: RecordedGm,
    pub engine: Option<Engine>,
    pub ada: u64,
    pub brann: u64,
    pub vale: u64,
    pub edits: u32,
    pub packs: Vec<ContentPack>,
}

impl Rig {
    pub fn new(name: &str) -> Self {
        Self::with(name, DetailLevel::Implied, Vec::new())
    }

    /// The rig at Detail Level `detail`, with `packs` loaded.
    pub fn with(name: &str, detail: DetailLevel, packs: Vec<ContentPack>) -> Self {
        let path = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("scene-{name}-{}.sqlite", std::process::id()));
        let _ = fs::remove_file(&path);
        let mut store = Store::open(&path).unwrap();
        let mut seed = adventure_seed();
        seed.detail = detail;
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
            packs,
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

    pub fn make_engine(&self, store: Store) -> Engine {
        Engine::new(
            store,
            Box::new(self.gm.clone()),
            Box::new(D10),
            self.packs.clone(),
            self.campaign,
        )
    }

    /// The app restarted with `packs` in place of the loaded ones: a pack
    /// added or removed between two Turns.
    pub fn set_packs(&mut self, packs: Vec<ContentPack>) {
        self.packs = packs;
        self.restart();
    }

    /// The Engine on the same file with another model.
    pub fn with_gm(&mut self, gm: Box<dyn Gm + Send>) {
        drop(self.engine.take());
        let store = Store::open(&self.path).unwrap();
        self.engine = Some(Engine::new(
            store,
            gm,
            Box::new(D10),
            self.packs.clone(),
            self.campaign,
        ));
    }

    /// "End scene" with `p` as the closing proposal and `narration` as the
    /// closing Narration.
    pub fn end_scene(
        &mut self,
        op: &str,
        p: &DecisionProposal,
        narration: &str,
    ) -> Result<TurnOutcome, TurnError> {
        self.gm.push_proposal(serde_json::to_string(p).unwrap());
        self.gm.push_narration(narration.into());
        let mut ctl = TurnControl {
            on_chunk: &mut |_| {},
            faded: &|| false,
        };
        self.engine().end_scene(op.into(), &mut ctl)
    }

    /// The PC's sight now: the one input of every Player screen.
    pub fn sight(&self) -> PcSight {
        let store = self.store();
        let turns = store.turns(self.campaign).unwrap();
        let reverted = store
            .reverted_turns(self.campaign)
            .unwrap()
            .into_iter()
            .collect();
        pc_sight(&self.view(), &turns, &reverted).unwrap()
    }

    pub fn commit_raw(&mut self, store: &mut Store, ops: Vec<LedgerOp>) {
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

    pub fn engine(&mut self) -> &mut Engine {
        self.engine.as_mut().unwrap()
    }

    pub fn store(&self) -> &Store {
        self.engine.as_ref().unwrap().store()
    }

    pub fn view(&self) -> WorldView {
        self.store().snapshot(self.campaign).unwrap()
    }

    /// Another program's edit: the Engine lets go of the file, the ops are
    /// committed in a Turn of their own, and a new Engine opens it again.
    pub fn edit(&mut self, ops: Vec<LedgerOp>) {
        drop(self.engine.take());
        let mut store = Store::open(&self.path).unwrap();
        self.commit_raw(&mut store, ops);
        self.engine = Some(self.make_engine(store));
    }

    /// Streams each Narration word by word from now on (`RecordedGm::chunked`).
    pub fn chunked(&mut self) {
        self.gm = self.gm.clone().chunked();
        self.restart();
    }

    /// Plays one Turn whose proposal is `p` and whose Narration is
    /// `narration`, through `ctl`.
    pub fn play_with(
        &mut self,
        op: &str,
        p: &DecisionProposal,
        narration: &str,
        ctl: &mut TurnControl<'_>,
    ) -> Result<TurnOutcome, TurnError> {
        self.gm.push_proposal(serde_json::to_string(p).unwrap());
        self.gm.push_narration(narration.into());
        self.engine()
            .play_turn_with(op.into(), "I stay with Brann.", ctl)
    }

    /// Closes the file and opens it again, as a restart of the app does.
    pub fn restart(&mut self) {
        drop(self.engine.take());
        let store = Store::open(&self.path).unwrap();
        self.engine = Some(self.make_engine(store));
    }

    /// Plays one Turn whose proposal is `p` and whose Narration is
    /// `narration`.
    pub fn play(
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

    pub fn played(&mut self, op: &str, p: &DecisionProposal, narration: &str) -> TurnId {
        self.play(op, p, narration).unwrap().turn_id
    }

    pub fn scene(&self) -> Option<TurnId> {
        self.view().scene_open
    }

    pub fn turn(&self, id: TurnId) -> StoredTurn {
        self.store().turn(id).unwrap().unwrap()
    }

    /// The `SetScene` ops a Turn committed.
    pub fn scene_ops(&self, id: TurnId) -> Vec<(bool, SceneWhy)> {
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
    pub fn calls(&self, kind: CallKind) -> Vec<RecordedCall> {
        self.gm
            .calls()
            .into_iter()
            .filter(|c| c.kind == kind)
            .collect()
    }

    pub fn last_call(&self, kind: CallKind) -> RecordedCall {
        self.calls(kind).pop().expect("a call of this kind")
    }

    /// Adds an NPC at `place` with a raw op, as another program would.
    pub fn add_npc(&mut self, name: &str, age: u16, confirmed: bool, place: &str) -> u64 {
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
    pub fn arrival(&mut self, subject: u64) {
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
    pub fn place(&mut self, name: &str) -> u64 {
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
    pub fn intimate(&self) -> DecisionProposal {
        let mut p = proposal(Verdict::Allowed);
        p.intimacy = Intimacy::Intimate;
        p.minutes = 5;
        p.reactions = vec![reaction(e(self.brann), Stance::Comply, None)];
        p
    }

    /// A plain proposal of five minutes.
    pub fn plain(&self) -> DecisionProposal {
        let mut p = proposal(Verdict::Allowed);
        p.minutes = 5;
        p
    }

    /// A closing proposal: Brann takes it calmly, in ten minutes.
    pub fn closing(&self) -> DecisionProposal {
        let mut p = proposal(Verdict::Allowed);
        p.action = ActionKind::Wait;
        p.minutes = 10;
        p.reactions = vec![reaction(e(self.brann), Stance::Comply, None)];
        p
    }

    /// An intimate proposal that moves the PC to `place`.
    pub fn intimate_move(&self, place: u64) -> DecisionProposal {
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

pub fn prompt_text(call: &RecordedCall) -> String {
    let p = &call.prompt;
    format!("{}{}{}{}", p.system, p.stable, p.scene, p.recent)
}
