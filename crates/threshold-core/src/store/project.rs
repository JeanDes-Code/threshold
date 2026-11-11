use super::*;
use crate::{
    domain::{Canon, EntityKind, FactSource, Knowing, LedgerOp, Trigger},
    normalize,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::collections::HashSet;

pub(super) fn enum_text<T: Serialize>(value: &T) -> Result<String, StoreError> {
    serde_json::to_value(value)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| StoreError::Invalid("expected enum string".into()))
}
fn parse<T: DeserializeOwned>(s: String) -> Result<T, StoreError> {
    Ok(serde_json::from_str(&s)?)
}
fn enum_parse<T: DeserializeOwned>(s: String) -> Result<T, StoreError> {
    Ok(serde_json::from_value(Value::String(s))?)
}
fn owned(db: &Connection, sql: &str, id: u64, campaign: u64) -> Result<(), StoreError> {
    let ok: bool = db.query_row(sql, params![id, campaign], |r| r.get(0))?;
    if ok {
        Ok(())
    } else {
        Err(StoreError::Invalid(
            "reference does not belong to campaign".into(),
        ))
    }
}
fn entity(db: &Connection, id: u64, c: u64) -> Result<(), StoreError> {
    owned(
        db,
        "SELECT EXISTS(SELECT 1 FROM entity WHERE id=?1 AND campaign_id=?2)",
        id,
        c,
    )
}
fn fact(db: &Connection, id: u64, c: u64) -> Result<(), StoreError> {
    owned(
        db,
        "SELECT EXISTS(SELECT 1 FROM fact f JOIN entity e ON e.id=f.subject_id WHERE f.id=?1 AND e.campaign_id=?2)",
        id,
        c,
    )
}
fn claim(db: &Connection, id: u64, c: u64) -> Result<(), StoreError> {
    owned(
        db,
        "SELECT EXISTS(SELECT 1 FROM claim a JOIN turn t ON t.id=a.turn_id WHERE a.id=?1 AND t.campaign_id=?2)",
        id,
        c,
    )
}
fn obstacle(db: &Connection, id: u64, c: u64) -> Result<(), StoreError> {
    owned(
        db,
        "SELECT EXISTS(SELECT 1 FROM obstacle a JOIN turn t ON t.id=a.opened_turn WHERE a.id=?1 AND t.campaign_id=?2)",
        id,
        c,
    )
}
fn event(db: &Connection, id: u64, c: u64) -> Result<(), StoreError> {
    owned(
        db,
        "SELECT EXISTS(SELECT 1 FROM world_event a JOIN turn t ON t.id=a.scheduled_turn WHERE a.id=?1 AND t.campaign_id=?2)",
        id,
        c,
    )
}
fn normalized_slot(slot: &Slot<EntityId>) -> Slot<EntityId> {
    Slot {
        subject: slot.subject,
        key: normalize::key(&slot.key),
        value: normalize::value(&slot.value),
    }
}
fn update_one(db: &Connection, sql: &str, id: u64, turn: u64) -> Result<(), StoreError> {
    if db.execute(sql, params![id, turn])? != 1 {
        return Err(StoreError::Invalid("target is not active".into()));
    }
    Ok(())
}

fn update_one_single(db: &Connection, sql: &str, id: u64) -> Result<(), StoreError> {
    if db.execute(sql, [id])? != 1 {
        return Err(StoreError::Invalid("target is not active".into()));
    }
    Ok(())
}

fn next_id(db: &Connection, table: &str, path: &str) -> Result<u64, StoreError> {
    let current: u64 = db.query_row(
        &format!("SELECT COALESCE(MAX(id),0) FROM {table}"),
        [],
        |r| r.get(0),
    )?;
    let previous: u64 = db.query_row("SELECT COALESCE(MAX(CAST(json_extract(op_json,'$.id') AS INTEGER)),0) FROM ledger WHERE json_type(op_json,?1) IS NOT NULL OR (?1='$.op.add_fact' AND json_type(op_json,'$.op.supersede_fact') IS NOT NULL)", [path], |r| r.get(0))?;
    current
        .max(previous)
        .checked_add(1)
        .ok_or_else(|| StoreError::Invalid("id space exhausted".into()))
}

pub(super) fn append(
    db: &Connection,
    c: u64,
    turn: u64,
    op: &LedgerOp<EntityId>,
) -> Result<Option<u64>, StoreError> {
    let (table, path) = match op {
        LedgerOp::AddEntity { .. } => (Some("entity"), "$.op.add_entity"),
        LedgerOp::AddFact { .. } => (Some("fact"), "$.op.add_fact"),
        LedgerOp::SupersedeFact { .. } => (Some("fact"), "$.op.add_fact"),
        LedgerOp::AddClaim { .. } => (Some("claim"), "$.op.add_claim"),
        LedgerOp::OpenObstacle { .. } => (Some("obstacle"), "$.op.open_obstacle"),
        LedgerOp::ScheduleEvent { .. } => (Some("world_event"), "$.op.schedule_event"),
        LedgerOp::RecordRoll { .. } => (Some("roll"), "$.op.record_roll"),
        LedgerOp::AddGoal { .. } => (Some("goal"), "$.op.add_goal"),
        LedgerOp::RecordCorrection { .. } => (Some("correction"), "$.op.record_correction"),
        _ => (None, ""),
    };
    let reserved = table.map(|table| next_id(db, table, path)).transpose()?;
    let id = apply(db, c, turn, op, reserved)?;
    let data = json!({"op":op,"id":id});
    db.execute(
        "INSERT INTO ledger(turn_id,op_json) VALUES(?1,?2)",
        params![turn, data.to_string()],
    )?;
    Ok(id)
}
fn apply(
    db: &Connection,
    c: u64,
    turn: u64,
    op: &LedgerOp<EntityId>,
    id: Option<u64>,
) -> Result<Option<u64>, StoreError> {
    let made = match op {
        LedgerOp::AddEntity {
            kind,
            name,
            age,
            other_names,
            age_confirmed,
            ..
        } => {
            db.execute("INSERT INTO entity(id,campaign_id,kind,name,norm_name,age,other_names_json,age_confirmed) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![id,c,enum_text(kind)?,name,normalize::name(name),age,serde_json::to_string(other_names)?,age_confirmed])?;
            true
        }
        // Only a PC or an NPC whose stored age is `ADULT_AGE` or more can be
        // confirmed as an adult (plan deviation 6: `Store::commit` is
        // public, so the store holds the rule too).
        LedgerOp::ConfirmAge { entity: target } => {
            entity(db, *target, c)?;
            if db.execute(
                "UPDATE entity SET age_confirmed=1 WHERE id=?1 AND campaign_id=?2 AND kind IN ('pc','npc') AND age IS NOT NULL AND age>=?3",
                params![target, c, crate::domain::ADULT_AGE],
            )? != 1
            {
                return Err(StoreError::Invalid(
                    "only a character with a stored adult age can be confirmed as an adult"
                        .into(),
                ));
            }
            false
        }
        LedgerOp::AddFact {
            slot,
            text,
            canon,
            source,
            seen_by,
            causes,
        } => {
            entity(db, slot.subject, c)?;
            for holder in seen_by {
                entity(db, *holder, c)?;
            }
            for cause in causes {
                fact(db, *cause, c)?;
            }
            // A fact a Turn writes (proposal, Check stake, Correction) is
            // Persistent when the PC witnesses it, Established otherwise.
            // Adventure Seed and World Event facts arrive with their canon,
            // and so does a Possible departure record a Turn writes for a
            // `leaves` the PC cannot follow (bug pass 5, M1).
            let actual_canon = if matches!(
                source,
                FactSource::Proposal | FactSource::Stake | FactSource::Correction
            ) && *canon != Canon::Possible
            {
                let mut pc_seen = false;
                for holder in seen_by {
                    pc_seen |= db.query_row(
                        "SELECT EXISTS(SELECT 1 FROM entity WHERE id=?1 AND kind='pc')",
                        [holder],
                        |r| r.get::<_, bool>(0),
                    )?;
                }
                if pc_seen {
                    Canon::Persistent
                } else {
                    Canon::Established
                }
            } else {
                *canon
            };
            // State keys hold changing state and are never Persistent, whatever
            // the source (Adventure Seed included) or the witnesses.
            // The outcome slot of an Open Item stays Established too (ADR
            // 0007, changed after review): the model judges that the Intent
            // settled the item, so a wrong close must stay repairable in play.
            let outcome_slot = || -> Result<bool, StoreError> {
                let open = format!(
                    "{}{}",
                    crate::domain::OPEN_KEY_PREFIX,
                    normalize::key(&slot.key)
                );
                Ok(db.query_row(
                    "SELECT EXISTS(SELECT 1 FROM fact WHERE subject_id=?1 AND key=?2)",
                    params![slot.subject, open],
                    |r| r.get::<_, bool>(0),
                )?)
            };
            let actual_canon = if actual_canon == Canon::Persistent
                && (crate::domain::is_state_key(&slot.key) || outcome_slot()?)
            {
                Canon::Established
            } else {
                actual_canon
            };
            db.execute("INSERT INTO fact(id,subject_id,key,value,text,canon,source,created_turn) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![id,slot.subject,normalize::key(&slot.key),normalize::value(&slot.value),text,enum_text(&actual_canon)?,enum_text(source)?,turn])?;
            let fact_id = db.last_insert_rowid();
            // A departure record (a Possible `location` with no place,
            // `world::with_witnessed_events`, or `leaves` to a place the PC
            // cannot name, bug pass 5) lasts until the next canon write on
            // its slot: that write supersedes it. So a return retires it, and
            // a slot never holds two live records (review of bug pass 4, M2).
            if actual_canon != Canon::Possible {
                db.execute(
                    "UPDATE fact SET superseded_by=?1 WHERE subject_id=?2 AND key=?3 AND key='location' AND value='' AND canon='possible' AND superseded_by IS NULL AND id<>?1",
                    params![fact_id, slot.subject, normalize::key(&slot.key)],
                )?;
            }
            for cause in causes {
                db.execute(
                    "INSERT INTO fact_cause(fact_id,cause_fact_id) VALUES(?1,?2)",
                    params![fact_id, cause],
                )?;
            }
            let mut holders = HashSet::new();
            for holder in seen_by {
                if holders.insert(*holder) {
                    db.execute(
                        "INSERT INTO fact_visible(fact_id,holder_id) VALUES(?1,?2)",
                        params![fact_id, holder],
                    )?;
                }
            }
            return Ok(Some(fact_id as u64));
        }
        LedgerOp::EstablishFact { fact_id } => {
            fact(db, *fact_id, c)?;
            if db.execute("UPDATE fact SET canon='established' WHERE id=?1 AND canon='possible' AND superseded_by IS NULL",[fact_id])? != 1 { return Err(StoreError::Invalid("fact is not live and possible".into())); }
            false
        }
        LedgerOp::SupersedeFact { old, new } => {
            fact(db, *old, c)?;
            let replacement =
                id.ok_or_else(|| StoreError::Invalid("missing replacement id".into()))?;
            if db.execute(
                "UPDATE fact SET superseded_by=?2 WHERE id=?1 AND superseded_by IS NULL",
                params![old, replacement],
            )? != 1
            {
                return Err(StoreError::Invalid("fact is already superseded".into()));
            }
            return apply(
                db,
                c,
                turn,
                &LedgerOp::<EntityId>::AddFact {
                    slot: new.slot.clone(),
                    text: new.text.clone(),
                    canon: new.canon,
                    source: new.source,
                    seen_by: new.seen_by.clone(),
                    causes: new.causes.clone(),
                },
                id,
            );
        }
        LedgerOp::AddClaim { claim, .. } => {
            entity(db, claim.speaker, c)?;
            entity(db, claim.slot.subject, c)?;
            db.execute("INSERT INTO claim(id,speaker_id,subject_id,key,value,text,turn_id) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![id,claim.speaker,claim.slot.subject,normalize::key(&claim.slot.key),normalize::value(&claim.slot.value),claim.text,turn])?;
            true
        }
        LedgerOp::AddVoiceCard {
            npc,
            habits,
            vocabulary,
            samples,
        } => {
            entity(db, *npc, c)?;
            db.execute(
                "INSERT INTO voice_card(npc_id,habits,vocabulary,samples_json) VALUES(?1,?2,?3,?4)",
                params![npc, habits, vocabulary, serde_json::to_string(samples)?],
            )?;
            false
        }
        LedgerOp::AddGoal { npc, text, against } => {
            entity(db, *npc, c)?;
            if let Some(other) = against {
                entity(db, *other, c)?;
            }
            db.execute(
                "INSERT INTO goal(id,npc_id,text,against_id,status) VALUES(?1,?2,?3,?4,'active')",
                params![id, npc, text, against],
            )?;
            true
        }
        LedgerOp::SetGoalStatus { goal, status } => {
            owned(
                db,
                "SELECT EXISTS(SELECT 1 FROM goal g JOIN entity e ON e.id=g.npc_id WHERE g.id=?1 AND e.campaign_id=?2)",
                *goal,
                c,
            )?;
            db.execute(
                "UPDATE goal SET status=?2 WHERE id=?1",
                params![goal, enum_text(status)?],
            )?;
            false
        }
        LedgerOp::RecordCorrection {
            target_turn,
            what_wrong,
            why,
        } => {
            owned(
                db,
                "SELECT EXISTS(SELECT 1 FROM turn WHERE id=?1 AND campaign_id=?2)",
                *target_turn,
                c,
            )?;
            db.execute("INSERT INTO correction(id,turn_id,target_turn_id,what_wrong,why) VALUES(?1,?2,?3,?4,?5)",params![id,turn,target_turn,what_wrong,why])?;
            true
        }
        LedgerOp::Know {
            holder,
            fact_id,
            claim_id,
            how,
            arrives_at,
        } => {
            entity(db, *holder, c)?;
            if let Some(f) = fact_id {
                fact(db, *f, c)?;
            }
            if let Some(a) = claim_id {
                claim(db, *a, c)?;
            }
            db.execute("INSERT INTO knowledge(holder_id,fact_id,claim_id,how,learned_at,arrives_at,turn_id) VALUES(?1,?2,?3,?4,(SELECT clock_min FROM campaign WHERE id=?5),?6,?7)",params![holder,fact_id,claim_id,enum_text(how)?,c,arrives_at,turn])?;
            let witnessed_now: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM entity e JOIN campaign camp ON camp.id=e.campaign_id WHERE e.id=?1 AND e.kind='pc' AND ?2<=camp.clock_min)", params![holder, arrives_at], |r| r.get(0))?;
            if *how == Knowing::Witnessed && fact_id.is_some() && witnessed_now {
                promote_seen(db, c, None, *fact_id)?;
            }
            false
        }
        LedgerOp::ShiftAttitude { from, to, delta } => {
            entity(db, *from, c)?;
            entity(db, *to, c)?;
            db.execute("INSERT INTO relationship(from_id,to_id,attitude) VALUES(?1,?2,MAX(-5,MIN(5,?3))) ON CONFLICT(from_id,to_id) DO UPDATE SET attitude=MAX(-5,MIN(5,attitude+?3))",params![from,to,delta])?;
            false
        }
        LedgerOp::OpenObstacle { fact_id } => {
            fact(db, *fact_id, c)?;
            db.execute(
                "INSERT INTO obstacle(id,fact_id,opened_turn) VALUES(?1,?2,?3)",
                params![id, fact_id, turn],
            )?;
            true
        }
        LedgerOp::ClearObstacle { obstacle_id } => {
            obstacle(db, *obstacle_id, c)?;
            update_one(
                db,
                "UPDATE obstacle SET cleared_turn=?2 WHERE id=?1 AND cleared_turn IS NULL",
                *obstacle_id,
                turn,
            )?;
            false
        }
        LedgerOp::ScheduleEvent {
            text,
            trigger,
            actors,
            goal_id,
            cause_fact_id,
            effects,
            seen_by,
            public: _,
        } => {
            for x in actors.iter().chain(seen_by) {
                entity(db, *x, c)?;
            }
            if let Trigger::When(s) = trigger {
                entity(db, s.subject, c)?;
            }
            if let Some(f) = cause_fact_id {
                fact(db, *f, c)?;
            }
            let mut normalized_effects = Vec::new();
            for effect in effects {
                normalized_effects.push(match effect {
                    Effect::SetFact {
                        slot,
                        text,
                        seen_by,
                    } => {
                        entity(db, slot.subject, c)?;
                        for holder in seen_by {
                            entity(db, *holder, c)?;
                        }
                        Effect::SetFact {
                            slot: normalized_slot(slot),
                            text: text.clone(),
                            seen_by: seen_by.clone(),
                        }
                    }
                    Effect::ShiftAttitude { from, to, delta } => {
                        entity(db, *from, c)?;
                        entity(db, *to, c)?;
                        Effect::ShiftAttitude {
                            from: *from,
                            to: *to,
                            delta: *delta,
                        }
                    }
                    Effect::Tell {
                        to,
                        claim,
                        delay_min,
                    } => {
                        entity(db, *to, c)?;
                        entity(db, claim.speaker, c)?;
                        entity(db, claim.slot.subject, c)?;
                        Effect::Tell {
                            to: *to,
                            claim: ClaimDraft {
                                speaker: claim.speaker,
                                slot: normalized_slot(&claim.slot),
                                text: claim.text.clone(),
                            },
                            delay_min: *delay_min,
                        }
                    }
                });
            }
            if let Some(g) = goal_id {
                owned(
                    db,
                    "SELECT EXISTS(SELECT 1 FROM goal g JOIN entity e ON e.id=g.npc_id WHERE g.id=?1 AND e.campaign_id=?2)",
                    *g,
                    c,
                )?;
            }
            let (at, slot) = match trigger {
                Trigger::At(n) => (Some(*n), None),
                Trigger::When(s) => (None, Some(serde_json::to_string(&normalized_slot(s))?)),
            };
            db.execute("INSERT INTO world_event(id,text,trigger_at,trigger_slot_json,status,actors_json,goal_id,cause_fact_id,scheduled_turn,effects_json,seen_by_json) VALUES(?1,?2,?3,?4,'scheduled',?5,?6,?7,?8,?9,?10)",params![id,text,at,slot,serde_json::to_string(actors)?,goal_id,cause_fact_id,turn,serde_json::to_string(&normalized_effects)?,serde_json::to_string(seen_by)?])?;
            true
        }
        LedgerOp::FireEvent { event_id } => {
            event(db, *event_id, c)?;
            update_one_single(
                db,
                "UPDATE world_event SET status='fired' WHERE id=?1 AND status='scheduled'",
                *event_id,
            )?;
            false
        }
        LedgerOp::CancelEvent {
            event_id,
            cause_fact_id,
        } => {
            event(db, *event_id, c)?;
            fact(db, *cause_fact_id, c)?;
            update_one_single(
                db,
                "UPDATE world_event SET status='cancelled' WHERE id=?1 AND status='scheduled'",
                *event_id,
            )?;
            false
        }
        LedgerOp::AdvanceClock { minutes } => {
            let before: u64 =
                db.query_row("SELECT clock_min FROM campaign WHERE id=?1", [c], |r| {
                    r.get(0)
                })?;
            db.execute(
                "UPDATE campaign SET clock_min=clock_min+?2 WHERE id=?1",
                params![c, minutes],
            )?;
            promote_seen(db, c, Some(before), None)?;
            false
        }
        LedgerOp::RecordRoll {
            seed,
            difficulty,
            stakes,
            factors,
            value,
            outcome,
            ..
        } => {
            db.execute("INSERT INTO roll(id,turn_id,seed,difficulty,stakes_json,factors_json,value,outcome) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![id,turn,*seed as i64,difficulty,serde_json::to_string(stakes)?,serde_json::to_string(factors)?,value,outcome])?;
            true
        }
        LedgerOp::RevertTurn { turn_id } => {
            owned(
                db,
                "SELECT EXISTS(SELECT 1 FROM turn WHERE id=?1 AND campaign_id=?2)",
                *turn_id,
                c,
            )?;
            let target_seq: u64 =
                db.query_row("SELECT seq FROM turn WHERE id=?1", [turn_id], |r| r.get(0))?;
            let current_seq: u64 =
                db.query_row("SELECT seq FROM turn WHERE id=?1", [turn], |r| r.get(0))?;
            if target_seq >= current_seq {
                return Err(StoreError::Invalid("revert target must be earlier".into()));
            }
            false
        }
    };
    Ok(made.then(|| db.last_insert_rowid() as u64))
}

fn promote_seen(
    db: &Connection,
    c: u64,
    after: Option<u64>,
    fact_id: Option<u64>,
) -> Result<(), StoreError> {
    db.execute("WITH candidates AS (SELECT DISTINCT f.id,f.subject_id,f.key FROM fact f JOIN knowledge k ON k.fact_id=f.id JOIN entity holder ON holder.id=k.holder_id JOIN campaign camp ON camp.id=holder.campaign_id WHERE holder.campaign_id=?1 AND holder.kind='pc' AND k.how='witnessed' AND k.arrives_at<=camp.clock_min AND (?2 IS NULL OR k.arrives_at>?2) AND (?3 IS NULL OR f.id=?3) AND f.canon='possible' AND f.superseded_by IS NULL) UPDATE fact SET canon='established' WHERE id IN (SELECT a.id FROM candidates a WHERE (SELECT count(*) FROM candidates b WHERE b.subject_id=a.subject_id AND b.key=a.key)=1 AND NOT EXISTS(SELECT 1 FROM fact live WHERE live.subject_id=a.subject_id AND live.key=a.key AND live.superseded_by IS NULL AND live.canon<>'possible'))",params![c,after,fact_id])?;
    Ok(())
}
pub(super) fn validate_supersessions(db: &Connection, c: u64) -> Result<(), StoreError> {
    let invalid: bool=db.query_row("SELECT EXISTS(SELECT 1 FROM fact old JOIN entity e ON e.id=old.subject_id LEFT JOIN fact new ON new.id=old.superseded_by WHERE e.campaign_id=?1 AND old.superseded_by IS NOT NULL AND (new.id IS NULL OR new.subject_id<>old.subject_id OR new.key<>old.key))",[c],|r|r.get(0))?;
    if invalid {
        return Err(StoreError::Invalid("invalid superseding fact".into()));
    }
    Ok(())
}

fn decode(s: &str) -> Result<(LedgerOp<EntityId>, Option<u64>), StoreError> {
    let value: Value = serde_json::from_str(s)?;
    if let Some(op) = value.get("op") {
        Ok((
            serde_json::from_value(op.clone())?,
            value.get("id").and_then(Value::as_u64),
        ))
    } else {
        Ok((serde_json::from_value(value)?, None))
    }
}
pub(super) fn turn_ops(db: &Connection, turn: u64) -> Result<Vec<LedgerOp<EntityId>>, StoreError> {
    let mut stmt = db.prepare("SELECT op_json FROM ledger WHERE turn_id=?1 ORDER BY seq")?;
    let rows = stmt
        .query_map([turn], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    rows.iter()
        .map(|row| decode(row).map(|(op, _)| op))
        .collect()
}
enum ReplayOp {
    Current(LedgerOp<EntityId>),
    LegacySupersede { old: u64, new: u64 },
}

fn decode_replay(s: &str) -> Result<(ReplayOp, Option<u64>), StoreError> {
    let value: Value = serde_json::from_str(s)?;
    let op = value.get("op").unwrap_or(&value);
    if let Some(legacy) = op.get("supersede_fact")
        && let (Some(old), Some(new)) = (
            legacy.get("fact_id").and_then(Value::as_u64),
            legacy.get("superseded_by").and_then(Value::as_u64),
        )
    {
        return Ok((ReplayOp::LegacySupersede { old, new }, None));
    }
    let (op, id) = decode(s)?;
    Ok((ReplayOp::Current(op), id))
}
pub(super) fn rebuild(db: &Connection, c: u64) -> Result<(), StoreError> {
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM campaign WHERE id=?1)",
        [c],
        |r| r.get(0),
    )?;
    if !exists {
        return Err(StoreError::Invalid("unknown campaign".into()));
    }
    let mut stmt=db.prepare("SELECT l.turn_id,l.op_json FROM ledger l JOIN turn t ON t.id=l.turn_id WHERE t.campaign_id=?1 ORDER BY l.seq")?;
    let rows = stmt
        .query_map([c], |r| Ok((r.get::<_, u64>(0)?, r.get::<_, String>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);
    let mut ops = Vec::new();
    for (turn, s) in rows {
        let (op, id) = decode_replay(&s)?;
        ops.push((turn, op, id));
    }
    let mut reverted = HashSet::new();
    for (turn, op, _) in ops.iter().rev() {
        if !reverted.contains(turn)
            && let ReplayOp::Current(LedgerOp::RevertTurn { turn_id }) = op
        {
            reverted.insert(*turn_id);
        }
    }
    db.execute_batch("PRAGMA defer_foreign_keys=ON")?;
    for sql in [
        "DELETE FROM fact_visible WHERE fact_id IN (SELECT f.id FROM fact f JOIN entity e ON e.id=f.subject_id WHERE e.campaign_id=?1)",
        "DELETE FROM fact_cause WHERE fact_id IN (SELECT f.id FROM fact f JOIN entity e ON e.id=f.subject_id WHERE e.campaign_id=?1)",
        "DELETE FROM knowledge WHERE holder_id IN (SELECT id FROM entity WHERE campaign_id=?1)",
        "DELETE FROM relationship WHERE from_id IN (SELECT id FROM entity WHERE campaign_id=?1)",
        "DELETE FROM voice_card WHERE npc_id IN (SELECT id FROM entity WHERE campaign_id=?1)",
        "DELETE FROM world_event WHERE scheduled_turn IN (SELECT id FROM turn WHERE campaign_id=?1)",
        "DELETE FROM goal WHERE npc_id IN (SELECT id FROM entity WHERE campaign_id=?1)",
        "DELETE FROM obstacle WHERE opened_turn IN (SELECT id FROM turn WHERE campaign_id=?1)",
        "DELETE FROM claim WHERE turn_id IN (SELECT id FROM turn WHERE campaign_id=?1)",
        "DELETE FROM roll WHERE turn_id IN (SELECT id FROM turn WHERE campaign_id=?1)",
        "DELETE FROM correction WHERE turn_id IN (SELECT id FROM turn WHERE campaign_id=?1)",
        "DELETE FROM fact WHERE subject_id IN (SELECT id FROM entity WHERE campaign_id=?1)",
        "DELETE FROM entity WHERE campaign_id=?1",
    ] {
        db.execute(sql, [c])?;
    }
    // The Detail Level starts again from the seed; the ops replay on it.
    let seed: String = db.query_row(
        "SELECT adventure_seed_json FROM campaign WHERE id=?1",
        [c],
        |r| r.get(0),
    )?;
    let seed: AdventureSeed = parse(seed)?;
    db.execute(
        "UPDATE campaign SET clock_min=0,detail_level=?2 WHERE id=?1",
        params![c, enum_text(&seed.detail)?],
    )?;
    for (turn, op, id) in ops {
        if !reverted.contains(&turn) {
            match op {
                ReplayOp::Current(LedgerOp::RevertTurn { .. }) => {}
                ReplayOp::Current(op) => {
                    apply(db, c, turn, &op, id)?;
                }
                ReplayOp::LegacySupersede { old, new } => {
                    fact(db, old, c)?;
                    if db.execute(
                        "UPDATE fact SET superseded_by=?2 WHERE id=?1 AND superseded_by IS NULL",
                        params![old, new],
                    )? != 1
                    {
                        return Err(StoreError::Invalid("fact is already superseded".into()));
                    }
                }
            }
        }
    }
    validate_supersessions(db, c)?;
    Ok(())
}

pub(super) fn read_turn(db: &Connection, id: u64) -> Result<StoredTurn, StoreError> {
    let row=db.query_row("SELECT campaign_id,seq,op_id,kind,intent,proposal_json,clock_before,clock_after,narration FROM turn WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get::<_,String>(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?)))?;
    Ok(StoredTurn {
        id,
        seq: row.1,
        record: TurnRecord {
            campaign_id: row.0,
            op_id: row.2,
            kind: enum_parse(row.3)?,
            intent: row.4,
            proposal_json: row.5,
            clock_before: row.6,
            clock_after: row.7,
        },
        narration: row.8,
    })
}
fn ids(db: &Connection, sql: &str, c: u64) -> Result<Vec<u64>, StoreError> {
    let mut s = db.prepare(sql)?;
    let result = s
        .query_map([c], |r| r.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(result)
}
pub(super) fn snapshot(db: &Connection, c: u64) -> Result<WorldView, StoreError> {
    let (clock, adventure_seed, detail): (u64, String, String) = db
        .query_row(
            "SELECT clock_min,adventure_seed_json,detail_level FROM campaign WHERE id=?1",
            [c],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
        .ok_or_else(|| StoreError::Invalid("unknown campaign".into()))?;
    let adventure_seed = Some(parse(adventure_seed)?);
    let detail: DetailLevel = enum_parse(detail)?;
    let mut entities = Vec::new();
    for id in ids(
        db,
        "SELECT id FROM entity WHERE campaign_id=?1 ORDER BY id",
        c,
    )? {
        let (kind, name, norm_name, age, other_names, age_confirmed) = db.query_row(
            "SELECT kind,name,norm_name,age,other_names_json,age_confirmed FROM entity WHERE id=?1",
            [id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get::<_, String>(4)?,
                    r.get(5)?,
                ))
            },
        )?;
        entities.push(Entity {
            id,
            kind: enum_parse::<EntityKind>(kind)?,
            name,
            norm_name,
            age,
            other_names: parse(other_names)?,
            age_confirmed,
        });
    }
    let mut facts = Vec::new();
    for id in ids(
        db,
        "SELECT f.id FROM fact f JOIN entity e ON e.id=f.subject_id WHERE e.campaign_id=?1 ORDER BY f.id",
        c,
    )? {
        let row=db.query_row("SELECT subject_id,key,value,text,canon,source,created_turn,superseded_by FROM fact WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?,r.get(6)?,r.get(7)?)))?;
        facts.push(Fact {
            id,
            slot: Slot {
                subject: row.0,
                key: row.1,
                value: row.2,
            },
            text: row.3,
            canon: enum_parse::<Canon>(row.4)?,
            source: enum_parse::<FactSource>(row.5)?,
            created_turn: row.6,
            superseded_by: row.7,
            seen_by: ids(
                db,
                "SELECT holder_id FROM fact_visible WHERE fact_id=?1 ORDER BY holder_id",
                id,
            )?,
        });
    }
    let live_facts = facts
        .iter()
        .filter(|f| f.superseded_by.is_none())
        .cloned()
        .collect();
    let mut claims = Vec::new();
    for id in ids(
        db,
        "SELECT a.id FROM claim a JOIN turn t ON t.id=a.turn_id WHERE t.campaign_id=?1 ORDER BY a.id",
        c,
    )? {
        let (speaker, subject, key, value, text, turn_id) = db.query_row(
            "SELECT speaker_id,subject_id,key,value,text,turn_id FROM claim WHERE id=?1",
            [id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            },
        )?;
        claims.push(Claim {
            id,
            claim: ClaimDraft {
                speaker,
                slot: Slot {
                    subject,
                    key,
                    value,
                },
                text,
            },
            turn_id,
        });
    }
    let mut obstacles_open = Vec::new();
    for id in ids(
        db,
        "SELECT o.id FROM obstacle o JOIN turn t ON t.id=o.opened_turn WHERE t.campaign_id=?1 AND o.cleared_turn IS NULL ORDER BY o.id",
        c,
    )? {
        let (fact_id, opened_turn) = db.query_row(
            "SELECT fact_id,opened_turn FROM obstacle WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        obstacles_open.push(Obstacle {
            id,
            fact_id,
            opened_turn,
        });
    }
    // `public` lives in the event's ledger op only: the `world_event`
    // projection of schema 3 has no column for it, and an older op has no
    // field (it reads as not public). Only the ops of the Turns that
    // scheduled a live event are parsed.
    let public: HashSet<u64> = ids(
        db,
        "SELECT CAST(json_extract(l.op_json,'$.id') AS INTEGER) FROM ledger l WHERE l.turn_id IN (SELECT w.scheduled_turn FROM world_event w JOIN turn t ON t.id=w.scheduled_turn WHERE t.campaign_id=?1 AND w.status='scheduled') AND json_extract(l.op_json,'$.op.schedule_event.public')=1",
        c,
    )?
    .into_iter()
    .collect();
    let mut events_scheduled = Vec::new();
    for id in ids(
        db,
        "SELECT w.id FROM world_event w JOIN turn t ON t.id=w.scheduled_turn WHERE t.campaign_id=?1 AND w.status='scheduled' ORDER BY w.id",
        c,
    )? {
        let row=db.query_row("SELECT text,trigger_at,trigger_slot_json,actors_json,goal_id,cause_fact_id,scheduled_turn,effects_json,seen_by_json FROM world_event WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get::<_,Option<u64>>(1)?,r.get::<_,Option<String>>(2)?,r.get::<_,String>(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get::<_,String>(7)?,r.get::<_,String>(8)?)))?;
        let trigger = match (row.1, row.2) {
            (Some(n), None) => Trigger::At(n),
            (None, Some(s)) => Trigger::When(parse(s)?),
            _ => return Err(StoreError::Invalid("invalid event trigger".into())),
        };
        events_scheduled.push(WorldEvent {
            id,
            text: row.0,
            trigger,
            actors: parse(row.3)?,
            goal_id: row.4,
            cause_fact_id: row.5,
            scheduled_turn: row.6,
            effects: parse(row.7)?,
            seen_by: parse(row.8)?,
            public: public.contains(&id),
        });
    }
    let mut goals_active = Vec::new();
    for id in ids(
        db,
        "SELECT g.id FROM goal g JOIN entity e ON e.id=g.npc_id WHERE e.campaign_id=?1 AND g.status='active' ORDER BY g.id",
        c,
    )? {
        let (npc_id, text, against_id) = db.query_row(
            "SELECT npc_id,text,against_id FROM goal WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        goals_active.push(Goal {
            id,
            npc_id,
            text,
            against_id,
        });
    }
    let mut stmt=db.prepare("SELECT r.from_id,r.to_id,r.attitude FROM relationship r JOIN entity e ON e.id=r.from_id WHERE e.campaign_id=?1 ORDER BY r.from_id,r.to_id")?;
    let attitudes = stmt
        .query_map([c], |r| {
            Ok(Attitude {
                from: r.get(0)?,
                to: r.get(1)?,
                attitude: r.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut stmt=db.prepare("SELECT k.holder_id,k.fact_id,k.claim_id,k.how,k.learned_at,k.arrives_at,k.turn_id FROM knowledge k JOIN entity e ON e.id=k.holder_id WHERE e.campaign_id=?1 ORDER BY k.rowid")?;
    let rows = stmt
        .query_map([c], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get::<_, String>(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let knowledge = rows
        .into_iter()
        .map(
            |(holder, fact_id, claim_id, how, learned_at, arrives_at, turn)| {
                Ok(Knowledge {
                    holder,
                    fact_id,
                    claim_id,
                    how: enum_parse::<Knowing>(how)?,
                    learned_at,
                    arrives_at,
                    turn,
                })
            },
        )
        .collect::<Result<Vec<_>, StoreError>>()?;
    let mut stmt = db.prepare("SELECT t.id,t.seq,t.op_id,t.kind,t.intent,t.proposal_json,t.clock_before,t.clock_after,t.narration,l.op_json FROM turn t LEFT JOIN ledger l ON l.turn_id=t.id AND (json_type(l.op_json,'$.op.revert_turn') IS NOT NULL OR json_type(l.op_json,'$.revert_turn') IS NOT NULL) WHERE t.campaign_id=?1 ORDER BY t.seq DESC,l.seq DESC")?;
    let rows = stmt
        .query_map([c], |r| {
            Ok((
                r.get::<_, u64>(0)?,
                r.get::<_, u64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, u64>(6)?,
                r.get::<_, u64>(7)?,
                r.get::<_, Option<String>>(8)?,
                r.get::<_, Option<String>>(9)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let turn_seq = rows.first().map_or(0, |r| r.1);
    let mut reverted = HashSet::new();
    let mut recent_turns = Vec::new();
    // The Narration of every standing story Turn, for `story_names`.
    let mut narrations: Vec<String> = Vec::new();
    let mut visited = HashSet::new();
    for (id, seq, op_id, kind, intent, proposal_json, clock_before, clock_after, narration, op) in
        rows
    {
        if reverted.contains(&id) {
            continue;
        }
        if let Some(op) = op
            && let (LedgerOp::RevertTurn { turn_id }, _) = decode(&op)?
        {
            reverted.insert(turn_id);
        }
        if !visited.insert(id) {
            continue;
        }
        let kind: TurnKind = enum_parse(kind)?;
        // An Opening Turn counts: the first play Turn's prompts must read
        // what the opening Narration already described.
        let story = matches!(
            kind,
            TurnKind::Play | TurnKind::Correction | TurnKind::Opening
        );
        if story && let Some(text) = &narration {
            narrations.push(text.clone());
        }
        if story && recent_turns.len() < 6 {
            recent_turns.push(StoredTurn {
                id,
                seq,
                record: TurnRecord {
                    campaign_id: c,
                    op_id,
                    kind,
                    intent,
                    proposal_json,
                    clock_before,
                    clock_after,
                },
                narration,
            });
        }
    }
    recent_turns.reverse();
    let mut stmt = db.prepare("SELECT v.npc_id,v.habits,v.vocabulary,v.samples_json FROM voice_card v JOIN entity e ON e.id=v.npc_id WHERE e.campaign_id=?1 ORDER BY v.npc_id")?;
    let rows = stmt
        .query_map([c], |r| {
            Ok((
                r.get::<_, u64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let voice_cards = rows
        .into_iter()
        .map(|(npc, habits, vocabulary, samples)| {
            Ok(VoiceCard {
                npc,
                habits,
                vocabulary,
                samples: parse(samples)?,
            })
        })
        .collect::<Result<Vec<_>, StoreError>>()?;
    let mut view = WorldView {
        clock_min: clock,
        adventure_seed,
        detail,
        entities,
        live_facts,
        facts,
        claims,
        obstacles_open,
        events_scheduled,
        goals_active,
        attitudes,
        knowledge,
        recent_turns,
        turn_seq,
        voice_cards,
        story_names: HashSet::new(),
    };
    view.story_names = crate::world::story_names(&view, narrations.iter().map(String::as_str));
    Ok(view)
}

pub(super) fn historical(db: &Connection, c: u64, seq: u64) -> Result<WorldView, StoreError> {
    let mut tmp = Connection::open_in_memory()?;
    super::migrations::apply(&tmp)?;
    let tx = tmp.transaction()?;
    let mut stmt =
        db.prepare("SELECT id,adventure_seed_json,clock_min,seed FROM campaign WHERE id=?1")?;
    let campaign = stmt
        .query_row([c], |r| {
            Ok((
                r.get::<_, u64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, u64>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })
        .optional()?
        .ok_or_else(|| StoreError::Invalid("unknown campaign".into()))?;
    tx.execute(
        "INSERT INTO campaign(id,adventure_seed_json,clock_min,seed) VALUES(?1,?2,?3,?4)",
        params![campaign.0, campaign.1, campaign.2, campaign.3],
    )?;
    let mut stmt=db.prepare("SELECT id,campaign_id,seq,op_id,kind,intent,proposal_json,clock_before,clock_after,narration FROM turn WHERE campaign_id=?1 AND seq<?2 ORDER BY seq")?;
    let rows = stmt
        .query_map(params![c, seq], |r| {
            (0..10)
                .map(|i| r.get::<_, rusqlite::types::Value>(i))
                .collect::<Result<Vec<_>, _>>()
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for row in rows {
        tx.execute("INSERT INTO turn(id,campaign_id,seq,op_id,kind,intent,proposal_json,clock_before,clock_after,narration) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",rusqlite::params_from_iter(row))?;
    }
    let mut stmt=db.prepare("SELECT l.seq,l.turn_id,l.op_json FROM ledger l JOIN turn t ON t.id=l.turn_id WHERE t.campaign_id=?1 AND t.seq<?2 ORDER BY l.seq")?;
    let rows = stmt
        .query_map(params![c, seq], |r| {
            Ok((
                r.get::<_, u64>(0)?,
                r.get::<_, u64>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (n, t, op) in rows {
        tx.execute(
            "INSERT INTO ledger(seq,turn_id,op_json) VALUES(?1,?2,?3)",
            params![n, t, op],
        )?;
    }
    rebuild(&tx, c)?;
    tx.commit()?;
    snapshot(&tmp, c)
}
