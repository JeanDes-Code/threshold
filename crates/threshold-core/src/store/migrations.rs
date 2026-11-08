use rusqlite::{Connection, Result};

pub(super) fn apply(db: &Connection) -> Result<()> {
    db.execute_batch(r#"
PRAGMA foreign_keys=ON;
CREATE TABLE IF NOT EXISTS campaign (id INTEGER PRIMARY KEY, charter_json TEXT NOT NULL, clock_min INTEGER NOT NULL, seed INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS entity (id INTEGER PRIMARY KEY, campaign_id INTEGER NOT NULL REFERENCES campaign(id), kind TEXT NOT NULL, name TEXT NOT NULL, norm_name TEXT NOT NULL, adult INTEGER, CHECK (kind NOT IN ('pc','npc') OR adult IS NOT NULL));
CREATE TABLE IF NOT EXISTS voice_card (npc_id INTEGER PRIMARY KEY REFERENCES entity(id), habits TEXT NOT NULL, vocabulary TEXT NOT NULL, samples_json TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS goal (id INTEGER PRIMARY KEY, npc_id INTEGER NOT NULL REFERENCES entity(id), text TEXT NOT NULL, against_id INTEGER REFERENCES entity(id), status TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS relationship (from_id INTEGER NOT NULL REFERENCES entity(id), to_id INTEGER NOT NULL REFERENCES entity(id), attitude INTEGER NOT NULL CHECK(attitude BETWEEN -5 AND 5), PRIMARY KEY(from_id,to_id));
CREATE TABLE IF NOT EXISTS turn (id INTEGER PRIMARY KEY, campaign_id INTEGER NOT NULL REFERENCES campaign(id), seq INTEGER NOT NULL, op_id TEXT NOT NULL, kind TEXT NOT NULL, intent TEXT NOT NULL, proposal_json TEXT NOT NULL, clock_before INTEGER NOT NULL, clock_after INTEGER NOT NULL, narration TEXT, UNIQUE(campaign_id,op_id), UNIQUE(campaign_id,seq));
CREATE TABLE IF NOT EXISTS fact (id INTEGER PRIMARY KEY, subject_id INTEGER NOT NULL REFERENCES entity(id), key TEXT NOT NULL, value TEXT NOT NULL, text TEXT NOT NULL, canon TEXT NOT NULL, source TEXT NOT NULL, created_turn INTEGER NOT NULL REFERENCES turn(id), superseded_by INTEGER REFERENCES fact(id) DEFERRABLE INITIALLY DEFERRED);
CREATE UNIQUE INDEX IF NOT EXISTS fact_live_slot ON fact(subject_id,key) WHERE superseded_by IS NULL AND canon <> 'possible';
CREATE TABLE IF NOT EXISTS fact_cause (fact_id INTEGER NOT NULL REFERENCES fact(id), cause_fact_id INTEGER NOT NULL REFERENCES fact(id));
CREATE TABLE IF NOT EXISTS fact_visible (fact_id INTEGER NOT NULL REFERENCES fact(id), holder_id INTEGER NOT NULL REFERENCES entity(id));
CREATE TABLE IF NOT EXISTS claim (id INTEGER PRIMARY KEY, speaker_id INTEGER NOT NULL REFERENCES entity(id), subject_id INTEGER NOT NULL REFERENCES entity(id), key TEXT NOT NULL, value TEXT NOT NULL, text TEXT NOT NULL, turn_id INTEGER NOT NULL REFERENCES turn(id));
CREATE TABLE IF NOT EXISTS knowledge (holder_id INTEGER NOT NULL REFERENCES entity(id), fact_id INTEGER REFERENCES fact(id), claim_id INTEGER REFERENCES claim(id), how TEXT NOT NULL, learned_at INTEGER NOT NULL, arrives_at INTEGER NOT NULL, CHECK ((fact_id IS NULL) <> (claim_id IS NULL)));
CREATE TABLE IF NOT EXISTS obstacle (id INTEGER PRIMARY KEY, fact_id INTEGER NOT NULL REFERENCES fact(id), opened_turn INTEGER NOT NULL REFERENCES turn(id), cleared_turn INTEGER REFERENCES turn(id));
CREATE TABLE IF NOT EXISTS world_event (id INTEGER PRIMARY KEY, text TEXT NOT NULL, trigger_at INTEGER, trigger_slot_json TEXT, status TEXT NOT NULL, actors_json TEXT NOT NULL, goal_id INTEGER REFERENCES goal(id), cause_fact_id INTEGER REFERENCES fact(id), scheduled_turn INTEGER NOT NULL REFERENCES turn(id), effects_json TEXT NOT NULL, seen_by_json TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS roll (id INTEGER PRIMARY KEY, turn_id INTEGER NOT NULL REFERENCES turn(id), seed INTEGER NOT NULL, difficulty INTEGER NOT NULL, stakes_json TEXT NOT NULL, factors_json TEXT NOT NULL, value INTEGER NOT NULL, outcome TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS correction (id INTEGER PRIMARY KEY, turn_id INTEGER NOT NULL REFERENCES turn(id), target_turn_id INTEGER NOT NULL REFERENCES turn(id), what_wrong TEXT NOT NULL, why TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS ledger (seq INTEGER PRIMARY KEY, turn_id INTEGER NOT NULL REFERENCES turn(id), op_json TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS debug_log (id INTEGER PRIMARY KEY, op_id TEXT NOT NULL, step TEXT NOT NULL, prompt TEXT NOT NULL, raw TEXT NOT NULL, ms INTEGER NOT NULL);
PRAGMA user_version=1;
"#)
}
