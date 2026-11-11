use std::path::Path;

use threshold_core::{
    domain::{
        AdventureSeed, Canon, EntityKind, EntityRef, FactSource, Handle, LedgerOp, PcSeed,
        SeedFact, Slot, StartSeed,
    },
    store::{CommitError, Store, TurnKind, TurnRecord},
};

fn adventure_seed() -> AdventureSeed {
    AdventureSeed {
        premise: "A frontier".into(),
        entities: vec![],
        pc: PcSeed {
            name: "Ada".into(),
            age: 30,
            other_names: vec![],
            concept: "Scout".into(),
        },
        boundaries: vec![],
        decided: vec![SeedFact {
            subject: "Ada".into(),
            key: "Home".into(),
            value: "The Hill".into(),
            text: "Ada lives on the hill.".into(),
        }],
        proposed: vec![SeedFact {
            subject: "Ada".into(),
            key: "Ally".into(),
            value: "A Raven".into(),
            text: "A raven may help.".into(),
        }],
        story_language: "en".into(),
        themes: vec![],
        gm_agreement: String::new(),
        resolution: String::new(),
        min_age: None,
        // These tests use absolute minutes: the clock starts at midnight.
        start: StartSeed {
            hour: Some(0),
            ..StartSeed::default()
        },
        open_details: vec![],
        detail: Default::default(),
    }
}
fn record(campaign_id: u64, op: &str) -> TurnRecord {
    TurnRecord {
        campaign_id,
        op_id: op.into(),
        kind: TurnKind::Play,
        intent: "look".into(),
        proposal_json: "{}".into(),
        clock_before: 0,
        clock_after: 0,
    }
}

fn local_npc(local: u8) -> LedgerOp {
    LedgerOp::AddEntity {
        local: Some(local),
        kind: EntityKind::Npc,
        name: "Orsel".into(),
        age: Some(40),
        other_names: vec![],
        age_confirmed: false,
    }
}

fn local_voice(local: u8) -> LedgerOp {
    LedgerOp::AddVoiceCard {
        npc: EntityRef::New(local),
        habits: "quiet".into(),
        vocabulary: "plain".into(),
        samples: vec![],
    }
}

#[test]
fn new_npc_with_voice_card_in_one_commit() {
    let mut db = Store::open_in_memory().unwrap();
    let campaign = db.create_campaign(&adventure_seed(), 42).unwrap();
    db.commit(
        record(campaign, "local"),
        vec![local_npc(1), local_voice(1)],
    )
    .unwrap();
    let npc = db
        .snapshot(campaign)
        .unwrap()
        .entities
        .iter()
        .find(|entity| entity.name == "Orsel")
        .unwrap()
        .id;
    let card = db.voice_card(npc).unwrap().unwrap();
    assert_eq!(card.npc, npc);
    assert_eq!(card.habits, "quiet");
    assert_eq!(card.vocabulary, "plain");
    assert!(card.samples.is_empty());
}

#[test]
fn unknown_local_ref_rolls_back() {
    let mut db = Store::open_in_memory().unwrap();
    let campaign = db.create_campaign(&adventure_seed(), 42).unwrap();
    let before = db.snapshot(campaign).unwrap();
    assert!(
        db.commit(
            record(campaign, "unknown"),
            vec![local_npc(1), local_voice(2)]
        )
        .is_err()
    );
    assert_eq!(db.snapshot(campaign).unwrap(), before);
    assert!(db.turn_by_op(&"unknown".into()).unwrap().is_none());
}

#[test]
fn duplicate_local_ref_rolls_back() {
    let mut db = Store::open_in_memory().unwrap();
    let campaign = db.create_campaign(&adventure_seed(), 42).unwrap();
    let before = db.snapshot(campaign).unwrap();
    assert!(
        db.commit(
            record(campaign, "duplicate"),
            vec![local_npc(1), local_npc(1)]
        )
        .is_err()
    );
    assert_eq!(db.snapshot(campaign).unwrap(), before);
    assert!(db.turn_by_op(&"duplicate".into()).unwrap().is_none());
}

#[test]
fn ledger_stores_resolved_ids() {
    let path = std::env::temp_dir().join(format!(
        "threshold-local-ref-{}-{:?}.db",
        std::process::id(),
        std::thread::current().id()
    ));
    let mut db = Store::open(&path).unwrap();
    let campaign = db.create_campaign(&adventure_seed(), 42).unwrap();
    let turn = db
        .commit(
            record(campaign, "replay-local"),
            vec![local_npc(1), local_voice(1)],
        )
        .unwrap();
    let before = db.snapshot(campaign).unwrap();
    let npc = before
        .entities
        .iter()
        .find(|entity| entity.name == "Orsel")
        .unwrap()
        .id;
    let sql = rusqlite::Connection::open(&path).unwrap();
    let stored: String = sql
        .query_row(
            "SELECT op_json FROM ledger WHERE turn_id=?1 AND json_type(op_json,'$.op.add_voice_card') IS NOT NULL",
            [turn],
            |r| r.get(0),
        )
        .unwrap();
    let stored: serde_json::Value = serde_json::from_str(&stored).unwrap();
    assert_eq!(
        stored["op"]["add_voice_card"]["npc"],
        serde_json::json!(npc)
    );
    let ledger: Vec<String> = sql
        .prepare("SELECT op_json FROM ledger WHERE turn_id=?1")
        .unwrap()
        .query_map([turn], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(ledger.iter().all(|op| !op.contains("\"new\"")));
    db.rebuild_projections(campaign).unwrap();
    assert_eq!(db.snapshot(campaign).unwrap(), before);
    assert_eq!(db.voice_card(npc).unwrap().unwrap().habits, "quiet");
    drop(sql);
    drop(db);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn present_matches_pc_location() {
    let mut db = Store::open_in_memory().unwrap();
    let mut seed = adventure_seed();
    seed.entities = vec![
        threshold_core::domain::SeedEntity {
            kind: EntityKind::Npc,
            name: "Near".into(),
            age: Some(40),
            other_names: vec![],
            hidden: false,
        },
        threshold_core::domain::SeedEntity {
            kind: EntityKind::Npc,
            name: "Far".into(),
            age: Some(40),
            other_names: vec![],
            hidden: false,
        },
    ];
    for (subject, value) in [("Ada", "The Hall"), ("Near", " hall  "), ("Far", "Garden")] {
        seed.decided.push(SeedFact {
            subject: subject.into(),
            key: "location".into(),
            value: value.into(),
            text: value.into(),
        });
    }
    let campaign = db.create_campaign(&seed, 42).unwrap();
    let view = db.snapshot(campaign).unwrap();
    let pc = view
        .entities
        .iter()
        .find(|entity| entity.name == "Ada")
        .unwrap()
        .id;
    let near = view
        .entities
        .iter()
        .find(|entity| entity.name == "Near")
        .unwrap()
        .id;
    let far = view
        .entities
        .iter()
        .find(|entity| entity.name == "Far")
        .unwrap()
        .id;
    assert!(view.present().contains(&pc));
    assert!(view.present().contains(&near));
    assert!(!view.present().contains(&far));
    let mut without_pc_location = view.clone();
    without_pc_location
        .live_facts
        .retain(|fact| fact.slot.subject != pc || fact.slot.key != "location");
    assert_eq!(without_pc_location.present(), vec![pc]);
}
fn fact(subject: u64, value: &str) -> LedgerOp {
    LedgerOp::AddFact {
        slot: Slot {
            subject: EntityRef::Id(subject),
            key: "door".into(),
            value: value.into(),
        },
        text: value.into(),
        canon: Canon::Established,
        source: FactSource::Proposal,
        seen_by: vec![],
        causes: vec![],
    }
}
fn supersede(old: u64, subject: u64, value: &str) -> LedgerOp {
    LedgerOp::SupersedeFact {
        old,
        new: threshold_core::domain::FactDraft {
            slot: Slot {
                subject: EntityRef::Id(subject),
                key: "door".into(),
                value: value.into(),
            },
            text: value.into(),
            canon: Canon::Established,
            source: FactSource::Proposal,
            seen_by: vec![],
            causes: vec![],
        },
    }
}
#[test]
fn migrations_apply_on_empty_db() {
    let mut db = Store::open_in_memory().unwrap();
    let id = db.create_campaign(&adventure_seed(), 42).unwrap();
    let setup = db.turn_by_op(&"__seed__".into()).unwrap().unwrap();
    assert_eq!(setup.record.kind, TurnKind::Setup);
    assert_eq!(
        serde_json::to_string(&setup.record.kind).unwrap(),
        "\"setup\""
    );
    let view = db.snapshot(id).unwrap();
    let pc = &view.entities[0];
    assert_eq!(pc.kind, EntityKind::Pc);
    assert_eq!(
        view.live_fact(pc.id, "home").unwrap().canon,
        Canon::Persistent
    );
    assert!(
        view.live_facts
            .iter()
            .any(|f| f.canon == Canon::Possible && f.slot.value == "raven")
    );
}
#[test]
fn character_without_age_is_refused() {
    let mut db = Store::open_in_memory().unwrap();
    let c = db.create_campaign(&adventure_seed(), 42).unwrap();
    let before = db.snapshot(c).unwrap();
    let add = |kind, age| LedgerOp::AddEntity {
        local: None,
        kind,
        name: "Stranger".into(),
        age,
        other_names: vec![],
        age_confirmed: false,
    };
    for (op_id, kind, age) in [
        ("ageless-npc", EntityKind::Npc, None),
        ("ageless-pc", EntityKind::Pc, None),
        ("aged-place", EntityKind::Place, Some(40)),
    ] {
        assert!(
            db.commit(record(c, op_id), vec![add(kind, age)]).is_err(),
            "{op_id} committed"
        );
        assert_eq!(db.snapshot(c).unwrap(), before);
        assert!(db.turn_by_op(&op_id.into()).unwrap().is_none());
    }
    // The same entities with the right age commit.
    db.commit(record(c, "aged-npc"), vec![add(EntityKind::Npc, Some(0))])
        .unwrap();
    db.commit(
        record(c, "ageless-place"),
        vec![add(EntityKind::Place, None)],
    )
    .unwrap();
}
#[test]
fn one_live_value_per_slot() {
    let mut db = Store::open_in_memory().unwrap();
    let c = db.create_campaign(&adventure_seed(), 42).unwrap();
    let before = db.snapshot(c).unwrap();
    let pc = before.entities[0].id;
    assert!(
        db.commit(
            record(c, "conflict"),
            vec![fact(pc, "open"), fact(pc, "closed")]
        )
        .is_err()
    );
    assert_eq!(db.snapshot(c).unwrap(), before);
    assert!(db.turn_by_op(&"conflict".into()).unwrap().is_none());
}
#[test]
fn duplicate_op_id_commits_once() {
    let mut db = Store::open_in_memory().unwrap();
    let c = db.create_campaign(&adventure_seed(), 42).unwrap();
    let pc = db.snapshot(c).unwrap().entities[0].id;
    let id = db
        .commit(record(c, "same"), vec![fact(pc, "open")])
        .unwrap();
    let before = db.snapshot(c).unwrap();
    assert!(
        matches!(db.commit(record(c, "same"), vec![fact(pc, "closed")]), Err(CommitError::DuplicateOp(found)) if found == id)
    );
    assert_eq!(db.snapshot(c).unwrap(), before);
    assert_eq!(db.turn_by_op(&"same".into()).unwrap().unwrap().id, id);
}
#[test]
fn campaign_isolation_and_scoped_op_ids() {
    let mut db = Store::open_in_memory().unwrap();
    let a = db.create_campaign(&adventure_seed(), 1).unwrap();
    let b = db.create_campaign(&adventure_seed(), 2).unwrap();
    db.commit(record(a, "same"), vec![]).unwrap();
    db.commit(record(b, "same"), vec![]).unwrap();
    assert!(db.commit(record(99999, "missing"), vec![]).is_err());
    assert_eq!(
        db.snapshot(a).unwrap().turn_seq,
        db.snapshot(b).unwrap().turn_seq
    );
}
#[test]
fn restart_keeps_state() {
    let path = std::env::temp_dir().join(format!(
        "threshold-store-{}-{:?}.db",
        std::process::id(),
        std::thread::current().id()
    ));
    let (c, before) = {
        let mut db = Store::open(&path).unwrap();
        let c = db.create_campaign(&adventure_seed(), 42).unwrap();
        let pc = db.snapshot(c).unwrap().entities[0].id;
        db.commit(record(c, "one"), vec![fact(pc, "open")]).unwrap();
        (c, db.snapshot(c).unwrap())
    };
    let db = Store::open(&path).unwrap();
    assert_eq!(db.snapshot(c).unwrap(), before);
    drop(db);
    std::fs::remove_file(path).unwrap();
}
#[test]
fn rebuild_matches_projections() {
    let mut db = Store::open_in_memory().unwrap();
    let c = db.create_campaign(&adventure_seed(), 42).unwrap();
    let pc = db.snapshot(c).unwrap().entities[0].id;
    db.commit(
        record(c, "one"),
        vec![
            LedgerOp::AddEntity {
                local: None,
                kind: EntityKind::Npc,
                name: "Bela".into(),
                age: Some(40),
                other_names: vec![],
                age_confirmed: false,
            },
            fact(pc, "open"),
        ],
    )
    .unwrap();
    let f = db.snapshot(c).unwrap().live_fact(pc, "door").unwrap().id;
    db.commit(
        record(c, "two"),
        vec![
            LedgerOp::OpenObstacle { fact_id: f },
            LedgerOp::AdvanceClock { minutes: 10 },
        ],
    )
    .unwrap();
    db.commit(
        record(c, "three"),
        vec![LedgerOp::ClearObstacle { obstacle_id: 1 }],
    )
    .unwrap();
    let before = db.snapshot(c).unwrap();
    db.rebuild_projections(c).unwrap();
    assert_eq!(db.snapshot(c).unwrap(), before);
    assert!(before.entity(Handle::Entity(pc)).is_some());
}

#[test]
fn replay_skips_ops_of_reverted_turn_in_hand_built_ledger() {
    let path = std::env::temp_dir().join(format!(
        "threshold-revert-{}-{:?}.db",
        std::process::id(),
        std::thread::current().id()
    ));
    let (c, pc) = {
        let mut db = Store::open(&path).unwrap();
        let c = db.create_campaign(&adventure_seed(), 42).unwrap();
        (c, db.snapshot(c).unwrap().entities[0].id)
    };
    {
        let sql = rusqlite::Connection::open(&path).unwrap();
        sql.execute("INSERT INTO turn(id,campaign_id,seq,op_id,kind,intent,proposal_json,clock_before,clock_after) VALUES(100,?1,2,'target','play','','{}',0,5)",[c]).unwrap();
        sql.execute("INSERT INTO turn(id,campaign_id,seq,op_id,kind,intent,proposal_json,clock_before,clock_after) VALUES(101,?1,3,'revert','correction','','{}',5,0)",[c]).unwrap();
        sql.execute(
            "INSERT INTO ledger(turn_id,op_json) VALUES(100,?1)",
            [serde_json::to_string(&fact(pc, "open")).unwrap()],
        )
        .unwrap();
        sql.execute(
            "INSERT INTO ledger(turn_id,op_json) VALUES(100,?1)",
            [serde_json::to_string(&LedgerOp::<EntityRef>::AdvanceClock { minutes: 5 }).unwrap()],
        )
        .unwrap();
        sql.execute(
            "INSERT INTO ledger(turn_id,op_json) VALUES(101,?1)",
            [serde_json::to_string(&LedgerOp::<EntityRef>::RevertTurn { turn_id: 100 }).unwrap()],
        )
        .unwrap();
    }
    let mut db = Store::open(&path).unwrap();
    let before_revert = db.snapshot_before(c, 3).unwrap();
    assert_eq!(before_revert.clock_min, 5);
    assert_eq!(
        before_revert.live_fact(pc, "door").unwrap().slot.value,
        "open"
    );
    db.rebuild_projections(c).unwrap();
    let after = db.snapshot(c).unwrap();
    assert_eq!(after.clock_min, 0);
    assert!(after.live_fact(pc, "door").is_none());
    assert_eq!(db.snapshot_before(c, 4).unwrap(), after);
    drop(db);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn replay_accepts_legacy_supersession_ledger() {
    let path = std::env::temp_dir().join(format!(
        "threshold-legacy-{}-{:?}.db",
        std::process::id(),
        std::thread::current().id()
    ));
    let mut db = Store::open(&path).unwrap();
    let c = db.create_campaign(&adventure_seed(), 1).unwrap();
    let pc = db.snapshot(c).unwrap().entities[0].id;
    db.commit(record(c, "old"), vec![fact(pc, "closed")])
        .unwrap();
    let old = db.snapshot(c).unwrap().live_fact(pc, "door").unwrap().id;
    db.commit(record(c, "new"), vec![supersede(old, pc, "open")])
        .unwrap();
    let v = db.snapshot(c).unwrap();
    let new = v.live_fact(pc, "door").unwrap().id;
    let sql = rusqlite::Connection::open(&path).unwrap();
    let turn = v.recent_turns.last().unwrap().id;
    sql.execute("UPDATE ledger SET op_json=?1 WHERE turn_id=?2", rusqlite::params![serde_json::json!({"op":{"supersede_fact":{"fact_id":old,"superseded_by":new}},"id":null}).to_string(),turn]).unwrap();
    sql.execute(
        "INSERT INTO ledger(turn_id,op_json) VALUES(?1,?2)",
        rusqlite::params![
            turn,
            serde_json::json!({"op":fact(pc,"open"),"id":new}).to_string()
        ],
    )
    .unwrap();
    db.rebuild_projections(c).unwrap();
    assert_eq!(db.snapshot(c).unwrap(), v);
    drop(sql);
    drop(db);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn cross_campaign_references_are_rejected_atomically() {
    let mut db = Store::open_in_memory().unwrap();
    let a = db.create_campaign(&adventure_seed(), 1).unwrap();
    let b = db.create_campaign(&adventure_seed(), 2).unwrap();
    let foreign = db.snapshot(b).unwrap().entities[0].id;
    let before = db.snapshot(a).unwrap();
    assert!(
        db.commit(record(a, "foreign"), vec![fact(foreign, "open")])
            .is_err()
    );
    assert_eq!(db.snapshot(a).unwrap(), before);
}

#[test]
fn duplicate_lookup_is_explicitly_scoped() {
    let mut db = Store::open_in_memory().unwrap();
    let a = db.create_campaign(&adventure_seed(), 1).unwrap();
    let b = db.create_campaign(&adventure_seed(), 2).unwrap();
    let one = db.commit(record(a, "same"), vec![]).unwrap();
    let two = db.commit(record(b, "same"), vec![]).unwrap();
    assert!(db.turn_by_op(&"same".into()).is_err());
    assert_eq!(
        db.turn_by_campaign_op(a, &"same".into())
            .unwrap()
            .unwrap()
            .id,
        one
    );
    assert_eq!(
        db.turn_by_campaign_op(b, &"same".into())
            .unwrap()
            .unwrap()
            .id,
        two
    );
}

#[test]
fn proposal_seen_by_pc_is_persistent_and_witnessed() {
    use threshold_core::domain::Knowing;
    let mut db = Store::open_in_memory().unwrap();
    let c = db.create_campaign(&adventure_seed(), 1).unwrap();
    let pc = db.snapshot(c).unwrap().entities[0].id;
    db.commit(
        record(c, "seen"),
        vec![LedgerOp::AddFact {
            slot: Slot {
                subject: EntityRef::Id(pc),
                key: "door".into(),
                value: "open".into(),
            },
            text: "Door open".into(),
            canon: Canon::Established,
            source: FactSource::Proposal,
            seen_by: vec![EntityRef::Id(pc)],
            causes: vec![],
        }],
    )
    .unwrap();
    let view = db.snapshot(c).unwrap();
    let f = view.live_fact(pc, "door").unwrap();
    assert_eq!(f.canon, Canon::Persistent);
    let id = f.id;
    assert_eq!(view.knowledge.len(), 1);
    assert_eq!(view.knowledge[0].holder, pc);
    assert_eq!(view.knowledge[0].fact_id, Some(id));
    assert_eq!(view.knowledge[0].how, Knowing::Witnessed);
    assert_eq!(view.knowledge[0].arrives_at, 0);
    assert_eq!(view.knowledge[0].learned_at, 0);
}

#[test]
fn supersede_then_replace_slot_is_atomic_and_rebuildable() {
    let mut db = Store::open_in_memory().unwrap();
    let c = db.create_campaign(&adventure_seed(), 1).unwrap();
    let pc = db.snapshot(c).unwrap().entities[0].id;
    db.commit(record(c, "old"), vec![fact(pc, "closed")])
        .unwrap();
    let old = db.snapshot(c).unwrap().live_fact(pc, "door").unwrap().id;
    db.commit(record(c, "new"), vec![supersede(old, pc, "open")])
        .unwrap();
    let before = db.snapshot(c).unwrap();
    assert_eq!(before.live_fact(pc, "door").unwrap().slot.value, "open");
    db.rebuild_projections(c).unwrap();
    assert_eq!(db.snapshot(c).unwrap(), before);
}

#[test]
fn possible_fact_seen_by_pc_becomes_established() {
    use threshold_core::domain::Knowing;
    let mut db = Store::open_in_memory().unwrap();
    let c = db.create_campaign(&adventure_seed(), 1).unwrap();
    let view = db.snapshot(c).unwrap();
    let pc = view.entities[0].id;
    let possible = view
        .live_facts
        .iter()
        .find(|f| f.canon == Canon::Possible)
        .unwrap()
        .id;
    db.commit(
        record(c, "seen"),
        vec![LedgerOp::Know {
            holder: EntityRef::Id(pc),
            fact_id: Some(possible),
            claim_id: None,
            how: Knowing::Witnessed,
            arrives_at: 0,
        }],
    )
    .unwrap();
    assert_eq!(
        db.snapshot(c)
            .unwrap()
            .fact(Handle::Fact(possible))
            .unwrap()
            .canon,
        Canon::Established
    );
}

#[test]
fn rebuild_preserves_claims_knowledge_attitudes_events_and_rolls() {
    use threshold_core::domain::{ClaimDraft, Knowing, Trigger};
    let mut db = Store::open_in_memory().unwrap();
    let c = db.create_campaign(&adventure_seed(), 1).unwrap();
    let pc = db.snapshot(c).unwrap().entities[0].id;
    db.commit(
        record(c, "one"),
        vec![
            LedgerOp::AddEntity {
                local: None,
                kind: EntityKind::Npc,
                name: "Bela".into(),
                age: Some(40),
                other_names: vec![],
                age_confirmed: false,
            },
            fact(pc, "closed"),
        ],
    )
    .unwrap();
    let v = db.snapshot(c).unwrap();
    let npc = v.entities.iter().find(|e| e.name == "Bela").unwrap().id;
    let old = v.live_fact(pc, "door").unwrap().id;
    let claim = ClaimDraft {
        speaker: EntityRef::Id(npc),
        slot: Slot {
            subject: EntityRef::Id(pc),
            key: "door".into(),
            value: "open".into(),
        },
        text: "Bela says the door is open".into(),
    };
    db.commit(
        record(c, "two"),
        vec![
            LedgerOp::AddClaim {
                claim,
                listeners: vec![],
                delay_min: 0,
            },
            LedgerOp::Know {
                holder: EntityRef::Id(pc),
                fact_id: None,
                claim_id: Some(1),
                how: Knowing::Told,
                arrives_at: 7,
            },
            LedgerOp::ShiftAttitude {
                from: EntityRef::Id(npc),
                to: EntityRef::Id(pc),
                delta: -3,
            },
            LedgerOp::OpenObstacle { fact_id: old },
            LedgerOp::ScheduleEvent {
                text: "Rain".into(),
                trigger: Trigger::At(5),
                actors: vec![EntityRef::Id(npc)],
                goal_id: None,
                cause_fact_id: Some(old),
                effects: vec![],
                seen_by: vec![EntityRef::Id(pc)],
                public: false,
            },
            LedgerOp::ScheduleEvent {
                text: "Wind".into(),
                trigger: Trigger::When(Slot {
                    subject: EntityRef::Id(pc),
                    key: "door".into(),
                    value: "open".into(),
                }),
                actors: vec![EntityRef::Id(npc)],
                goal_id: None,
                cause_fact_id: None,
                effects: vec![],
                seen_by: vec![],
                public: false,
            },
            LedgerOp::RecordRoll {
                seed: u64::MAX,
                difficulty: 4,
                against: None,
                stakes: serde_json::json!([]),
                factors: serde_json::json!([]),
                value: 6,
                outcome: "win".into(),
            },
        ],
    )
    .unwrap();
    let v = db.snapshot(c).unwrap();
    assert_eq!(v.claims[0].claim.slot.value, "open");
    assert_eq!(v.knowledge[0].arrives_at, 7);
    assert_eq!(v.attitudes[0].attitude, -3);
    let first = v.events_scheduled[0].id;
    let second = v.events_scheduled[1].id;
    let obstacle = v.obstacles_open[0].id;
    db.commit(
        record(c, "three"),
        vec![
            LedgerOp::ClearObstacle {
                obstacle_id: obstacle,
            },
            LedgerOp::FireEvent { event_id: first },
            LedgerOp::CancelEvent {
                event_id: second,
                cause_fact_id: old,
            },
            LedgerOp::AdvanceClock { minutes: 7 },
            supersede(old, pc, "open"),
        ],
    )
    .unwrap();
    let before = db.snapshot(c).unwrap();
    assert_eq!(before.clock_min, 7);
    assert!(before.obstacles_open.is_empty());
    assert!(before.events_scheduled.is_empty());
    assert_eq!(before.fact(Handle::Fact(old)).unwrap().slot.value, "closed");
    db.rebuild_projections(c).unwrap();
    assert_eq!(db.snapshot(c).unwrap(), before);
}

#[test]
fn revert_then_replace_slot_in_same_commit() {
    let mut db = Store::open_in_memory().unwrap();
    let c = db.create_campaign(&adventure_seed(), 1).unwrap();
    let pc = db.snapshot(c).unwrap().entities[0].id;
    let target = db
        .commit(record(c, "old"), vec![fact(pc, "closed")])
        .unwrap();
    db.commit(
        record(c, "correction"),
        vec![LedgerOp::RevertTurn { turn_id: target }, fact(pc, "open")],
    )
    .unwrap();
    assert_eq!(
        db.snapshot(c)
            .unwrap()
            .live_fact(pc, "door")
            .unwrap()
            .slot
            .value,
        "open"
    );
    let before = db.snapshot(c).unwrap();
    db.rebuild_projections(c).unwrap();
    assert_eq!(db.snapshot(c).unwrap(), before);
}

#[test]
fn reverting_a_revert_restores_original_turn() {
    let mut db = Store::open_in_memory().unwrap();
    let c = db.create_campaign(&adventure_seed(), 1).unwrap();
    let pc = db.snapshot(c).unwrap().entities[0].id;
    let target = db
        .commit(record(c, "old"), vec![fact(pc, "closed")])
        .unwrap();
    let undo = db
        .commit(
            record(c, "undo"),
            vec![LedgerOp::RevertTurn { turn_id: target }],
        )
        .unwrap();
    assert!(db.snapshot(c).unwrap().live_fact(pc, "door").is_none());
    db.commit(
        record(c, "undo-undo"),
        vec![LedgerOp::RevertTurn { turn_id: undo }],
    )
    .unwrap();
    assert_eq!(
        db.snapshot(c)
            .unwrap()
            .live_fact(pc, "door")
            .unwrap()
            .slot
            .value,
        "closed"
    );
}

#[test]
fn delayed_witness_promotes_possible_only_on_arrival() {
    use threshold_core::domain::Knowing;
    let mut db = Store::open_in_memory().unwrap();
    let c = db.create_campaign(&adventure_seed(), 1).unwrap();
    let v = db.snapshot(c).unwrap();
    let pc = v.entities[0].id;
    let f = v
        .live_facts
        .iter()
        .find(|f| f.canon == Canon::Possible)
        .unwrap()
        .id;
    db.commit(
        record(c, "know"),
        vec![LedgerOp::Know {
            holder: EntityRef::Id(pc),
            fact_id: Some(f),
            claim_id: None,
            how: Knowing::Witnessed,
            arrives_at: 5,
        }],
    )
    .unwrap();
    assert_eq!(
        db.snapshot(c).unwrap().fact(Handle::Fact(f)).unwrap().canon,
        Canon::Possible
    );
    db.commit(
        record(c, "time"),
        vec![LedgerOp::AdvanceClock { minutes: 5 }],
    )
    .unwrap();
    assert_eq!(
        db.snapshot(c).unwrap().fact(Handle::Fact(f)).unwrap().canon,
        Canon::Established
    );
}

#[test]
fn contradicted_delayed_witness_does_not_block_clock() {
    use threshold_core::domain::Knowing;
    let mut db = Store::open_in_memory().unwrap();
    let c = db.create_campaign(&adventure_seed(), 1).unwrap();
    let v = db.snapshot(c).unwrap();
    let pc = v.entities[0].id;
    let raven = v.live_fact(pc, "ally").unwrap().id;
    db.commit(
        record(c, "witness"),
        vec![LedgerOp::Know {
            holder: EntityRef::Id(pc),
            fact_id: Some(raven),
            claim_id: None,
            how: Knowing::Witnessed,
            arrives_at: 10,
        }],
    )
    .unwrap();
    db.commit(
        record(c, "wolf"),
        vec![LedgerOp::AddFact {
            slot: Slot {
                subject: EntityRef::Id(pc),
                key: "ally".into(),
                value: "wolf".into(),
            },
            text: "wolf".into(),
            canon: Canon::Established,
            source: FactSource::Proposal,
            seen_by: vec![],
            causes: vec![],
        }],
    )
    .unwrap();
    db.commit(
        record(c, "advance"),
        vec![LedgerOp::AdvanceClock { minutes: 10 }],
    )
    .unwrap();
    assert_eq!(
        db.snapshot(c)
            .unwrap()
            .fact(Handle::Fact(raven))
            .unwrap()
            .canon,
        Canon::Possible
    );
}

#[test]
fn simultaneous_possible_facts_in_one_slot_do_not_promote() {
    use threshold_core::domain::Knowing;
    let mut ch = adventure_seed();
    ch.proposed.push(SeedFact {
        subject: "Ada".into(),
        key: "ally".into(),
        value: "wolf".into(),
        text: "wolf".into(),
    });
    let mut db = Store::open_in_memory().unwrap();
    let c = db.create_campaign(&ch, 1).unwrap();
    let v = db.snapshot(c).unwrap();
    let pc = v.entities[0].id;
    let ids: Vec<_> = v
        .live_facts
        .iter()
        .filter(|f| f.slot.key == "ally")
        .map(|f| f.id)
        .collect();
    db.commit(
        record(c, "knowledge"),
        ids.iter()
            .map(|id| LedgerOp::Know {
                holder: EntityRef::Id(pc),
                fact_id: Some(*id),
                claim_id: None,
                how: Knowing::Witnessed,
                arrives_at: 10,
            })
            .collect(),
    )
    .unwrap();
    db.commit(
        record(c, "clock"),
        vec![LedgerOp::AdvanceClock { minutes: 10 }],
    )
    .unwrap();
    let v = db.snapshot(c).unwrap();
    assert!(
        ids.iter()
            .all(|id| v.fact(Handle::Fact(*id)).unwrap().canon == Canon::Possible)
    );
    db.rebuild_projections(c).unwrap();
    assert_eq!(db.snapshot(c).unwrap(), v);
}

#[test]
fn claim_listeners_receive_delayed_told_knowledge() {
    use threshold_core::domain::{ClaimDraft, Knowing};
    let mut db = Store::open_in_memory().unwrap();
    let c = db.create_campaign(&adventure_seed(), 1).unwrap();
    let pc = db.snapshot(c).unwrap().entities[0].id;
    db.commit(
        record(c, "npc"),
        vec![LedgerOp::AddEntity {
            local: None,
            kind: EntityKind::Npc,
            name: "Bela".into(),
            age: Some(40),
            other_names: vec![],
            age_confirmed: false,
        }],
    )
    .unwrap();
    let npc = db
        .snapshot(c)
        .unwrap()
        .entities
        .iter()
        .find(|e| e.name == "Bela")
        .unwrap()
        .id;
    db.commit(
        record(c, "claim"),
        vec![LedgerOp::AddClaim {
            claim: ClaimDraft {
                speaker: EntityRef::Id(npc),
                slot: Slot {
                    subject: EntityRef::Id(pc),
                    key: "door".into(),
                    value: "open".into(),
                },
                text: "open".into(),
            },
            listeners: vec![EntityRef::Id(pc), EntityRef::Id(npc)],
            delay_min: 120,
        }],
    )
    .unwrap();
    let v = db.snapshot(c).unwrap();
    assert_eq!(
        v.knowledge
            .iter()
            .filter(|k| k.how == Knowing::Told && k.arrives_at == 120)
            .count(),
        2
    );
    db.commit(
        record(c, "60"),
        vec![LedgerOp::AdvanceClock { minutes: 60 }],
    )
    .unwrap();
    assert_eq!(
        db.snapshot(c)
            .unwrap()
            .knowledge
            .iter()
            .filter(|k| k.how == Knowing::Told && k.arrives_at <= 60)
            .count(),
        0
    );
    db.commit(
        record(c, "120"),
        vec![LedgerOp::AdvanceClock { minutes: 60 }],
    )
    .unwrap();
    assert_eq!(
        db.snapshot(c)
            .unwrap()
            .knowledge
            .iter()
            .filter(|k| k.how == Knowing::Told && k.arrives_at <= 120)
            .count(),
        2
    );
    let before = db.snapshot(c).unwrap();
    db.rebuild_projections(c).unwrap();
    assert_eq!(db.snapshot(c).unwrap(), before);
}

#[test]
fn supersession_creates_replacement_without_predicted_id() {
    use threshold_core::domain::FactDraft;
    let mut db = Store::open_in_memory().unwrap();
    let c = db.create_campaign(&adventure_seed(), 1).unwrap();
    let pc = db.snapshot(c).unwrap().entities[0].id;
    db.commit(record(c, "old"), vec![fact(pc, "closed")])
        .unwrap();
    let old = db.snapshot(c).unwrap().live_fact(pc, "door").unwrap().id;
    db.commit(
        record(c, "new"),
        vec![LedgerOp::SupersedeFact {
            old,
            new: FactDraft {
                slot: Slot {
                    subject: EntityRef::Id(pc),
                    key: "door".into(),
                    value: "open".into(),
                },
                text: "open".into(),
                canon: Canon::Established,
                source: FactSource::Proposal,
                seen_by: vec![],
                causes: vec![],
            },
        }],
    )
    .unwrap();
    assert_eq!(
        db.snapshot(c)
            .unwrap()
            .live_fact(pc, "door")
            .unwrap()
            .slot
            .value,
        "open"
    );
}

#[test]
fn adventure_seed_subjects_require_pc_declared_entity_or_world() {
    use threshold_core::domain::SeedEntity;
    let mut db = Store::open_in_memory().unwrap();
    let mut ch = adventure_seed();
    ch.entities.push(SeedEntity {
        name: "Bela".into(),
        kind: EntityKind::Npc,
        age: Some(40),
        other_names: vec![],
        hidden: false,
    });
    ch.decided.push(SeedFact {
        subject: "Bela".into(),
        key: "role".into(),
        value: "guide".into(),
        text: "guide".into(),
    });
    ch.proposed.push(SeedFact {
        subject: "world".into(),
        key: "weather".into(),
        value: "rain".into(),
        text: "rain".into(),
    });
    let c = db.create_campaign(&ch, 1).unwrap();
    let v = db.snapshot(c).unwrap();
    assert!(
        v.entities
            .iter()
            .any(|e| e.name == "Bela" && e.kind == EntityKind::Npc)
    );
    assert!(
        v.entities
            .iter()
            .any(|e| e.name == "world" && e.kind == EntityKind::Place)
    );
    ch.decided[0].subject = "unknown".into();
    assert!(db.create_campaign(&ch, 2).is_err());
    ch.decided[0].subject = "Ada".into();
    ch.entities[0].age = None;
    assert!(db.create_campaign(&ch, 3).is_err());
    ch.entities[0].age = Some(40);
    ch.pc.name = "world".into();
    ch.decided[0].subject = "world".into();
    ch.proposed[0].subject = "world".into();
    assert!(db.create_campaign(&ch, 4).is_err());
}

#[test]
fn additional_projections_survive_rebuild() {
    use threshold_core::domain::GoalStatus;
    let path = std::env::temp_dir().join(format!(
        "threshold-extra-{}-{:?}.db",
        std::process::id(),
        std::thread::current().id()
    ));
    let mut db = Store::open(&path).unwrap();
    let c = db.create_campaign(&adventure_seed(), 1).unwrap();
    let v = db.snapshot(c).unwrap();
    let pc = v.entities[0].id;
    let cause = v.live_fact(pc, "home").unwrap().id;
    db.commit(
        record(c, "npc"),
        vec![LedgerOp::AddEntity {
            local: None,
            kind: EntityKind::Npc,
            name: "Bela".into(),
            age: Some(40),
            other_names: vec![],
            age_confirmed: false,
        }],
    )
    .unwrap();
    let npc = db
        .snapshot(c)
        .unwrap()
        .entities
        .iter()
        .find(|e| e.name == "Bela")
        .unwrap()
        .id;
    let target = db
        .commit(
            record(c, "extra"),
            vec![
                LedgerOp::AddVoiceCard {
                    npc: EntityRef::Id(npc),
                    habits: "pauses".into(),
                    vocabulary: "formal".into(),
                    samples: vec!["hello".into()],
                },
                LedgerOp::AddGoal {
                    npc: EntityRef::Id(npc),
                    text: "escape".into(),
                    against: Some(EntityRef::Id(pc)),
                },
                LedgerOp::AddFact {
                    slot: Slot {
                        subject: EntityRef::Id(pc),
                        key: "door".into(),
                        value: "open".into(),
                    },
                    text: "open".into(),
                    canon: Canon::Established,
                    source: FactSource::Proposal,
                    seen_by: vec![],
                    causes: vec![cause],
                },
                LedgerOp::RecordRoll {
                    seed: 1,
                    difficulty: 2,
                    against: None,
                    stakes: serde_json::json!({"win":"open"}),
                    factors: serde_json::json!(["help"]),
                    value: 3,
                    outcome: "win".into(),
                },
            ],
        )
        .unwrap();
    let goal = db.snapshot(c).unwrap().goals_active[0].id;
    db.commit(
        record(c, "correction"),
        vec![
            LedgerOp::SetGoalStatus {
                goal,
                status: GoalStatus::Done,
            },
            LedgerOp::RecordCorrection {
                target_turn: target,
                what_wrong: "door".into(),
                why: "mistake".into(),
            },
        ],
    )
    .unwrap();
    let sql = rusqlite::Connection::open(&path).unwrap();
    let read = || -> (String, String, String, String, i64, i64) {
        sql.query_row("SELECT (SELECT samples_json FROM voice_card), (SELECT status FROM goal), (SELECT stakes_json FROM roll), (SELECT factors_json FROM roll), (SELECT count(*) FROM fact_cause), (SELECT count(*) FROM correction)", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).unwrap()
    };
    let before = read();
    assert_eq!(before.0, "[\"hello\"]");
    assert_eq!(before.1, "done");
    assert_eq!((before.4, before.5), (1, 1));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&before.2).unwrap(),
        serde_json::json!({"win":"open"})
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&before.3).unwrap(),
        serde_json::json!(["help"])
    );
    db.rebuild_projections(c).unwrap();
    assert_eq!(read(), before);
    drop(sql);
    drop(db);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn recent_turns_exclude_reverted_and_keep_last_six_play_or_correction() {
    let mut db = Store::open_in_memory().unwrap();
    let c = db.create_campaign(&adventure_seed(), 1).unwrap();
    let mut turns = Vec::new();
    for n in 0..8 {
        turns.push(db.commit(record(c, &format!("play{n}")), vec![]).unwrap());
    }
    let mut ooc = record(c, "ooc");
    ooc.kind = TurnKind::Ooc;
    db.commit(ooc, vec![]).unwrap();
    let mut correction = record(c, "correction");
    correction.kind = TurnKind::Correction;
    db.commit(correction, vec![LedgerOp::RevertTurn { turn_id: turns[6] }])
        .unwrap();
    let v = db.snapshot(c).unwrap();
    let ops: Vec<_> = v
        .recent_turns
        .iter()
        .map(|t| t.record.op_id.as_str())
        .collect();
    assert_eq!(
        ops,
        vec!["play2", "play3", "play4", "play5", "play7", "correction"]
    );
    assert_eq!(v.turn_seq, 11);
    assert_eq!(
        db.snapshot_before(c, 11)
            .unwrap()
            .recent_turns
            .last()
            .unwrap()
            .record
            .op_id,
        "play7"
    );
}

#[test]
fn live_fact_normalizes_lookup_key() {
    let mut db = Store::open_in_memory().unwrap();
    let c = db.create_campaign(&adventure_seed(), 1).unwrap();
    let pc = db.snapshot(c).unwrap().entities[0].id;
    db.commit(
        record(c, "fact"),
        vec![LedgerOp::AddFact {
            slot: Slot {
                subject: EntityRef::Id(pc),
                key: "Door State".into(),
                value: "open".into(),
            },
            text: "open".into(),
            canon: Canon::Established,
            source: FactSource::Proposal,
            seen_by: vec![],
            causes: vec![],
        }],
    )
    .unwrap();
    assert!(
        db.snapshot(c)
            .unwrap()
            .live_fact(pc, " Door State ")
            .is_some()
    );
}

#[test]
fn future_schema_is_rejected_without_changing_version() {
    let path = std::env::temp_dir().join(format!(
        "threshold-future-{}-{:?}.db",
        std::process::id(),
        std::thread::current().id()
    ));
    let sql = rusqlite::Connection::open(&path).unwrap();
    sql.pragma_update(None, "user_version", 999).unwrap();
    drop(sql);
    let err = match Store::open(&path) {
        Ok(_) => panic!("future version accepted"),
        Err(e) => e,
    };
    assert!(err.to_string().contains("version"));
    let sql = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        sql.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        999
    );
    drop(sql);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn reverted_ids_are_not_reused_by_other_campaigns() {
    let mut db = Store::open_in_memory().unwrap();
    let a = db.create_campaign(&adventure_seed(), 1).unwrap();
    let b = db.create_campaign(&adventure_seed(), 2).unwrap();
    let pc_a = db.snapshot(a).unwrap().entities[0].id;
    let pc_b = db.snapshot(b).unwrap().entities[0].id;
    let target = db
        .commit(record(a, "old"), vec![fact(pc_a, "closed")])
        .unwrap();
    let old = db.snapshot(a).unwrap().live_fact(pc_a, "door").unwrap().id;
    let undo = db
        .commit(
            record(a, "undo"),
            vec![LedgerOp::RevertTurn { turn_id: target }],
        )
        .unwrap();
    db.commit(record(b, "other"), vec![fact(pc_b, "open")])
        .unwrap();
    let other = db.snapshot(b).unwrap().live_fact(pc_b, "door").unwrap().id;
    assert_ne!(old, other);
    db.commit(
        record(a, "restore"),
        vec![LedgerOp::RevertTurn { turn_id: undo }],
    )
    .unwrap();
    assert_eq!(
        db.snapshot(a).unwrap().live_fact(pc_a, "door").unwrap().id,
        old
    );
}

#[test]
fn cited_possible_fact_is_established_and_replays() {
    let mut db = Store::open_in_memory().unwrap();
    let c = db.create_campaign(&adventure_seed(), 1).unwrap();
    let before = db.snapshot(c).unwrap();
    let id = before
        .live_facts
        .iter()
        .find(|f| f.canon == Canon::Possible)
        .unwrap()
        .id;
    db.commit(
        record(c, "cite"),
        vec![LedgerOp::EstablishFact { fact_id: id }],
    )
    .unwrap();
    let after = db.snapshot(c).unwrap();
    assert_eq!(
        after.fact(Handle::Fact(id)).unwrap().canon,
        Canon::Established
    );
    db.rebuild_projections(c).unwrap();
    assert_eq!(db.snapshot(c).unwrap(), after);
}

#[test]
fn normalized_keys_share_the_same_live_slot() {
    let mut db = Store::open_in_memory().unwrap();
    let c = db.create_campaign(&adventure_seed(), 1).unwrap();
    let pc = db.snapshot(c).unwrap().entities[0].id;
    let a = LedgerOp::AddFact {
        slot: Slot {
            subject: EntityRef::Id(pc),
            key: "Door State".into(),
            value: "The Open".into(),
        },
        text: "open".into(),
        canon: Canon::Established,
        source: FactSource::Proposal,
        seen_by: vec![],
        causes: vec![],
    };
    db.commit(record(c, "one"), vec![a]).unwrap();
    let v = db.snapshot(c).unwrap();
    assert_eq!(v.live_fact(pc, "door_state").unwrap().slot.value, "open");
    let b = LedgerOp::AddFact {
        slot: Slot {
            subject: EntityRef::Id(pc),
            key: "door_state".into(),
            value: "closed".into(),
        },
        text: "closed".into(),
        canon: Canon::Established,
        source: FactSource::Proposal,
        seen_by: vec![],
        causes: vec![],
    };
    assert!(db.commit(record(c, "two"), vec![b]).is_err());
}

#[test]
fn event_effect_cannot_reference_another_campaign() {
    use threshold_core::domain::{Effect, Trigger};
    let mut db = Store::open_in_memory().unwrap();
    let a = db.create_campaign(&adventure_seed(), 1).unwrap();
    let b = db.create_campaign(&adventure_seed(), 2).unwrap();
    let foreign = db.snapshot(b).unwrap().entities[0].id;
    let before = db.snapshot(a).unwrap();
    let op = LedgerOp::ScheduleEvent {
        text: "wrong".into(),
        trigger: Trigger::At(1),
        actors: vec![],
        goal_id: None,
        cause_fact_id: None,
        effects: vec![Effect::SetFact {
            slot: Slot {
                subject: EntityRef::Id(foreign),
                key: "door".into(),
                value: "open".into(),
            },
            text: "wrong".into(),
            seen_by: vec![],
        }],
        seen_by: vec![],
        public: false,
    };
    assert!(db.commit(record(a, "foreign effect"), vec![op]).is_err());
    assert_eq!(db.snapshot(a).unwrap(), before);
}

#[test]
fn duplicate_and_failed_commits_leave_ledger_untouched_and_debug_survives() {
    use threshold_core::store::DebugEntry;
    let path = std::env::temp_dir().join(format!(
        "threshold-ledger-{}-{:?}.db",
        std::process::id(),
        std::thread::current().id()
    ));
    let mut db = Store::open(&path).unwrap();
    let c = db.create_campaign(&adventure_seed(), 5).unwrap();
    let pc = db.snapshot(c).unwrap().entities[0].id;
    let t = db
        .commit(
            record(c, "first"),
            vec![
                fact(pc, "open"),
                LedgerOp::RecordRoll {
                    seed: u64::MAX,
                    difficulty: 3,
                    against: None,
                    stakes: serde_json::json!([]),
                    factors: serde_json::json!([]),
                    value: 7,
                    outcome: "win".into(),
                },
            ],
        )
        .unwrap();
    db.set_narration(t, "The door opens.").unwrap();
    db.log_debug(DebugEntry {
        op_id: "failed".into(),
        step: "checks".into(),
        prompt: "prompt".into(),
        raw: "raw".into(),
        ms: 4,
    })
    .unwrap();
    let sql = rusqlite::Connection::open(&path).unwrap();
    let count: u64 = sql
        .query_row("SELECT count(*) FROM ledger", [], |r| r.get(0))
        .unwrap();
    assert!(
        matches!(db.commit(record(c,"first"),vec![fact(pc,"closed")]),Err(CommitError::DuplicateOp(id)) if id==t)
    );
    assert!(
        db.commit(record(c, "failed"), vec![fact(pc, "closed")])
            .is_err()
    );
    let after: u64 = sql
        .query_row("SELECT count(*) FROM ledger", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, after);
    assert_eq!(
        sql.query_row("SELECT count(*) FROM debug_log", [], |r| r.get::<_, u64>(0))
            .unwrap(),
        1
    );
    let before = db.snapshot(c).unwrap();
    db.rebuild_projections(c).unwrap();
    assert_eq!(db.snapshot(c).unwrap(), before);
    let roll: (i64, i32, String) = sql
        .query_row("SELECT seed,value,outcome FROM roll", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap();
    assert_eq!(roll, (-1, 7, "win".into()));
    assert_eq!(
        db.turn_by_campaign_op(c, &"first".into())
            .unwrap()
            .unwrap()
            .narration
            .as_deref(),
        Some("The door opens.")
    );
    drop(sql);
    drop(db);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn lists_campaigns_and_all_turns() {
    let mut store = Store::open_in_memory().unwrap();
    assert!(store.campaigns().unwrap().is_empty());
    let first = store.create_campaign(&adventure_seed(), 1).unwrap();
    let second = store.create_campaign(&adventure_seed(), 2).unwrap();
    for n in 0..8 {
        store
            .commit(record(first, &format!("op-{n}")), vec![])
            .unwrap();
    }
    assert_eq!(store.campaigns().unwrap(), vec![first, second]);
    let turns = store.turns(first).unwrap();
    // The Setup Turn, then eight more, in order: more than `recent_turns`.
    assert_eq!(turns.len(), 9);
    assert!(turns.windows(2).all(|w| w[0].seq < w[1].seq));
    assert_eq!(turns[0].record.kind, TurnKind::Setup);
    assert_eq!(turns[8].record.op_id, "op-7");
    assert_eq!(store.turns(second).unwrap().len(), 1);
}

#[test]
fn open_existing_refuses_a_foreign_file() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("open-existing");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let missing = dir.join("missing.sqlite");
    assert!(Store::open_existing(&missing).is_err());
    assert!(!missing.exists());

    let empty = dir.join("empty.sqlite");
    std::fs::write(&empty, b"").unwrap();
    assert!(Store::open_existing(&empty).is_err());
    assert_eq!(std::fs::metadata(&empty).unwrap().len(), 0);

    let real = dir.join("real.sqlite");
    let mut store = Store::open(&real).unwrap();
    store.create_campaign(&adventure_seed(), 1).unwrap();
    drop(store);
    assert_eq!(
        Store::open_existing(&real)
            .unwrap()
            .campaigns()
            .unwrap()
            .len(),
        1
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn version_1_file_is_refused() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("version-1");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("old.sqlite");
    let sql = rusqlite::Connection::open(&path).unwrap();
    sql.execute_batch(
        "CREATE TABLE campaign (id INTEGER PRIMARY KEY, adventure_seed_json TEXT NOT NULL, clock_min INTEGER NOT NULL, seed INTEGER NOT NULL);
         PRAGMA user_version=1;",
    )
    .unwrap();
    drop(sql);
    let bytes = std::fs::read(&path).unwrap();

    let err = match Store::open_existing(&path) {
        Ok(_) => panic!("a version 1 file opened"),
        Err(e) => e.to_string(),
    };
    assert!(err.contains("version 1"), "{err}");
    assert_eq!(std::fs::read(&path).unwrap(), bytes, "the file changed");
    assert!(Store::open(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), bytes, "the file changed");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn seed_other_names_resolve_and_are_known_facts() {
    use threshold_core::domain::{OtherName, SeedEntity};
    let mut db = Store::open_in_memory().unwrap();
    let mut seed = adventure_seed();
    seed.pc.other_names = vec![OtherName {
        name: "Jade".into(),
        known_to: vec!["Bela".into()],
    }];
    seed.entities = vec![
        SeedEntity {
            name: "Bela".into(),
            kind: EntityKind::Npc,
            age: Some(61),
            other_names: vec![OtherName {
                name: "the Old Chloé".into(),
                known_to: vec![],
            }],
            hidden: false,
        },
        SeedEntity {
            name: "Cato".into(),
            kind: EntityKind::Npc,
            age: Some(25),
            other_names: vec![],
            hidden: false,
        },
    ];
    // A fact may name its subject by an other name.
    seed.decided.push(SeedFact {
        subject: "jade".into(),
        key: "trade".into(),
        value: "courier".into(),
        text: "Ada carries letters.".into(),
    });
    let c = db.create_campaign(&seed, 7).unwrap();
    let view = db.snapshot(c).unwrap();
    let id = |name: &str| view.entity_by_name(name).unwrap().id;
    let (ada, bela, cato) = (id("Ada"), id("Bela"), id("Cato"));
    assert_eq!(id("JADE"), ada);
    assert_eq!(id("Old Chloe"), bela);
    let pc = view.entities.iter().find(|e| e.id == ada).unwrap();
    assert_eq!(
        (pc.age, pc.other_names.clone()),
        (Some(30), vec!["Jade".into()])
    );
    assert!(view.live_fact(ada, "trade").is_some());

    // One Persistent fact per other name, held by the bearer and `known_to`.
    let holders = |fact: u64| {
        let mut h: Vec<u64> = view
            .knowledge
            .iter()
            .filter(|k| k.fact_id == Some(fact))
            .map(|k| k.holder)
            .collect();
        h.sort();
        h
    };
    let alias = view.live_fact(ada, "alias_jade").unwrap();
    assert_eq!(alias.canon, Canon::Persistent);
    assert_eq!(alias.slot.value, "jade");
    assert_eq!(holders(alias.id), vec![ada, bela]);
    // The key takes the name's normal form: "Chloé" and "Chloe" share it.
    let alias = view.live_fact(bela, "alias_old_chloe").unwrap();
    assert_eq!(holders(alias.id), vec![bela]);
    assert!(view.knowledge.iter().all(|k| k.holder != cato));

    // The projections rebuild to the same view.
    db.rebuild_projections(c).unwrap();
    assert_eq!(db.snapshot(c).unwrap(), view);

    // A `known_to` name that resolves to nobody is refused.
    seed.pc.other_names[0].known_to = vec!["Nobody".into()];
    assert!(db.create_campaign(&seed, 8).is_err());
}

fn named(name: &str, other_names: &[&str]) -> threshold_core::domain::SeedEntity {
    threshold_core::domain::SeedEntity {
        name: name.into(),
        kind: EntityKind::Npc,
        age: Some(40),
        other_names: other_names
            .iter()
            .map(|n| threshold_core::domain::OtherName {
                name: (*n).into(),
                known_to: vec![],
            })
            .collect(),
        hidden: false,
    }
}

#[test]
fn seed_names_are_unique_across_main_and_other_names() {
    let mut db = Store::open_in_memory().unwrap();
    // A later other name takes an earlier main name.
    let mut seed = adventure_seed();
    seed.entities = vec![named("Bela", &["ADA"])];
    let err = db.create_campaign(&seed, 1).unwrap_err().to_string();
    assert!(err.contains("duplicate"), "{err}");
    // An earlier other name takes a later main name.
    let mut seed = adventure_seed();
    seed.entities = vec![named("Bela", &["Cato"]), named("Cato", &[])];
    assert!(db.create_campaign(&seed, 2).is_err());
    // Two entities share one other name.
    let mut seed = adventure_seed();
    seed.entities = vec![named("Bela", &["Red"]), named("Cato", &["red"])];
    assert!(db.create_campaign(&seed, 3).is_err());
    assert!(db.campaigns().unwrap().is_empty());
}

#[test]
fn same_other_name_twice_is_refused_with_a_clear_error() {
    let mut db = Store::open_in_memory().unwrap();
    let mut seed = adventure_seed();
    seed.entities = vec![named("Bela", &["Chloé", "Chloe"])];
    let err = db.create_campaign(&seed, 1).unwrap_err().to_string();
    assert!(!err.contains("UNIQUE"), "{err}");
    assert!(err.contains("Bela") && err.contains("Chloe"), "{err}");
    assert!(db.campaigns().unwrap().is_empty());
}

#[test]
fn world_is_reserved_for_other_names_too() {
    let mut db = Store::open_in_memory().unwrap();
    let mut seed = adventure_seed();
    seed.entities = vec![named("Bela", &["The World"])];
    let err = db.create_campaign(&seed, 1).unwrap_err().to_string();
    assert!(err.contains("world"), "{err}");
    assert!(db.campaigns().unwrap().is_empty());
}

#[test]
fn knowledge_rows_carry_their_turn() {
    let mut db = Store::open_in_memory().unwrap();
    let c = db.create_campaign(&adventure_seed(), 1).unwrap();
    let pc = db.snapshot(c).unwrap().entities[0].id;
    let turn = db
        .commit(
            record(c, "seen"),
            vec![LedgerOp::AddFact {
                slot: Slot {
                    subject: EntityRef::Id(pc),
                    key: "door".into(),
                    value: "open".into(),
                },
                text: "Door open".into(),
                canon: Canon::Established,
                source: FactSource::Proposal,
                seen_by: vec![EntityRef::Id(pc)],
                causes: vec![],
            }],
        )
        .unwrap();
    let row_turn = |db: &Store| {
        let view = db.snapshot(c).unwrap();
        let fact = view.live_fact(pc, "door").unwrap().id;
        view.knowledge
            .iter()
            .find(|k| k.holder == pc && k.fact_id == Some(fact))
            .expect("the PC knows the fact it witnessed")
            .turn
    };

    assert_eq!(row_turn(&db), turn);
    db.rebuild_projections(c).unwrap();
    assert_eq!(row_turn(&db), turn);
}

#[test]
fn version_2_file_is_refused() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("version-2");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("old.sqlite");
    let sql = rusqlite::Connection::open(&path).unwrap();
    sql.execute_batch(
        "CREATE TABLE campaign (id INTEGER PRIMARY KEY, adventure_seed_json TEXT NOT NULL, clock_min INTEGER NOT NULL, seed INTEGER NOT NULL);
         PRAGMA user_version=2;",
    )
    .unwrap();
    drop(sql);
    let bytes = std::fs::read(&path).unwrap();

    let err = match Store::open_existing(&path) {
        Ok(_) => panic!("a version 2 file opened"),
        Err(e) => e.to_string(),
    };
    assert!(err.contains("version 2"), "{err}");
    assert_eq!(std::fs::read(&path).unwrap(), bytes, "the file changed");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn new_file_has_the_player_tables() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("player-tables");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("new.sqlite");
    drop(Store::open(&path).unwrap());

    let sql = rusqlite::Connection::open(&path).unwrap();
    let version: u32 = sql
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 4);
    for table in ["player_note", "player_pin"] {
        let exists: bool = sql
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                [table],
                |r| r.get(0),
            )
            .unwrap();
        assert!(exists, "no table {table}");
    }
    drop(sql);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// B6 (QA, 2026-11-10): the World Clock starts at the seed's hour of Day 1;
/// a seed with no hour starts at 08:00; a rebuild keeps it; a seed saved
/// before the field existed still loads.
#[test]
fn clock_starts_at_the_seed_hour() {
    let mut db = Store::open_in_memory().unwrap();
    let mut evening = adventure_seed();
    evening.start.hour = Some(19);
    let c = db.create_campaign(&evening, 42).unwrap();
    assert_eq!(db.snapshot(c).unwrap().clock_min, 19 * 60);
    let before = db.snapshot(c).unwrap();
    db.rebuild_projections(c).unwrap();
    assert_eq!(db.snapshot(c).unwrap(), before);

    let mut unset = adventure_seed();
    unset.start.hour = None;
    let c = db.create_campaign(&unset, 42).unwrap();
    assert_eq!(db.snapshot(c).unwrap().clock_min, 8 * 60);

    let mut json = serde_json::to_value(adventure_seed()).unwrap();
    json["start"].as_object_mut().unwrap().remove("hour");
    let old: AdventureSeed = serde_json::from_value(json).unwrap();
    assert_eq!(old.start.hour, None);
    assert_eq!(old.start.start_hour(), 8);
}

/// Review of bug pass 4, blocker: `public` is a field of the event's
/// ledger op only. Schema 3 keeps its `world_event` projection, so an old
/// Campaign file opens; the store reads the flag from the op, after a
/// rebuild and in a past state too, and an op without it is not public.
#[test]
fn an_event_s_public_flag_lives_in_its_ledger_op() {
    use threshold_core::domain::Trigger;
    let mut db = Store::open_in_memory().unwrap();
    let c = db.create_campaign(&adventure_seed(), 1).unwrap();
    let orsel = db.snapshot(c).unwrap().entities[0].id;
    let event = |text: &str, public: bool| LedgerOp::ScheduleEvent {
        text: text.into(),
        trigger: Trigger::At(60),
        actors: vec![EntityRef::Id(orsel)],
        goal_id: None,
        cause_fact_id: None,
        effects: vec![],
        seen_by: vec![],
        public,
    };
    let turn = db
        .commit(
            record(c, "one"),
            vec![
                event("Orsel announces the toll", true),
                event("Orsel sulks", false),
            ],
        )
        .unwrap();
    let flags = |view: &threshold_core::store::WorldView| -> Vec<(String, bool)> {
        view.events_scheduled
            .iter()
            .map(|e| (e.text.clone(), e.public))
            .collect()
    };
    let expected = vec![
        ("Orsel announces the toll".to_owned(), true),
        ("Orsel sulks".to_owned(), false),
    ];
    assert_eq!(flags(&db.snapshot(c).unwrap()), expected);
    let seq = db.turn(turn).unwrap().unwrap().seq;
    assert_eq!(flags(&db.snapshot_before(c, seq + 1).unwrap()), expected);
    db.rebuild_projections(c).unwrap();
    assert_eq!(flags(&db.snapshot(c).unwrap()), expected);
    // An event written before the field existed has no `public` in its op:
    // a private event serializes the same way, and reads as not public.
    let ops = db.turn_ops(turn).unwrap();
    let json = serde_json::to_string(&ops[1]).unwrap();
    assert!(!json.contains("public"), "{json}");
    assert!(
        serde_json::to_string(&ops[0])
            .unwrap()
            .contains("\"public\":true")
    );
}

// --- Mature content module, Task 1: schema 4 and Age Confirmation -----------

#[test]
fn a_version_3_file_is_refused() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("version-3");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("old.sqlite");
    let sql = rusqlite::Connection::open(&path).unwrap();
    sql.execute_batch(
        "CREATE TABLE campaign (id INTEGER PRIMARY KEY, adventure_seed_json TEXT NOT NULL, clock_min INTEGER NOT NULL, seed INTEGER NOT NULL);
         PRAGMA user_version=3;",
    )
    .unwrap();
    drop(sql);
    let bytes = std::fs::read(&path).unwrap();

    let err = match Store::open_existing(&path) {
        Ok(_) => panic!("a version 3 file opened"),
        Err(e) => e.to_string(),
    };
    assert!(err.contains("version 3"), "{err}");
    let err = threshold_core::store::peek(&path).unwrap_err().to_string();
    assert!(err.contains("version 3"), "{err}");
    assert_eq!(std::fs::read(&path).unwrap(), bytes, "the file changed");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn new_file_is_version_4_with_the_new_columns() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("version-4");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("new.sqlite");
    let mut db = Store::open(&path).unwrap();
    let c = db.create_campaign(&adventure_seed(), 1).unwrap();
    db.commit(record(c, "op-1"), vec![]).unwrap();
    drop(db);

    let sql = rusqlite::Connection::open(&path).unwrap();
    let version: u32 = sql
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 4);
    assert_eq!(threshold_core::store::SCHEMA_VERSION, 4);
    for (table, column) in [
        ("campaign", "detail_level"),
        ("campaign", "scene_open_turn"),
        ("entity", "age_confirmed"),
        ("turn", "faded"),
    ] {
        let exists: bool = sql
            .query_row(
                &format!("SELECT EXISTS(SELECT 1 FROM pragma_table_info('{table}') WHERE name=?1)"),
                [column],
                |r| r.get(0),
            )
            .unwrap();
        assert!(exists, "no column {table}.{column}");
    }
    let (detail, scene): (String, Option<u64>) = sql
        .query_row(
            "SELECT detail_level,scene_open_turn FROM campaign WHERE id=?1",
            [c],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((detail.as_str(), scene), ("excluded", None));
    let unconfirmed: bool = sql
        .query_row(
            "SELECT COUNT(*)>0 AND MAX(age_confirmed)=0 FROM entity",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(unconfirmed);
    let unfaded: bool = sql
        .query_row("SELECT COUNT(*)>0 AND MAX(faded)=0 FROM turn", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(unfaded);
    drop(sql);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_detail_level_comes_from_the_seed_and_survives_rebuild() {
    use threshold_core::domain::DetailLevel;
    let mut db = Store::open_in_memory().unwrap();
    let mut seed = adventure_seed();
    seed.detail = DetailLevel::Implied;
    let c = db.create_campaign(&seed, 1).unwrap();
    assert_eq!(db.snapshot(c).unwrap().detail, DetailLevel::Implied);
    db.rebuild_projections(c).unwrap();
    assert_eq!(db.snapshot(c).unwrap().detail, DetailLevel::Implied);
    db.commit(record(c, "op-1"), vec![]).unwrap();
    assert_eq!(
        db.snapshot_before(c, 2).unwrap().detail,
        DetailLevel::Implied
    );
}

#[test]
fn an_op_without_the_flag_is_unconfirmed() {
    let op: LedgerOp = serde_json::from_value(serde_json::json!({
        "add_entity": {"kind": "npc", "name": "Orsel", "age": 40}
    }))
    .unwrap();
    let LedgerOp::AddEntity { age_confirmed, .. } = &op else {
        panic!("not an AddEntity: {op:?}");
    };
    assert!(!age_confirmed);

    let mut db = Store::open_in_memory().unwrap();
    let c = db.create_campaign(&adventure_seed(), 1).unwrap();
    db.commit(record(c, "op-1"), vec![op]).unwrap();
    let view = db.snapshot(c).unwrap();
    let orsel = view.entity_by_name("Orsel").unwrap();
    assert!(!orsel.age_confirmed);
    // The seed's PC is unconfirmed too: no path confirms it in this task.
    assert!(!view.entity_by_name("Ada").unwrap().age_confirmed);
}

#[test]
fn confirm_age_sets_the_flag_and_survives_rebuild() {
    let mut db = Store::open_in_memory().unwrap();
    let c = db.create_campaign(&adventure_seed(), 1).unwrap();
    db.commit(record(c, "op-1"), vec![local_npc(1)]).unwrap();
    let orsel = db.snapshot(c).unwrap().entity_by_name("Orsel").unwrap().id;
    db.commit(
        record(c, "op-2"),
        vec![LedgerOp::ConfirmAge {
            entity: EntityRef::Id(orsel),
        }],
    )
    .unwrap();
    let confirmed = |db: &Store| {
        db.snapshot(c)
            .unwrap()
            .entities
            .iter()
            .find(|e| e.id == orsel)
            .unwrap()
            .age_confirmed
    };
    assert!(confirmed(&db));
    db.rebuild_projections(c).unwrap();
    assert!(confirmed(&db));
    // A historical view before the confirmation does not hold it.
    let before = db.snapshot_before(c, 3).unwrap();
    assert!(!before.entity_by_name("Orsel").unwrap().age_confirmed);
    // The confirmation never changes the age.
    assert_eq!(
        db.snapshot(c).unwrap().entity_by_name("Orsel").unwrap().age,
        Some(40)
    );
}

#[test]
fn confirm_age_refuses_a_minor_a_place_and_no_age() {
    let mut db = Store::open_in_memory().unwrap();
    let c = db.create_campaign(&adventure_seed(), 1).unwrap();
    let add = |kind, name: &str, age| LedgerOp::AddEntity {
        local: None,
        kind,
        name: name.into(),
        age,
        other_names: vec![],
        age_confirmed: false,
    };
    db.commit(
        record(c, "op-1"),
        vec![
            add(EntityKind::Npc, "Pip", Some(17)),
            add(EntityKind::Place, "Gate", None),
            add(EntityKind::Item, "Lamp", None),
            add(EntityKind::Faction, "Guild", None),
        ],
    )
    .unwrap();
    let view = db.snapshot(c).unwrap();
    let id = |name: &str| view.entity_by_name(name).unwrap().id;
    for (n, entity) in [id("Pip"), id("Gate"), id("Lamp"), id("Guild"), 9_999]
        .into_iter()
        .enumerate()
    {
        let result = db.commit(
            record(c, &format!("confirm-{n}")),
            vec![LedgerOp::ConfirmAge {
                entity: EntityRef::Id(entity),
            }],
        );
        assert!(
            matches!(result, Err(CommitError::Store(_))),
            "entity {entity}: {result:?}"
        );
    }
    // Nothing committed: no Turn row, no flag.
    assert_eq!(db.snapshot(c).unwrap(), view);
}
