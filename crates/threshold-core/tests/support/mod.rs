//! Shared test fixtures: a small campaign built through the real store, and
//! builders for Decision Proposals. Later tasks reuse them.
#![allow(dead_code)]

pub mod hidden;

use threshold_core::{
    domain::{
        ActionKind, AdventureSeed, Canon, CheckDraft, DecisionProposal, EntityKind, EntityRef,
        FactSource, GoalStatus, Handle, LedgerOp, NewFact, NewNpc, PcSeed, Reaction, SeedEntity,
        SeedFact, Seen, Slot, SlotDraft, Stance, StartSeed, Trigger, Verdict,
    },
    rules::RollResult,
    store::{Store, TurnKind, TurnRecord, WorldView},
};

/// The gate scene. Ada (the PC, 30), Orsel (52) and Brann (18) stand at the
/// gate square. Pip (17, a minor) is at the market. The gate is locked and that is
/// an open obstacle.
pub struct World {
    pub store: Store,
    pub campaign: u64,
    pub view: WorldView,
    pub ada: u64,
    pub orsel: u64,
    pub brann: u64,
    pub pip: u64,
    pub gate: u64,
    /// `gate.locked = yes`, Established, the open obstacle.
    pub gate_locked: u64,
    /// `gate.material = iron`, Persistent (decided in the Adventure Seed).
    pub gate_material: u64,
    /// `orsel.owes = ada`, Established, known to Orsel only.
    pub debt: u64,
    /// `ada.holds = gate key`, Established, GM only.
    pub secret: u64,
    /// `orsel.bribable = yes`, Possible (proposed in the Adventure Seed).
    pub rumor: u64,
    /// Orsel's active goal against Ada.
    pub goal: u64,
    /// `Orsel rings the bell` at minute 30; actor Orsel.
    pub bell: u64,
    /// The obstacle row on `gate_locked`.
    pub obstacle: u64,
    ops: u32,
}

fn seed_fact(subject: &str, key: &str, value: &str) -> SeedFact {
    SeedFact {
        subject: subject.into(),
        key: key.into(),
        value: value.into(),
        text: format!("{subject} {key} {value}"),
    }
}

pub fn place(name: &str) -> SeedEntity {
    SeedEntity {
        name: name.into(),
        kind: EntityKind::Place,
        age: None,
        other_names: vec![],
        hidden: false,
    }
}

fn npc(name: &str, age: u16) -> SeedEntity {
    SeedEntity {
        name: name.into(),
        kind: EntityKind::Npc,
        age: Some(age),
        other_names: vec![],
        hidden: false,
    }
}

pub fn adventure_seed() -> AdventureSeed {
    AdventureSeed {
        premise: "A town gate at dusk".into(),
        pc: PcSeed {
            name: "Ada".into(),
            age: 30,
            other_names: vec![],
            concept: "Courier".into(),
        },
        entities: vec![
            npc("Orsel", 52),
            npc("Brann", 18),
            npc("Pip", 17),
            place("Gate"),
        ],
        boundaries: vec![],
        decided: vec![
            seed_fact("Ada", "location", "gate square"),
            seed_fact("Orsel", "location", "gate square"),
            seed_fact("Brann", "location", "gate square"),
            seed_fact("Pip", "location", "market"),
            seed_fact("Gate", "material", "iron"),
        ],
        proposed: vec![seed_fact("Orsel", "bribable", "yes")],
        story_language: "en".into(),
        themes: vec![],
        gm_agreement: String::new(),
        resolution: String::new(),
        min_age: None,
        // The gate world counts its times from midnight of Day 1: its
        // events and clocks are absolute minutes.
        start: StartSeed {
            hour: Some(0),
            ..StartSeed::default()
        },
        open_details: vec![],
    }
}

fn add_fact(subject: u64, key: &str, value: &str, seen_by: Vec<u64>) -> LedgerOp {
    LedgerOp::AddFact {
        slot: Slot {
            subject: EntityRef::Id(subject),
            key: key.into(),
            value: value.into(),
        },
        text: format!("{key} {value}"),
        canon: Canon::Established,
        source: FactSource::Proposal,
        seen_by: seen_by.into_iter().map(EntityRef::Id).collect(),
        causes: vec![],
    }
}

impl World {
    pub fn new() -> Self {
        let mut store = Store::open_in_memory().unwrap();
        let campaign = store.create_campaign(&adventure_seed(), 7).unwrap();
        let view = store.snapshot(campaign).unwrap();
        let id = |name: &str| view.entities.iter().find(|e| e.name == name).unwrap().id;
        let (ada, orsel, brann, pip, gate) =
            (id("Ada"), id("Orsel"), id("Brann"), id("Pip"), id("Gate"));
        let mut world = World {
            store,
            campaign,
            view,
            ada,
            orsel,
            brann,
            pip,
            gate,
            gate_locked: 0,
            gate_material: 0,
            debt: 0,
            secret: 0,
            rumor: 0,
            goal: 0,
            bell: 0,
            obstacle: 0,
            ops: 0,
        };
        world.commit(vec![
            add_fact(gate, "locked", "yes", vec![]),
            add_fact(orsel, "owes", "ada", vec![orsel]),
            add_fact(ada, "holds", "gate key", vec![]),
            LedgerOp::ShiftAttitude {
                from: EntityRef::Id(orsel),
                to: EntityRef::Id(ada),
                delta: -4,
            },
            LedgerOp::AddGoal {
                npc: EntityRef::Id(orsel),
                text: "Keep Ada out of town".into(),
                against: Some(EntityRef::Id(ada)),
            },
            LedgerOp::ScheduleEvent {
                text: "Orsel rings the bell".into(),
                trigger: Trigger::At(30),
                actors: vec![EntityRef::Id(orsel)],
                goal_id: None,
                cause_fact_id: None,
                effects: vec![],
                seen_by: vec![],
            },
        ]);
        let live = |w: &World, subject: u64, key: &str| w.view.live_fact(subject, key).unwrap().id;
        world.gate_locked = live(&world, gate, "locked");
        world.gate_material = live(&world, gate, "material");
        world.debt = live(&world, orsel, "owes");
        world.secret = live(&world, ada, "holds");
        world.rumor = live(&world, orsel, "bribable");
        world.goal = world.view.goals_active[0].id;
        world.bell = world.view.events_scheduled[0].id;
        world.commit(vec![LedgerOp::OpenObstacle {
            fact_id: world.gate_locked,
        }]);
        world.obstacle = world.view.obstacles_open[0].id;
        world
    }

    /// Commits ops as one Play Turn and refreshes `view`.
    pub fn commit(&mut self, ops: Vec<LedgerOp>) {
        self.ops += 1;
        self.store
            .commit(
                TurnRecord {
                    campaign_id: self.campaign,
                    op_id: format!("op-{}", self.ops),
                    kind: TurnKind::Play,
                    intent: "test".into(),
                    proposal_json: "{}".into(),
                    clock_before: self.view.clock_min,
                    clock_after: self.view.clock_min,
                },
                ops,
            )
            .unwrap();
        self.view = self.store.snapshot(self.campaign).unwrap();
    }

    /// The fact id one higher than any fact in the view: the id the next new
    /// fact would get in an empty database.
    pub fn next_fact_id(&self) -> u64 {
        self.view.facts.iter().map(|f| f.id).max().unwrap_or(0) + 1
    }

    /// Adds a place entity the PC can name, for a move to go to (`move_to`,
    /// ADR 0007): the PC holds one fact about it.
    pub fn add_place(&mut self, name: &str) -> u64 {
        self.commit(vec![LedgerOp::AddEntity {
            local: None,
            kind: EntityKind::Place,
            name: name.into(),
            age: None,
            other_names: vec![],
        }]);
        let id = self.view.entity_by_name(name).unwrap().id;
        let ada = self.ada;
        self.commit(vec![add_fact(id, "kind", "square", vec![ada])]);
        id
    }

    pub fn goal_done(&mut self) {
        self.commit(vec![LedgerOp::SetGoalStatus {
            goal: self.goal,
            status: GoalStatus::Done,
        }]);
    }
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

pub fn e(id: u64) -> Handle {
    Handle::Entity(id)
}

pub fn f(id: u64) -> Handle {
    Handle::Fact(id)
}

pub fn proposal(verdict: Verdict) -> DecisionProposal {
    DecisionProposal {
        action: ActionKind::Act,
        verdict,
        contested: false,
        move_to: None,
        open_items: Default::default(),
        blockers: vec![],
        check: None,
        reactions: vec![],
        new_npcs: vec![],
        facts: vec![],
        cancel: vec![],
        schedule: None,
        minutes: 0,
        intimate: false,
        facts_on_win: false,
    }
}

pub fn slot(subject: Handle, key: &str, value: &str) -> SlotDraft {
    SlotDraft {
        subject,
        key: key.into(),
        value: value.into(),
    }
}

pub fn fact(slot: SlotDraft, seen: Seen, because: Vec<Handle>) -> NewFact {
    NewFact {
        slot,
        seen,
        said_by: None,
        because,
    }
}

pub fn reaction(
    npc: Handle,
    stance: Stance,
    cause: Option<threshold_core::domain::Cause>,
) -> Reaction {
    Reaction {
        npc,
        stance,
        plan: "reacts".into(),
        cause,
    }
}

pub fn check(against: Option<Handle>, win: SlotDraft, lose: SlotDraft) -> CheckDraft {
    CheckDraft {
        against,
        win: Some(win),
        lose: Some(lose),
        factors: vec![],
    }
}

pub fn new_npc(n: u64, name: &str, age: u16) -> NewNpc {
    NewNpc {
        id: Handle::NewNpc(n),
        name: name.into(),
        age,
        voice: "Clipped words, never says please.".into(),
    }
}

pub fn roll(success: bool) -> RollResult {
    RollResult {
        seed: 99,
        difficulty: 6,
        value: if success { 8 } else { 3 },
        success,
    }
}
