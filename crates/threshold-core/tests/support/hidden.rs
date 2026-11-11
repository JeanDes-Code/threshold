//! The hidden-world fixture: the town gate of `support`, with things the PC
//! must never see. Every view test searches its result for `HIDDEN`.

use std::collections::HashSet;

use serde_json::json;
use threshold_core::{
    RecordedGm,
    domain::{
        ActionKind, Canon, ClaimDraft, EntityKind, EntityRef, FactDraft, FactSource, LedgerOp,
        OtherName, SeedEntity, SeedFact, Slot, Trigger, Verdict,
    },
    rules::{D10, RollResult},
    store::{Store, TurnId, TurnKind, TurnRecord, WorldView},
    turn::{BEGIN_OP_ID, Engine, create_campaign},
    world::{PcSight, pc_sight},
};

use super::{adventure_seed, proposal};

/// Texts the PC does not know: a GM-only fact, a goal, a World Event, a
/// hidden other name, a hidden entry, a stranger no Narration names, a fact
/// known to an NPC only, and an NPC at another place.
pub const HIDDEN: &[&str] = &[
    "gate key",
    "Keep Ada out of town",
    "Orsel rings the bell",
    "Toll-Wolf",
    "Vey",
    "Kestrel",
    "owes",
    "Pip",
];

/// The town gate, at minute 5. Ada (the PC) stands at the gate square with
/// Orsel, Brann, the hidden Vey and the stranger Kestrel. Pip is at the
/// market. Orsel is also "the Toll-Wolf", known to Brann only. Ada knows
/// one open item: `ada.pending_meet = Brann at the mill`.
///
/// Turns: the seed Setup Turn, the world Setup Turn, the Opening Turn, then
/// two play Turns. Kestrel arrives in the first play Turn.
pub struct HiddenWorld {
    pub store: Store,
    pub campaign: u64,
    pub ada: u64,
    pub orsel: u64,
    pub brann: u64,
    pub pip: u64,
    pub gate: u64,
    pub vey: u64,
    pub kestrel: u64,
    /// `ada.pending_meet`, known to Ada.
    pub meet: u64,
    /// `orsel.alias_toll_wolf`, known to Orsel and Brann.
    pub alias: u64,
    pub opening: TurnId,
    ops: u32,
}

/// A `proposal_json` as the Engine stores it.
pub fn record(action: ActionKind, verdict: Verdict, roll: Option<RollResult>) -> String {
    let mut p = proposal(verdict);
    p.action = action;
    json!({ "proposal": p, "reactions": [], "events": [], "roll": roll }).to_string()
}

/// A `proposal_json` whose proposal settles `item` (`E<id> pending_<x>`)
/// with `outcome`, as the Engine stores a Turn that closes an Open Item.
pub fn settling(
    action: ActionKind,
    item: String,
    outcome: threshold_core::domain::ItemStatus,
) -> String {
    let mut p = proposal(Verdict::Allowed);
    p.action = action;
    p.settles = Some(threshold_core::domain::Settle { item, outcome });
    json!({ "proposal": p, "reactions": [], "events": [], "roll": null }).to_string()
}

/// The PC's sight of a campaign in `store`.
pub fn sight_of(store: &Store, campaign: u64) -> PcSight {
    let view = store.snapshot(campaign).unwrap();
    let turns = store.turns(campaign).unwrap();
    let reverted: HashSet<TurnId> = store
        .reverted_turns(campaign)
        .unwrap()
        .into_iter()
        .collect();
    pc_sight(&view, &turns, &reverted).expect("the campaign has a PC")
}

/// A fact the Turn writes with `source: proposal`: it replaces the live
/// fact on the same slot, if any.
pub fn set(
    view: &WorldView,
    subject: u64,
    key: &str,
    value: &str,
    text: &str,
    seen_by: &[u64],
) -> LedgerOp {
    let new = FactDraft {
        slot: Slot {
            subject: EntityRef::Id(subject),
            key: key.into(),
            value: value.into(),
        },
        text: text.into(),
        canon: Canon::Established,
        source: FactSource::Proposal,
        seen_by: seen_by.iter().copied().map(EntityRef::Id).collect(),
        causes: vec![],
    };
    match view.live_fact(subject, key) {
        Some(old) => LedgerOp::SupersedeFact { old: old.id, new },
        None => LedgerOp::AddFact {
            slot: new.slot,
            text: new.text,
            canon: new.canon,
            source: new.source,
            seen_by: new.seen_by,
            causes: new.causes,
        },
    }
}

/// A Claim that `speaker` tells `listeners`, arriving after `delay_min`.
pub fn claim(
    speaker: u64,
    subject: u64,
    key: &str,
    value: &str,
    listeners: &[u64],
    delay_min: u32,
) -> LedgerOp {
    LedgerOp::AddClaim {
        claim: ClaimDraft {
            speaker: EntityRef::Id(speaker),
            slot: Slot {
                subject: EntityRef::Id(subject),
                key: key.into(),
                value: value.into(),
            },
            text: format!("{key}: {value}"),
        },
        listeners: listeners.iter().copied().map(EntityRef::Id).collect(),
        delay_min,
    }
}

/// A new NPC that stands at `place`, seen by `seen_by`.
pub fn newcomer(name: &str, place: &str, seen_by: &[u64]) -> Vec<LedgerOp> {
    vec![
        LedgerOp::AddEntity {
            local: Some(1),
            kind: EntityKind::Npc,
            name: name.into(),
            age: Some(35),
            other_names: vec![],
            age_confirmed: false,
        },
        LedgerOp::AddFact {
            slot: Slot {
                subject: EntityRef::New(1),
                key: "location".into(),
                value: place.into(),
            },
            text: format!("{name} is at {place}."),
            canon: Canon::Established,
            source: FactSource::Proposal,
            seen_by: seen_by.iter().copied().map(EntityRef::Id).collect(),
            causes: vec![],
        },
    ]
}

impl HiddenWorld {
    pub fn new() -> Self {
        let mut seed = adventure_seed();
        let orsel = seed
            .entities
            .iter_mut()
            .find(|e| e.name == "Orsel")
            .unwrap();
        orsel.other_names = vec![OtherName {
            name: "the Toll-Wolf".into(),
            known_to: vec!["Brann".into()],
        }];
        seed.entities.push(SeedEntity {
            name: "Vey".into(),
            kind: EntityKind::Npc,
            age: Some(40),
            other_names: vec![],
            hidden: true,
        });
        seed.decided.push(SeedFact {
            subject: "Vey".into(),
            key: "location".into(),
            value: "gate square".into(),
            text: "Vey waits in the shadow of the gate square.".into(),
        });
        let mut store = Store::open_in_memory().unwrap();
        let campaign = create_campaign(&mut store, &seed, 7).unwrap();
        let view = store.snapshot(campaign).unwrap();
        let id = |name: &str| view.entities.iter().find(|e| e.name == name).unwrap().id;
        let mut w = HiddenWorld {
            campaign,
            ada: id("Ada"),
            orsel: id("Orsel"),
            brann: id("Brann"),
            pip: id("Pip"),
            gate: id("Gate"),
            vey: id("Vey"),
            kestrel: 0,
            meet: 0,
            alias: view
                .facts
                .iter()
                .find(|f| f.slot.key.starts_with("alias_"))
                .unwrap()
                .id,
            opening: 0,
            store,
            ops: 0,
        };
        let (ada, orsel, gate) = (w.ada, w.orsel, w.gate);
        let view = w.view();
        w.commit(
            TurnKind::Setup,
            "",
            "{}",
            0,
            vec![
                set(&view, gate, "locked", "yes", "The gate is locked.", &[]),
                set(
                    &view,
                    orsel,
                    "owes",
                    "ada",
                    "Orsel owes Ada a debt.",
                    &[orsel],
                ),
                set(
                    &view,
                    ada,
                    "holds",
                    "gate key",
                    "Ada holds the gate key.",
                    &[],
                ),
                set(
                    &view,
                    ada,
                    "pending_meet",
                    "brann at the mill",
                    "Ada plans to meet Brann at the mill.",
                    &[ada],
                ),
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
                    public: false,
                },
            ],
        );
        w.meet = w.view().live_fact(ada, "pending_meet").unwrap().id;
        w.opening = w.commit(TurnKind::Opening, "", "{}", 0, vec![]);
        w.narrate(
            w.opening,
            "Dusk falls on the gate square. Orsel watches the road. Brann leans on the wall.",
        );
        let first = w.play(
            "I wait by the gate.",
            &record(ActionKind::Observe, Verdict::Allowed, None),
            5,
            newcomer("Kestrel", "gate square", &[ada]),
        );
        w.narrate(first, "A stranger in a grey hood stops near Orsel.");
        w.kestrel = id_of(&w.view(), "Kestrel");
        let second = w.play(
            "I ask Brann about the road.",
            &record(ActionKind::Talk, Verdict::Allowed, None),
            0,
            vec![],
        );
        w.narrate(second, "Brann shrugs. Orsel says nothing.");
        w
    }

    pub fn view(&self) -> WorldView {
        self.store.snapshot(self.campaign).unwrap()
    }

    pub fn sight(&self) -> PcSight {
        sight_of(&self.store, self.campaign)
    }

    /// The `seq` of a Turn.
    pub fn seq(&self, turn: TurnId) -> u64 {
        self.store.turn(turn).unwrap().unwrap().seq
    }

    /// Commits a Turn of `kind` that moves the World Clock by `minutes`.
    pub fn commit(
        &mut self,
        kind: TurnKind,
        intent: &str,
        proposal_json: &str,
        minutes: u32,
        mut ops: Vec<LedgerOp>,
    ) -> TurnId {
        self.ops += 1;
        let clock = self.view().clock_min;
        if minutes > 0 {
            ops.push(LedgerOp::AdvanceClock { minutes });
        }
        let op_id = if kind == TurnKind::Opening {
            BEGIN_OP_ID.to_owned()
        } else {
            format!("hidden-{}", self.ops)
        };
        self.store
            .commit(
                TurnRecord {
                    campaign_id: self.campaign,
                    op_id,
                    kind,
                    intent: intent.into(),
                    proposal_json: proposal_json.into(),
                    clock_before: clock,
                    clock_after: clock + u64::from(minutes),
                },
                ops,
            )
            .unwrap()
    }

    /// Commits a play Turn.
    pub fn play(
        &mut self,
        intent: &str,
        proposal_json: &str,
        minutes: u32,
        ops: Vec<LedgerOp>,
    ) -> TurnId {
        self.commit(TurnKind::Play, intent, proposal_json, minutes, ops)
    }

    /// Commits a Correction Turn that reverts `turn`.
    pub fn revert(&mut self, turn: TurnId, intent: &str, minutes: u32) -> TurnId {
        self.commit(
            TurnKind::Correction,
            intent,
            &record(ActionKind::Observe, Verdict::Allowed, None),
            minutes,
            vec![LedgerOp::RevertTurn { turn_id: turn }],
        )
    }

    pub fn narrate(&mut self, turn: TurnId, text: &str) {
        self.store.set_narration(turn, text).unwrap();
    }

    /// Hands the store to an Engine that plays with `gm`.
    pub fn engine(self, gm: &RecordedGm) -> (Engine, u64) {
        let campaign = self.campaign;
        (
            Engine::new(
                self.store,
                Box::new(gm.clone()),
                Box::new(D10),
                vec![],
                campaign,
            ),
            campaign,
        )
    }
}

impl Default for HiddenWorld {
    fn default() -> Self {
        Self::new()
    }
}

pub fn id_of(view: &WorldView, name: &str) -> u64 {
    view.entities
        .iter()
        .find(|e| e.name == name)
        .unwrap_or_else(|| panic!("no entity {name}"))
        .id
}
