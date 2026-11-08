use threshold_core::{
    domain::{Canon, Charter, CharterFact, EntityKind, FactSource, Handle, LedgerOp, PcSeed, Slot},
    store::{CommitError, Store, TurnKind, TurnRecord},
};

fn charter() -> Charter {
    Charter {
        premise: "A frontier".into(),
        pc: PcSeed {
            name: "Ada".into(),
            adult: true,
            concept: "Scout".into(),
        },
        boundaries: vec![],
        decided: vec![CharterFact {
            subject: "Ada".into(),
            key: "Home".into(),
            value: "The Hill".into(),
            text: "Ada lives on the hill.".into(),
        }],
        proposed: vec![CharterFact {
            subject: "Ada".into(),
            key: "Ally".into(),
            value: "A Raven".into(),
            text: "A raven may help.".into(),
        }],
        story_language: "en".into(),
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
fn fact(subject: u64, value: &str) -> LedgerOp {
    LedgerOp::AddFact {
        slot: Slot {
            subject,
            key: "door".into(),
            value: value.into(),
        },
        text: value.into(),
        canon: Canon::Established,
        source: FactSource::Proposal,
        seen_by: vec![],
    }
}
#[test]
fn migrations_apply_on_empty_db() {
    let mut db = Store::open_in_memory().unwrap();
    let id = db.create_campaign(&charter(), 42).unwrap();
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
fn npc_without_adult_status_is_refused() {
    let mut db = Store::open_in_memory().unwrap();
    let c = db.create_campaign(&charter(), 42).unwrap();
    let before = db.snapshot(c).unwrap();
    assert!(
        db.commit(
            record(c, "bad"),
            vec![LedgerOp::AddEntity {
                kind: EntityKind::Npc,
                name: "Stranger".into(),
                adult: None
            }]
        )
        .is_err()
    );
    assert_eq!(db.snapshot(c).unwrap(), before);
    assert!(db.turn_by_op(&"bad".into()).unwrap().is_none());
}
#[test]
fn one_live_value_per_slot() {
    let mut db = Store::open_in_memory().unwrap();
    let c = db.create_campaign(&charter(), 42).unwrap();
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
    let c = db.create_campaign(&charter(), 42).unwrap();
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
    let a = db.create_campaign(&charter(), 1).unwrap();
    let b = db.create_campaign(&charter(), 2).unwrap();
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
        let c = db.create_campaign(&charter(), 42).unwrap();
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
    let c = db.create_campaign(&charter(), 42).unwrap();
    let pc = db.snapshot(c).unwrap().entities[0].id;
    db.commit(
        record(c, "one"),
        vec![
            LedgerOp::AddEntity {
                kind: EntityKind::Npc,
                name: "Bela".into(),
                adult: Some(true),
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
        let c = db.create_campaign(&charter(), 42).unwrap();
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
            [serde_json::to_string(&LedgerOp::AdvanceClock { minutes: 5 }).unwrap()],
        )
        .unwrap();
        sql.execute(
            "INSERT INTO ledger(turn_id,op_json) VALUES(101,?1)",
            [serde_json::to_string(&LedgerOp::RevertTurn { turn_id: 100 }).unwrap()],
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
fn cross_campaign_references_are_rejected_atomically() {
    let mut db = Store::open_in_memory().unwrap();
    let a = db.create_campaign(&charter(), 1).unwrap();
    let b = db.create_campaign(&charter(), 2).unwrap();
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
    let a = db.create_campaign(&charter(), 1).unwrap();
    let b = db.create_campaign(&charter(), 2).unwrap();
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
    let c = db.create_campaign(&charter(), 1).unwrap();
    let pc = db.snapshot(c).unwrap().entities[0].id;
    db.commit(
        record(c, "seen"),
        vec![LedgerOp::AddFact {
            slot: Slot {
                subject: pc,
                key: "door".into(),
                value: "open".into(),
            },
            text: "Door open".into(),
            canon: Canon::Established,
            source: FactSource::Proposal,
            seen_by: vec![pc],
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
    let c = db.create_campaign(&charter(), 1).unwrap();
    let pc = db.snapshot(c).unwrap().entities[0].id;
    db.commit(record(c, "old"), vec![fact(pc, "closed")])
        .unwrap();
    let old = db.snapshot(c).unwrap().live_fact(pc, "door").unwrap().id;
    let next = db.next_fact_id().unwrap();
    db.commit(
        record(c, "new"),
        vec![
            LedgerOp::SupersedeFact {
                fact_id: old,
                superseded_by: next,
            },
            fact(pc, "open"),
        ],
    )
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
    let c = db.create_campaign(&charter(), 1).unwrap();
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
            holder: pc,
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
    let c = db.create_campaign(&charter(), 1).unwrap();
    let pc = db.snapshot(c).unwrap().entities[0].id;
    db.commit(
        record(c, "one"),
        vec![
            LedgerOp::AddEntity {
                kind: EntityKind::Npc,
                name: "Bela".into(),
                adult: Some(true),
            },
            fact(pc, "closed"),
        ],
    )
    .unwrap();
    let v = db.snapshot(c).unwrap();
    let npc = v.entities.iter().find(|e| e.name == "Bela").unwrap().id;
    let old = v.live_fact(pc, "door").unwrap().id;
    let claim = ClaimDraft {
        speaker: npc,
        slot: Slot {
            subject: pc,
            key: "door".into(),
            value: "open".into(),
        },
        text: "Bela says the door is open".into(),
    };
    db.commit(
        record(c, "two"),
        vec![
            LedgerOp::AddClaim { claim },
            LedgerOp::Know {
                holder: pc,
                fact_id: None,
                claim_id: Some(1),
                how: Knowing::Told,
                arrives_at: 7,
            },
            LedgerOp::ShiftAttitude {
                from: npc,
                to: pc,
                delta: -3,
            },
            LedgerOp::OpenObstacle { fact_id: old },
            LedgerOp::ScheduleEvent {
                text: "Rain".into(),
                trigger: Trigger::At(5),
                actors: vec![npc],
                goal_id: None,
                cause_fact_id: Some(old),
                effects: vec![],
                seen_by: vec![pc],
            },
            LedgerOp::ScheduleEvent {
                text: "Wind".into(),
                trigger: Trigger::When(Slot {
                    subject: pc,
                    key: "door".into(),
                    value: "open".into(),
                }),
                actors: vec![npc],
                goal_id: None,
                cause_fact_id: None,
                effects: vec![],
                seen_by: vec![],
            },
            LedgerOp::RecordRoll {
                seed: u64::MAX,
                difficulty: 4,
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
            LedgerOp::SupersedeFact {
                fact_id: old,
                superseded_by: old + 1,
            },
            fact(pc, "open"),
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
    let c = db.create_campaign(&charter(), 1).unwrap();
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
    let c = db.create_campaign(&charter(), 1).unwrap();
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
    let c = db.create_campaign(&charter(), 1).unwrap();
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
            holder: pc,
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
fn reverted_ids_are_not_reused_by_other_campaigns() {
    let mut db = Store::open_in_memory().unwrap();
    let a = db.create_campaign(&charter(), 1).unwrap();
    let b = db.create_campaign(&charter(), 2).unwrap();
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
    let c = db.create_campaign(&charter(), 1).unwrap();
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
    let c = db.create_campaign(&charter(), 1).unwrap();
    let pc = db.snapshot(c).unwrap().entities[0].id;
    let a = LedgerOp::AddFact {
        slot: Slot {
            subject: pc,
            key: "Door State".into(),
            value: "The Open".into(),
        },
        text: "open".into(),
        canon: Canon::Established,
        source: FactSource::Proposal,
        seen_by: vec![],
    };
    db.commit(record(c, "one"), vec![a]).unwrap();
    let v = db.snapshot(c).unwrap();
    assert_eq!(v.live_fact(pc, "door_state").unwrap().slot.value, "open");
    let b = LedgerOp::AddFact {
        slot: Slot {
            subject: pc,
            key: "door_state".into(),
            value: "closed".into(),
        },
        text: "closed".into(),
        canon: Canon::Established,
        source: FactSource::Proposal,
        seen_by: vec![],
    };
    assert!(db.commit(record(c, "two"), vec![b]).is_err());
}

#[test]
fn event_effect_cannot_reference_another_campaign() {
    use threshold_core::domain::{Effect, Trigger};
    let mut db = Store::open_in_memory().unwrap();
    let a = db.create_campaign(&charter(), 1).unwrap();
    let b = db.create_campaign(&charter(), 2).unwrap();
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
                subject: foreign,
                key: "door".into(),
                value: "open".into(),
            },
            text: "wrong".into(),
            seen_by: vec![],
        }],
        seen_by: vec![],
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
    let c = db.create_campaign(&charter(), 5).unwrap();
    let pc = db.snapshot(c).unwrap().entities[0].id;
    let t = db
        .commit(
            record(c, "first"),
            vec![
                fact(pc, "open"),
                LedgerOp::RecordRoll {
                    seed: u64::MAX,
                    difficulty: 3,
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
