//! The Turn engine (design §6.1 and §6.5, data-shape §2).
//!
//! `turn` is the only module that follows time order. A play Turn runs:
//! duplicate `op_id` check, Intent check, proposal prompt, `Gm::propose`,
//! parse, engine-owned wait, the core's repairs (`checks::settle`, and
//! `checks::default_check` on the retry or for a contested `check` verdict),
//! shape and Commit checks (one shared retry
//! with the errors in `recent`), Roll, `to_ops`, World Clock, perception on arrival, one Commit,
//! then the streamed Narration and `set_narration`. Validation, `to_ops` and
//! the World Clock all read the snapshot taken before the Turn, so a fact a
//! World Event writes in this Turn is never a valid cause.
//!
//! Every step writes one `debug_log` row with its milliseconds. The debug log
//! sits outside the Commit transaction, so a failed Turn stays visible.

use std::{collections::HashSet, fmt, time::Instant};

use serde_json::json;

use crate::{
    checks::{self, CheckKind, Rejection, Validated},
    context::{
        adventure_block, fact_text, guard_events, guard_plans, narration_prompt, one_line,
        opening_prompt, perceived_by_pc, proposal_context, seen_present,
    },
    domain::{
        ActionKind, AdventureSeed, ClockMin, DecisionProposal, EntityKind, EntityRef, FactSource,
        Handle, Knowing, LedgerOp, Reaction, Verdict,
    },
    model::{Gm, GmError, Prompt},
    pack::ContentPack,
    proposal_schema_for,
    rules::{RollResult, Ruleset, turn_seed},
    speech::TagFilter,
    store::{
        CampaignId, CommitError, DebugEntry, OpId, Store, StoreError, StoredTurn, TurnId, TurnKind,
        TurnRecord, WorldView,
    },
    world::{self, pc_knowledge},
};

/// The longest Intent, in characters.
pub const MAX_INTENT_CHARS: usize = 2_000;

/// The `op_id` of the Opening Turn: a Campaign has one Begin at most.
pub const BEGIN_OP_ID: &str = "__begin__";

/// Milliseconds per pipeline step, and the prompt tokens the adapter
/// reported for the last proposal call (`None` when it reports none).
/// `propose` and `validate` add up both attempts when the retry runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Timings {
    pub propose_ms: u64,
    pub validate_ms: u64,
    pub roll_ms: u64,
    pub world_ms: u64,
    pub commit_ms: u64,
    pub narrate_ms: u64,
    pub total_ms: u64,
    pub proposal_prompt_tokens: Option<u64>,
    pub narration_prompt_tokens: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnOutcome {
    pub turn_id: TurnId,
    pub roll: Option<RollResult>,
    pub narration: String,
    pub timings: Timings,
}

#[derive(Debug)]
pub enum TurnError {
    EmptyIntent,
    IntentTooLong,
    Model(GmError),
    /// Both attempts failed; these are the errors of the second one.
    Rejected(Vec<Rejection>),
    Store(StoreError),
    /// Only the last play Turn can be corrected.
    NotCorrectable(String),
    /// Begin comes before the first play Turn only.
    PlayStarted,
    /// The Turn is committed, but its Narration failed. The Turn row has no
    /// Narration: `renarrate`, or a retry under the same `op_id`, narrates it
    /// again.
    NarrationFailed {
        turn_id: TurnId,
        error: GmError,
    },
}

impl fmt::Display for TurnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyIntent => f.write_str("the intent is empty"),
            Self::IntentTooLong => {
                write!(f, "the intent is longer than {MAX_INTENT_CHARS} characters")
            }
            Self::Model(e) => e.fmt(f),
            Self::Rejected(errs) => {
                f.write_str("the proposal was rejected twice:")?;
                for e in errs {
                    write!(f, "\n{e}")?;
                }
                Ok(())
            }
            Self::Store(e) => e.fmt(f),
            Self::NotCorrectable(why) => write!(f, "nothing to correct: {why}"),
            Self::PlayStarted => f.write_str("play has already started: Begin comes first"),
            Self::NarrationFailed { turn_id, error } => {
                write!(
                    f,
                    "turn {turn_id} is committed, but its narration failed: {error}"
                )
            }
        }
    }
}

impl std::error::Error for TurnError {}

impl From<StoreError> for TurnError {
    fn from(e: StoreError) -> Self {
        Self::Store(e)
    }
}

impl From<GmError> for TurnError {
    fn from(e: GmError) -> Self {
        Self::Model(e)
    }
}

/// Creates a campaign and, in its Setup Turn, lets the PC witness who openly
/// stands at the starting place (perception on arrival).
pub fn create_campaign(
    store: &mut Store,
    adventure_seed: &AdventureSeed,
    rng_seed: u64,
) -> Result<CampaignId, StoreError> {
    store.create_campaign_with(adventure_seed, rng_seed, |view| {
        let pc = view.entities.iter().find(|e| e.kind == EntityKind::Pc);
        match pc.and_then(|pc| view.live_fact(pc.id, "location")) {
            Some(location) => world::arrival(view, &location.slot.value, view.clock_min),
            None => Vec::new(),
        }
    })
}

pub struct Engine {
    store: Store,
    gm: Box<dyn Gm + Send>,
    rules: Box<dyn Ruleset + Send>,
    packs: Vec<ContentPack>,
    campaign: CampaignId,
}

/// What one committed Turn hands to the narrator, before the perception
/// filter.
struct Committed<'a> {
    ops: &'a [LedgerOp],
    /// Texts of the World Events the PC witnessed.
    events: &'a [String],
    reactions: &'a [Reaction],
    /// The proposal's action and verdict: the narrator tells an impossible
    /// attempt, or a move the Commit did not make, as one that does not
    /// happen. Never the blockers (GM-only text).
    turn: Option<(ActionKind, Verdict)>,
}

/// The result of the steps before the Commit.
struct Planned {
    ops: Vec<LedgerOp>,
    proposal_json: String,
    roll: Option<RollResult>,
    reactions: Vec<Reaction>,
    /// Texts of the World Events the PC witnessed this Turn.
    events: Vec<String>,
    minutes: u32,
    action: ActionKind,
    verdict: Verdict,
}

pub const OOC_RULES: &str = "\
You answer the player's out-of-character question about a single-player tabletop role-playing game.
Answer only from the adventure and what the player character (PC) knows, given below. If the answer is not there, say the PC does not know.
Never invent facts about the world, and never continue the story.
Answer briefly, in plain prose.
";

impl Engine {
    pub fn new(
        store: Store,
        gm: Box<dyn Gm + Send>,
        rules: Box<dyn Ruleset + Send>,
        packs: Vec<ContentPack>,
        campaign: CampaignId,
    ) -> Self {
        Self {
            store,
            gm,
            rules,
            packs,
            campaign,
        }
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Plays one Turn. A second call with the same `op_id` returns the stored
    /// Turn and makes no model call, unless its Narration is missing: then it
    /// narrates again.
    pub fn play_turn(
        &mut self,
        op_id: OpId,
        intent: &str,
        on_chunk: &mut dyn FnMut(&str),
    ) -> Result<TurnOutcome, TurnError> {
        let start = Instant::now();
        if let Some(stored) = self.store.turn_by_campaign_op(self.campaign, &op_id)? {
            return self.stored_outcome(stored, on_chunk);
        }
        check_intent(intent)?;
        let view = self.store.snapshot(self.campaign)?;
        let seed = turn_seed(self.store.campaign_seed(self.campaign)?, view.turn_seq + 1);
        let mut timings = Timings::default();
        let planned = self.plan(&op_id, &view, intent, seed, &mut timings)?;
        let record = TurnRecord {
            campaign_id: self.campaign,
            op_id,
            kind: TurnKind::Play,
            intent: intent.to_owned(),
            proposal_json: planned.proposal_json.clone(),
            clock_before: view.clock_min,
            clock_after: view.clock_min + ClockMin::from(planned.minutes),
        };
        self.finish(record, planned, Vec::new(), timings, start, on_chunk)
    }

    /// Corrects the last play Turn: re-plans it from the state before it,
    /// with the clarified Intent and the corrected Turn's stored Roll seed,
    /// and commits a Correction Turn whose ops revert the old one first.
    /// History stays: the old Turn row and its ops remain in the ledger.
    pub fn correct_last(
        &mut self,
        op_id: OpId,
        clarified_intent: &str,
        why: &str,
        on_chunk: &mut dyn FnMut(&str),
    ) -> Result<TurnOutcome, TurnError> {
        let start = Instant::now();
        if let Some(stored) = self.store.turn_by_campaign_op(self.campaign, &op_id)? {
            return self.stored_outcome(stored, on_chunk);
        }
        check_intent(clarified_intent)?;
        let now = self.store.snapshot(self.campaign)?;
        let last = now
            .recent_turns
            .last()
            .filter(|t| t.seq == now.turn_seq)
            .ok_or_else(|| TurnError::NotCorrectable("the last Turn is not a play Turn".into()))?;
        if last.record.kind != TurnKind::Play {
            return Err(TurnError::NotCorrectable(
                "a Correction reaches back one play Turn only".into(),
            ));
        }
        let view = self.store.snapshot_before(self.campaign, last.seq)?;
        let seed = stored_roll(&last.record.proposal_json).map_or_else(
            || {
                self.store
                    .campaign_seed(self.campaign)
                    .map(|campaign| turn_seed(campaign, last.seq))
            },
            |roll| Ok(roll.seed),
        )?;
        let mut timings = Timings::default();
        let mut planned = self.plan(&op_id, &view, clarified_intent, seed, &mut timings)?;
        for op in &mut planned.ops {
            as_correction(op);
        }
        let head = vec![
            LedgerOp::RevertTurn { turn_id: last.id },
            LedgerOp::RecordCorrection {
                target_turn: last.id,
                what_wrong: format!("The intent was read as: {}", last.record.intent),
                why: why.to_owned(),
            },
        ];
        let record = TurnRecord {
            campaign_id: self.campaign,
            op_id,
            kind: TurnKind::Correction,
            intent: clarified_intent.to_owned(),
            proposal_json: planned.proposal_json.clone(),
            clock_before: view.clock_min,
            clock_after: view.clock_min + ClockMin::from(planned.minutes),
        };
        self.finish(record, planned, head, timings, start, on_chunk)
    }

    /// Begin: commits the Opening Turn (zero ops, empty Intent, no World
    /// Clock move, no World Event) and narrates the start state. It decides
    /// nothing for the PC. A second call returns the stored Turn and makes no
    /// model call, unless its Narration is missing: then it narrates again.
    /// Refused once a play Turn exists.
    pub fn begin(&mut self, on_chunk: &mut dyn FnMut(&str)) -> Result<TurnOutcome, TurnError> {
        let start = Instant::now();
        let op_id: OpId = BEGIN_OP_ID.to_owned();
        if let Some(stored) = self.store.turn_by_campaign_op(self.campaign, &op_id)? {
            return self.stored_outcome(stored, on_chunk);
        }
        if self
            .store
            .turns(self.campaign)?
            .iter()
            .any(|t| matches!(t.record.kind, TurnKind::Play | TurnKind::Correction))
        {
            return Err(TurnError::PlayStarted);
        }
        let view = self.store.snapshot(self.campaign)?;
        let record = TurnRecord {
            campaign_id: self.campaign,
            op_id: op_id.clone(),
            kind: TurnKind::Opening,
            intent: String::new(),
            proposal_json: "{}".into(),
            clock_before: view.clock_min,
            clock_after: view.clock_min,
        };
        let mut timings = Timings::default();
        let Some(turn_id) = self.commit_logged(record, Vec::new(), &mut timings)? else {
            return self.duplicate_outcome(&op_id, on_chunk);
        };
        let view_after = self.store.snapshot(self.campaign)?;
        let step = Instant::now();
        let narration = self
            .narrate_opening(&op_id, &view_after, &view_after, on_chunk)
            .map_err(|e| narration_failed(turn_id, e))?;
        timings.narrate_ms = ms(step);
        timings.narration_prompt_tokens = self.gm.last_prompt_tokens();
        self.store.set_narration(turn_id, &narration)?;
        timings.total_ms = ms(start);
        Ok(TurnOutcome {
            turn_id,
            roll: None,
            narration,
            timings,
        })
    }

    /// Answers an out-of-character question with one plain model call. The
    /// model sees the rules, the Adventure Seed's premise, PC and boundaries,
    /// and PC Knowledge; never a decided or proposed seed fact, a hidden
    /// fact, a goal or an event. Nothing is committed; the call writes one
    /// debug row under `op_id`.
    pub fn ooc(&mut self, op_id: OpId, question: &str) -> Result<String, TurnError> {
        check_intent(question)?;
        let view = self.store.snapshot(self.campaign)?;
        let prompt = ooc_prompt(&view, question);
        let start = Instant::now();
        let answer = self.gm.narrate(&prompt, &mut |_| {});
        self.log(&op_id, "ooc", &prompt, answer_text(&answer), start)?;
        Ok(answer?)
    }

    /// Narrates a committed Turn again, from the state right after it, and
    /// stores the new Narration. This recovers a Turn whose Narration was
    /// interrupted.
    pub fn renarrate(
        &mut self,
        turn: TurnId,
        on_chunk: &mut dyn FnMut(&str),
    ) -> Result<String, TurnError> {
        let stored = self
            .store
            .turn(turn)?
            .filter(|t| t.record.campaign_id == self.campaign)
            .ok_or_else(|| StoreError::Invalid("unknown turn".into()))?;
        let view_after = self.store.snapshot_before(self.campaign, stored.seq + 1)?;
        // The screens read the story against the state now, so the stream
        // filter does too.
        let now = self.store.snapshot(self.campaign)?;
        if stored.record.kind == TurnKind::Opening {
            let op_id = &stored.record.op_id;
            let narration = self.narrate_opening(op_id, &view_after, &now, on_chunk)?;
            self.store.set_narration(turn, &narration)?;
            return Ok(narration);
        }
        let ops: Vec<LedgerOp> = self
            .store
            .turn_ops(turn)?
            .into_iter()
            .map(|op| {
                op.try_map(&mut |id| Ok::<_, std::convert::Infallible>(EntityRef::Id(id)))
                    .unwrap_or_else(|never| match never {})
            })
            .collect();
        let reactions = stored_reactions(&stored.record.proposal_json);
        let events = stored_events(&stored.record.proposal_json);
        let committed = Committed {
            ops: &ops,
            events: &events,
            reactions: &reactions,
            turn: stored_action(&stored.record.proposal_json),
        };
        let narration = self.narrate(
            &stored.record.op_id,
            &view_after,
            &now,
            &committed,
            &stored.record.intent,
            on_chunk,
        )?;
        self.store.set_narration(turn, &narration)?;
        Ok(narration)
    }

    /// The outcome of a Turn already stored under this `op_id`.
    fn stored_outcome(
        &mut self,
        stored: StoredTurn,
        on_chunk: &mut dyn FnMut(&str),
    ) -> Result<TurnOutcome, TurnError> {
        let narration = match stored.narration {
            Some(narration) => narration,
            None => self
                .renarrate(stored.id, on_chunk)
                .map_err(|e| narration_failed(stored.id, e))?,
        };
        Ok(TurnOutcome {
            turn_id: stored.id,
            roll: stored_roll(&stored.record.proposal_json),
            narration,
            timings: Timings::default(),
        })
    }

    /// Commits one Turn and writes its `commit` debug row. `None` when a Turn
    /// already holds this `op_id`.
    fn commit_logged(
        &mut self,
        record: TurnRecord,
        ops: Vec<LedgerOp>,
        timings: &mut Timings,
    ) -> Result<Option<TurnId>, TurnError> {
        let op_id = record.op_id.clone();
        let step = Instant::now();
        let committed = self.store.commit(record, ops);
        timings.commit_ms = ms(step);
        let raw = match &committed {
            Ok(id) => format!("turn {id}"),
            Err(e) => e.to_string(),
        };
        self.log(&op_id, "commit", &Prompt::default(), &raw, step)?;
        match committed {
            Ok(id) => Ok(Some(id)),
            Err(CommitError::DuplicateOp(_)) => Ok(None),
            Err(CommitError::Store(e)) => Err(e.into()),
        }
    }

    /// The outcome of the Turn a Commit found already stored under `op_id`.
    fn duplicate_outcome(
        &mut self,
        op_id: &OpId,
        on_chunk: &mut dyn FnMut(&str),
    ) -> Result<TurnOutcome, TurnError> {
        let stored = self
            .store
            .turn_by_campaign_op(self.campaign, op_id)?
            .ok_or_else(|| StoreError::Invalid("duplicate op without a turn".into()))?;
        self.stored_outcome(stored, on_chunk)
    }

    /// Commits the planned ops (after `head`), then narrates.
    fn finish(
        &mut self,
        record: TurnRecord,
        planned: Planned,
        head: Vec<LedgerOp>,
        mut timings: Timings,
        start: Instant,
        on_chunk: &mut dyn FnMut(&str),
    ) -> Result<TurnOutcome, TurnError> {
        let op_id = record.op_id.clone();
        let intent = record.intent.clone();
        let mut ops = head;
        ops.extend(planned.ops);
        let Some(turn_id) = self.commit_logged(record, ops.clone(), &mut timings)? else {
            return self.duplicate_outcome(&op_id, on_chunk);
        };

        let view_after = self.store.snapshot(self.campaign)?;
        let step = Instant::now();
        let committed = Committed {
            ops: &ops,
            events: &planned.events,
            reactions: &planned.reactions,
            turn: Some((planned.action, planned.verdict)),
        };
        let narration = self
            .narrate(
                &op_id,
                &view_after,
                &view_after,
                &committed,
                &intent,
                on_chunk,
            )
            .map_err(|e| narration_failed(turn_id, e))?;
        timings.narrate_ms = ms(step);
        timings.narration_prompt_tokens = self.gm.last_prompt_tokens();
        self.store.set_narration(turn_id, &narration)?;
        timings.total_ms = ms(start);
        Ok(TurnOutcome {
            turn_id,
            roll: planned.roll,
            narration,
            timings,
        })
    }

    /// The narration call. Only what the PC perceives reaches it, and only
    /// the reactions of NPCs the PC knows are here: the others are offscreen
    /// intents. A plan or a World Event text that names a secret is dropped
    /// (`guard_plans`, `guard_events`). `now` is the current state: the
    /// stream filter reads it (`narrate_through`).
    fn narrate(
        &mut self,
        op_id: &str,
        view_after: &WorldView,
        now: &WorldView,
        committed: &Committed<'_>,
        intent: &str,
        on_chunk: &mut dyn FnMut(&str),
    ) -> Result<String, TurnError> {
        let perceived = perceived_by_pc(committed.ops, view_after);
        let seen = seen_present(view_after);
        let reactions: Vec<Reaction> = committed
            .reactions
            .iter()
            .filter(|r| {
                match r.npc {
                Handle::Entity(id) => seen.contains(&id),
                Handle::NewNpc(n) => perceived.iter().any(|op| {
                    matches!(op, LedgerOp::AddEntity { local: Some(l), .. } if u64::from(*l) == n)
                }),
                _ => false,
            }
            })
            .cloned()
            .collect();
        let reactions = guard_plans(view_after, &reactions);
        let events = guard_events(view_after, committed.events);
        let prompt = narration_prompt(
            view_after,
            &self.packs,
            &perceived,
            &events,
            &reactions,
            intent,
            committed.turn,
        );
        let hidden = self.unnameable_names(now)?;
        let start = Instant::now();
        let narration = narrate_through(self.gm.as_ref(), &prompt, hidden, on_chunk);
        self.log(op_id, "narrate", &prompt, answer_text(&narration), start)?;
        Ok(narration?)
    }

    /// The opening narration call: `opening_prompt` on the state after the
    /// Opening Turn, which equals the state before it. `now` is the current
    /// state, for the stream filter.
    fn narrate_opening(
        &mut self,
        op_id: &str,
        view_after: &WorldView,
        now: &WorldView,
        on_chunk: &mut dyn FnMut(&str),
    ) -> Result<String, TurnError> {
        let prompt = opening_prompt(view_after, &self.packs);
        let hidden = self.unnameable_names(now)?;
        let start = Instant::now();
        let narration = narrate_through(self.gm.as_ref(), &prompt, hidden, on_chunk);
        self.log(op_id, "narrate", &prompt, answer_text(&narration), start)?;
        Ok(narration?)
    }

    /// The names a speech tag must not print, from the state `now` and the
    /// Turn rows, as the screens will read them after the Commit (data shape
    /// 3.1). Store reads only: no model call.
    fn unnameable_names(&self, now: &WorldView) -> Result<HashSet<String>, TurnError> {
        let turns = self.store.turns(self.campaign)?;
        let reverted = self
            .store
            .reverted_turns(self.campaign)?
            .into_iter()
            .collect();
        Ok(world::unnameable_names(now, &turns, &reverted))
    }

    /// Every step before the Commit, against `view` (the state before the
    /// Turn). One retry in total, shared by parse, shape and Commit checks.
    fn plan(
        &mut self,
        op_id: &str,
        view: &WorldView,
        intent: &str,
        seed: u64,
        timings: &mut Timings,
    ) -> Result<Planned, TurnError> {
        let ctx = proposal_context(view, &self.packs, intent);
        let schema = proposal_schema_for(&ctx.handles);
        let mut prompt = ctx.prompt;
        for attempt in ["propose", "propose_retry"] {
            let step = Instant::now();
            let raw = self.gm.propose(&prompt, &schema);
            timings.propose_ms += ms(step);
            timings.proposal_prompt_tokens = self.gm.last_prompt_tokens();
            let logged = match self.gm.last_done_reason().filter(|r| r != "stop") {
                Some(reason) => format!("{}\n[generation stopped: {reason}]", answer_text(&raw)),
                None => answer_text(&raw).to_owned(),
            };
            self.log(op_id, attempt, &prompt, &logged, step)?;
            let raw = raw?;

            let step = Instant::now();
            let retry = attempt == "propose_retry";
            let parsed = match parse(&raw) {
                Ok(mut p) => {
                    // The items the Intent names: the only ones `settles`
                    // may close (review M1).
                    p.offered.clone_from(&ctx.handles.open_items);
                    let (mut p, mut notes) = checks::settle(engine_time(p, view, intent), view);
                    // A contested act with no Check: one repair note on the
                    // first attempt, a default Check on the retry (ADR 0007).
                    // A contested `check` verdict gets it at once.
                    if retry || checks::checks_now(&p) {
                        notes.extend(checks::default_check(&mut p, view));
                    }
                    notes.extend(checks::oppose(&mut p, view));
                    if !notes.is_empty() {
                        self.log(op_id, "settle", &Prompt::default(), &notes.join("\n"), step)?;
                    }
                    Ok(p)
                }
                Err(errs) => Err(errs),
            };
            let checked = parsed.as_ref().map_err(Clone::clone).and_then(|p| {
                let checked = checks::validate(p, view);
                match (retry, checks::contest_note(p)) {
                    (false, Some(note)) => {
                        let mut errs = checked.err().unwrap_or_default();
                        errs.push(note);
                        Err(errs)
                    }
                    _ => checked,
                }
            });
            timings.validate_ms += ms(step);
            let errs = match checked {
                Ok(valid) => {
                    self.log(op_id, "validate", &Prompt::default(), "ok", step)?;
                    return self.resolve(op_id, valid, view, seed, timings);
                }
                Err(errs) => errs,
            };
            let lines: Vec<String> = errs.iter().map(ToString::to_string).collect();
            self.log(
                op_id,
                "validate",
                &Prompt::default(),
                &lines.join("\n"),
                step,
            )?;
            if retry {
                return Err(TurnError::Rejected(errs));
            }
            prompt
                .recent
                .push_str("REJECTED PROPOSAL (fix every error below, then answer again)\n");
            for line in &lines {
                prompt.recent.push_str(&format!("- {line}\n"));
            }
        }
        unreachable!("the loop returns on its second attempt")
    }

    /// Roll, `to_ops`, World Clock, perception on arrival and of World
    /// Events, for a valid proposal.
    fn resolve(
        &mut self,
        op_id: &str,
        valid: Validated<'_>,
        view: &WorldView,
        seed: u64,
        timings: &mut Timings,
    ) -> Result<Planned, TurnError> {
        let p = valid.proposal();
        let step = Instant::now();
        let roll = p.check.as_ref().map(|check| {
            let difficulty = self.rules.difficulty(check, view);
            self.rules.roll(difficulty, seed)
        });
        timings.roll_ms = ms(step);
        let raw = roll.map_or_else(|| "no check".into(), |r| format!("{r:?}"));
        self.log(op_id, "roll", &Prompt::default(), &raw, step)?;

        let step = Instant::now();
        let resolved = checks::to_ops(valid, roll.as_ref(), view);
        let mut ops = resolved.ops;
        // A due event that writes a slot the proposal writes waits a Turn.
        let written = world::written_slots(&ops);
        // `to_ops` already advances the clock by the proposal's minutes.
        ops.extend(
            world::advance(view, p.minutes, &written)
                .into_iter()
                .filter(|op| !matches!(op, LedgerOp::AdvanceClock { .. })),
        );
        let clock_after = view.clock_min + ClockMin::from(p.minutes);
        ops = world::with_arrival(view, ops, clock_after);
        let (ops, events) = world::with_witnessed_events(view, ops);
        timings.world_ms = ms(step);
        self.log(op_id, "world", &Prompt::default(), "", step)?;

        let proposal_json = json!({
            "proposal": p,
            "reactions": resolved.reactions,
            "events": events,
            "roll": roll,
        })
        .to_string();
        Ok(Planned {
            ops,
            proposal_json,
            roll,
            reactions: resolved.reactions,
            events,
            minutes: p.minutes,
            action: p.action,
            verdict: p.verdict,
        })
    }

    fn log(
        &self,
        op_id: &str,
        step: &str,
        prompt: &Prompt,
        raw: &str,
        start: Instant,
    ) -> Result<(), StoreError> {
        self.store.log_debug(DebugEntry {
            op_id: op_id.to_owned(),
            step: step.to_owned(),
            prompt: format!(
                "{}{}{}{}",
                prompt.system, prompt.stable, prompt.scene, prompt.recent
            ),
            raw: raw.to_owned(),
            ms: ms(start),
        })
    }
}

/// A fact the proposal or its Check wrote is, in a Correction Turn, a
/// Correction fact. World Event facts keep their source.
fn as_correction(op: &mut LedgerOp) {
    let source = match op {
        LedgerOp::AddFact { source, .. } => source,
        LedgerOp::SupersedeFact { new, .. } => &mut new.source,
        _ => return,
    };
    if matches!(source, FactSource::Proposal | FactSource::Stake) {
        *source = FactSource::Correction;
    }
}

/// A model error after the Commit: the Turn stands, its Narration failed.
fn narration_failed(turn_id: TurnId, e: TurnError) -> TurnError {
    match e {
        TurnError::Model(error) => TurnError::NarrationFailed { turn_id, error },
        other => other,
    }
}

fn ms(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn answer_text(result: &Result<String, GmError>) -> &str {
    match result {
        Ok(text) => text,
        Err(_) => "model error",
    }
}

fn check_intent(intent: &str) -> Result<(), TurnError> {
    if intent.trim().is_empty() {
        Err(TurnError::EmptyIntent)
    } else if intent.chars().count() > MAX_INTENT_CHARS {
        Err(TurnError::IntentTooLong)
    } else {
        Ok(())
    }
}

fn schema_error(reason: String) -> Vec<Rejection> {
    vec![Rejection {
        check: CheckKind::Schema,
        handle: None,
        reason,
    }]
}

/// Parses the model text. The shape check runs with the Commit checks
/// (`checks::validate`), after the core's repairs: on the retry, a missing
/// Check is one the core can build (`checks::default_check`).
fn parse(raw: &str) -> Result<DecisionProposal, Vec<Rejection>> {
    serde_json::from_str(raw.trim()).map_err(|e| schema_error(format!("invalid JSON: {e}")))
}

/// The engine owns the time of a Turn, so the model cannot freeze the World
/// Clock with `minutes: 0`. A wait takes `world::wait_minutes`, but only when
/// the Intent asks for one (`world::asks_to_wait`): the model labels a
/// passive act `wait`, and "I warm my hands at the fire" took 60 minutes
/// (bug pass 3). Such a wait is an act. A contested wait is an act too, and
/// so is a contested move with no destination: the QA arm-wrestle came as
/// `move` with `move_to: null`, and the narrator told a move that did not
/// happen. Any other Turn with no minutes takes the Ruleset default of its
/// action (`rules::default_minutes`). It runs before the Commit checks, so
/// they see the final minutes. Relabelling the action adds no PC act.
fn engine_time(mut p: DecisionProposal, view: &WorldView, intent: &str) -> DecisionProposal {
    let not_a_wait = p.contested || !world::asks_to_wait(intent);
    if p.action == ActionKind::Wait && not_a_wait {
        p.action = ActionKind::Act;
    }
    if p.action == ActionKind::Move && p.contested && p.move_to.is_none() {
        p.action = ActionKind::Act;
    }
    if p.action == ActionKind::Wait {
        p.minutes = world::wait_minutes(view, p.minutes);
    } else if p.minutes == 0 {
        p.minutes = crate::rules::default_minutes(p.action);
    }
    p
}

/// The Roll stored in a Turn's `proposal_json`, if the Turn had a Check.
pub fn stored_roll(proposal_json: &str) -> Option<RollResult> {
    let record: serde_json::Value = serde_json::from_str(proposal_json).ok()?;
    serde_json::from_value(record.get("roll")?.clone()).ok()
}

/// The action and the verdict stored in a Turn's `proposal_json`: the
/// `proposal.action` and `proposal.verdict` of the record. `None` for a
/// Turn with no proposal, such as the Opening Turn.
pub fn stored_action(proposal_json: &str) -> Option<(ActionKind, Verdict)> {
    let record: serde_json::Value = serde_json::from_str(proposal_json).ok()?;
    let proposal = record.get("proposal")?;
    let action = serde_json::from_value(proposal.get("action")?.clone()).ok()?;
    let verdict = serde_json::from_value(proposal.get("verdict")?.clone()).ok()?;
    Some((action, verdict))
}

fn stored_reactions(proposal_json: &str) -> Vec<Reaction> {
    serde_json::from_str::<serde_json::Value>(proposal_json)
        .ok()
        .and_then(|record| serde_json::from_value(record.get("reactions")?.clone()).ok())
        .unwrap_or_default()
}

fn stored_events(proposal_json: &str) -> Vec<String> {
    serde_json::from_str::<serde_json::Value>(proposal_json)
        .ok()
        .and_then(|record| serde_json::from_value(record.get("events")?.clone()).ok())
        .unwrap_or_default()
}

/// The OOC prompt: rules, the narration prompt's stable block, then PC
/// Knowledge, then the question.
fn ooc_prompt(view: &WorldView, question: &str) -> Prompt {
    let stable = adventure_block(view, None);
    let mut scene = String::from("WHAT THE PC KNOWS\n");
    let name = |id: u64| {
        view.entities
            .iter()
            .find(|e| e.id == id)
            .map_or_else(|| "someone".to_owned(), |e| e.name.clone())
    };
    for belief in pc_knowledge(view) {
        match (belief.how, belief.source) {
            (Knowing::Told, Some(speaker)) => scene.push_str(&format!(
                "- {} (told by {})\n",
                fact_text(&belief.key, &belief.text),
                name(speaker)
            )),
            _ => scene.push_str(&format!("- {}\n", fact_text(&belief.key, &belief.text))),
        }
    }
    Prompt {
        system: OOC_RULES.to_owned(),
        stable,
        scene,
        recent: format!("QUESTION\n{}\n", one_line(question)),
    }
}

/// One narrator call whose stream passes through `speech::TagFilter`: a tag
/// that names someone the PC cannot name never reaches `on_chunk`. The
/// answer, which becomes the stored Narration, is returned unchanged; the
/// commit-time rule of `world::pc_sight` handles it.
fn narrate_through(
    gm: &dyn Gm,
    prompt: &Prompt,
    hidden: HashSet<String>,
    on_chunk: &mut dyn FnMut(&str),
) -> Result<String, GmError> {
    let mut filter = TagFilter::new(hidden);
    let answer = gm.narrate(prompt, &mut |chunk| {
        let shown = filter.push(chunk);
        if !shown.is_empty() {
            on_chunk(&shown);
        }
    });
    let rest = filter.finish();
    if !rest.is_empty() {
        on_chunk(&rest);
    }
    answer
}
