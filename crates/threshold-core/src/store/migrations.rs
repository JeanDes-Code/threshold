use super::{SCHEMA_VERSION, StoreError};
use rusqlite::Connection;

/// Creates the schema in an empty database. An older version is refused:
/// there is no migration, and the file stays as it is.
pub(super) fn apply(db: &Connection) -> Result<(), StoreError> {
    let version: u32 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version != 0 && version != SCHEMA_VERSION {
        return Err(StoreError::Version(version));
    }
    db.execute_batch("PRAGMA foreign_keys=ON;")?;
    if version == 0 {
        db.execute_batch(&format!(
            "BEGIN;\n{STORE_SCHEMA}{}PRAGMA user_version={SCHEMA_VERSION};\nCOMMIT;",
            crate::player::SCHEMA
        ))?;
    }
    Ok(())
}

/// The tables of the world: the ledger and its projections. Schema 4 adds
/// `campaign.detail_level` (projection: the seed's `detail`),
/// `campaign.scene_open_turn` (projection: `SetScene`), `entity.age_confirmed`
/// (projection: `AddEntity`, `ConfirmAge`), `turn.faded` (not a
/// projection; `Store::set_faded`) and `turn.told` (not a projection: the
/// level an intimate Turn's stored Narration was told at,
/// `Store::set_told_narration`; added in place after the final review of
/// 2026-11-12, before version 4 shipped).
const STORE_SCHEMA: &str = r#"
CREATE TABLE campaign (id INTEGER PRIMARY KEY, adventure_seed_json TEXT NOT NULL, clock_min INTEGER NOT NULL, seed INTEGER NOT NULL, detail_level TEXT NOT NULL DEFAULT 'excluded' CHECK (detail_level IN ('excluded','implied','explicit')), scene_open_turn INTEGER REFERENCES turn(id));
CREATE TABLE entity (id INTEGER PRIMARY KEY, campaign_id INTEGER NOT NULL REFERENCES campaign(id), kind TEXT NOT NULL, name TEXT NOT NULL, norm_name TEXT NOT NULL, age INTEGER, other_names_json TEXT NOT NULL, age_confirmed INTEGER NOT NULL DEFAULT 0 CHECK (age_confirmed IN (0,1)) CHECK (age_confirmed = 0 OR age IS NOT NULL), CHECK ((kind IN ('pc','npc')) = (age IS NOT NULL)));
CREATE TABLE voice_card (npc_id INTEGER PRIMARY KEY REFERENCES entity(id), habits TEXT NOT NULL, vocabulary TEXT NOT NULL, samples_json TEXT NOT NULL);
CREATE TABLE goal (id INTEGER PRIMARY KEY, npc_id INTEGER NOT NULL REFERENCES entity(id), text TEXT NOT NULL, against_id INTEGER REFERENCES entity(id), status TEXT NOT NULL);
CREATE TABLE relationship (from_id INTEGER NOT NULL REFERENCES entity(id), to_id INTEGER NOT NULL REFERENCES entity(id), attitude INTEGER NOT NULL CHECK(attitude BETWEEN -5 AND 5), PRIMARY KEY(from_id,to_id));
CREATE TABLE turn (id INTEGER PRIMARY KEY, campaign_id INTEGER NOT NULL REFERENCES campaign(id), seq INTEGER NOT NULL, op_id TEXT NOT NULL, kind TEXT NOT NULL, intent TEXT NOT NULL, proposal_json TEXT NOT NULL, clock_before INTEGER NOT NULL, clock_after INTEGER NOT NULL, narration TEXT, faded INTEGER NOT NULL DEFAULT 0, told TEXT CHECK (told IS NULL OR told IN ('implied','explicit')), UNIQUE(campaign_id,op_id), UNIQUE(campaign_id,seq));
CREATE TABLE fact (id INTEGER PRIMARY KEY, subject_id INTEGER NOT NULL REFERENCES entity(id), key TEXT NOT NULL, value TEXT NOT NULL, text TEXT NOT NULL, canon TEXT NOT NULL, source TEXT NOT NULL, created_turn INTEGER NOT NULL REFERENCES turn(id), superseded_by INTEGER REFERENCES fact(id) DEFERRABLE INITIALLY DEFERRED);
CREATE UNIQUE INDEX fact_live_slot ON fact(subject_id,key) WHERE superseded_by IS NULL AND canon <> 'possible';
CREATE TABLE fact_cause (fact_id INTEGER NOT NULL REFERENCES fact(id), cause_fact_id INTEGER NOT NULL REFERENCES fact(id));
CREATE TABLE fact_visible (fact_id INTEGER NOT NULL REFERENCES fact(id), holder_id INTEGER NOT NULL REFERENCES entity(id));
CREATE TABLE claim (id INTEGER PRIMARY KEY, speaker_id INTEGER NOT NULL REFERENCES entity(id), subject_id INTEGER NOT NULL REFERENCES entity(id), key TEXT NOT NULL, value TEXT NOT NULL, text TEXT NOT NULL, turn_id INTEGER NOT NULL REFERENCES turn(id));
CREATE TABLE knowledge (holder_id INTEGER NOT NULL REFERENCES entity(id), fact_id INTEGER REFERENCES fact(id), claim_id INTEGER REFERENCES claim(id), how TEXT NOT NULL, learned_at INTEGER NOT NULL, arrives_at INTEGER NOT NULL, turn_id INTEGER NOT NULL REFERENCES turn(id), CHECK ((fact_id IS NULL) <> (claim_id IS NULL)));
CREATE TABLE obstacle (id INTEGER PRIMARY KEY, fact_id INTEGER NOT NULL REFERENCES fact(id), opened_turn INTEGER NOT NULL REFERENCES turn(id), cleared_turn INTEGER REFERENCES turn(id));
CREATE TABLE world_event (id INTEGER PRIMARY KEY, text TEXT NOT NULL, trigger_at INTEGER, trigger_slot_json TEXT, status TEXT NOT NULL, actors_json TEXT NOT NULL, goal_id INTEGER REFERENCES goal(id), cause_fact_id INTEGER REFERENCES fact(id), scheduled_turn INTEGER NOT NULL REFERENCES turn(id), effects_json TEXT NOT NULL, seen_by_json TEXT NOT NULL);
CREATE TABLE roll (id INTEGER PRIMARY KEY, turn_id INTEGER NOT NULL REFERENCES turn(id), seed INTEGER NOT NULL, difficulty INTEGER NOT NULL, stakes_json TEXT NOT NULL, factors_json TEXT NOT NULL, value INTEGER NOT NULL, outcome TEXT NOT NULL);
CREATE TABLE correction (id INTEGER PRIMARY KEY, turn_id INTEGER NOT NULL REFERENCES turn(id), target_turn_id INTEGER NOT NULL REFERENCES turn(id), what_wrong TEXT NOT NULL, why TEXT NOT NULL);
CREATE TABLE ledger (seq INTEGER PRIMARY KEY, turn_id INTEGER NOT NULL REFERENCES turn(id), op_json TEXT NOT NULL);
CREATE TABLE debug_log (id INTEGER PRIMARY KEY, op_id TEXT NOT NULL, step TEXT NOT NULL, prompt TEXT NOT NULL, raw TEXT NOT NULL, ms INTEGER NOT NULL);
"#;
