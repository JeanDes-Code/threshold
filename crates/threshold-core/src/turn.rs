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
        QUIET_EVENT_LINE, adventure_block, guard_events, guard_plans, narration_prompt_with,
        one_line, opening_prompt, perceived_by_pc, proposal_context, seen_present,
    },
    domain::{
        ActionKind, AdventureSeed, ClockMin, ConfirmedAges, DecisionProposal, EntityId, EntityKind,
        EntityRef, FactSource, Handle, LedgerOp, Reaction, Verdict,
    },
    mature::{self, RefusalKind},
    model::{Gm, GmError, Prompt},
    pack::ContentPack,
    proposal_schema_for,
    rules::{RollResult, Ruleset, turn_seed},
    speech::TagFilter,
    store::{
        CampaignId, CommitError, DebugEntry, OpId, Store, StoreError, StoredTurn, TurnId, TurnKind,
        TurnRecord, WorldView,
    },
    world::{self, PcSight},
};

/// The longest Intent, in characters.
pub const MAX_INTENT_CHARS: usize = 2_000;

/// The debug step of a row with the server's numbers for one model call
/// (`Engine::log_stats`).
pub const METRICS_STEP: &str = "metrics";

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
    /// A Refusal (glossary): the core stopped the Turn for a Content
    /// Boundary, with no Commit. An age or level refusal comes after one
    /// proposal call, with no retry. `blocked` lists the characters who
    /// block an `AdultsOnly` refusal, for the debug log and the core; a
    /// Player screen never shows it raw.
    Refused {
        kind: RefusalKind,
        blocked: Vec<EntityId>,
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
            Self::Refused { kind, .. } => f.write_str(crate::view::refusal_text(*kind)),
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
/// stands at the starting place (perception on arrival). `ages` are the
/// ages the Player confirmed in the Seed Draft (`build::confirmed_ages`);
/// `None` confirms no age (`Store::create_campaign_with`).
pub fn create_campaign(
    store: &mut Store,
    adventure_seed: &AdventureSeed,
    rng_seed: u64,
    ages: Option<&ConfirmedAges>,
) -> Result<CampaignId, StoreError> {
    store.create_campaign_with(adventure_seed, rng_seed, ages, |view| {
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
    /// Outcome lines the engine adds: a wait cut short (`wait_cut_line`).
    extra: Vec<String>,
    /// The Turn was committed with `intimate`: G4 (`mature::told`) reads it.
    intimate: bool,
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
    /// The minutes a wait Intent stated (`world::intent_minutes`), if any.
    wait_asked: Option<u32>,
    /// The proposal's `intimate`, after the tag of the first attempt stuck.
    intimate: bool,
}

/// The OOC rules. The last line is `context::INTIMATE_RULE`, the narrator's
/// stable neutral line: an OOC answer holds no intimate content either.
pub const OOC_RULES: &str = concat!(
    "\
You answer the player's out-of-character question about a single-player tabletop role-playing game. The player plays the player character (PC): answer the player as \"you\".
Answer only from the adventure and the blocks below. Together they hold everything the PC knows, and every block counts: a request, an offer, a plan or a promise also tells what you know about each person it names. Say that you do not know only when no block holds the answer.
NOW: where you are, the time, and who is here with you. STILL OPEN: the requests, offers, plans and promises not answered or not done yet. ALREADY ANSWERED: each one you accepted, refused, did, failed or gave up, and how. RECENT TURNS: what the player last did and read. WHAT YOU KNOW: the other things you know about people, places and things.
Never invent facts about the world, and never continue the story.
Answer briefly, in plain prose.
",
    crate::context::intimate_rule_text!(),
    "\n"
);

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
        let turns = self.store.turns(self.campaign)?;
        let reverted: HashSet<TurnId> = self
            .store
            .reverted_turns(self.campaign)?
            .into_iter()
            .collect();
        let sight = crate::world::pc_sight(&view, &turns, &reverted);
        let prompt = ooc_prompt(&view, sight.as_ref(), question);
        let start = Instant::now();
        let answer = self.gm.narrate(&prompt, &mut |_| {});
        self.log(&op_id, "ooc", &prompt, answer_text(&answer), start)?;
        self.log_stats(&op_id, "ooc")?;
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
            extra: stored_wait_asked(&stored.record.proposal_json)
                .and_then(|asked| {
                    let minutes = stored.record.clock_after - stored.record.clock_before;
                    wait_cut_line(u32::try_from(minutes).unwrap_or(u32::MAX), asked)
                })
                .into_iter()
                .collect(),
            intimate: mature::stored_intimate(&stored.record.proposal_json),
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
            extra: planned
                .wait_asked
                .and_then(|asked| wait_cut_line(planned.minutes, asked))
                .into_iter()
                .collect(),
            intimate: planned.intimate,
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
    /// stream filter reads it (`narrate_through`). G4: `mature::told`
    /// chooses how the Turn is told; an intimate Turn is told by
    /// implication only, with the core's own line.
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
        let mut events = guard_events(view_after, committed.events);
        // A witnessed event stopped the clock, and every part of it is
        // guarded away: the narrator gets a neutral line, never an empty
        // outcome.
        let event_fact = perceived.iter().any(|op| match op {
            LedgerOp::AddFact { source, .. } => *source == FactSource::WorldEvent,
            LedgerOp::SupersedeFact { new, .. } => new.source == FactSource::WorldEvent,
            _ => false,
        });
        if !committed.events.is_empty() && events.is_empty() && !event_fact {
            events.push(QUIET_EVENT_LINE.to_owned());
        }
        let told = mature::told(view_after, now, committed.intimate);
        let prompt = narration_prompt_with(
            view_after,
            &self.packs,
            &perceived,
            &events,
            &reactions,
            intent,
            committed.turn,
            &committed.extra,
            &told,
        );
        let hidden = self.unnameable_names(now)?;
        let start = Instant::now();
        let narration = narrate_through(self.gm.as_ref(), &prompt, hidden, on_chunk);
        self.log(op_id, "narrate", &prompt, answer_text(&narration), start)?;
        self.log_stats(op_id, "narrate")?;
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
        self.log_stats(op_id, "narrate")?;
        Ok(narration?)
    }

    /// The names a speech tag must not print, from the state `now`, as the
    /// screens will read them after the Commit (data shape 3.1). No model
    /// call.
    fn unnameable_names(&self, now: &WorldView) -> Result<HashSet<String>, TurnError> {
        Ok(world::unnameable_names(now))
    }

    /// Every step before the Commit, against `view` (the state before the
    /// Turn). One retry in total, shared by parse, shape and Commit checks.
    ///
    /// The adults-only gate (ADR 0008): a final rejection
    /// (`CheckKind::is_final`) ends the Turn at once as a Refusal, with no
    /// retry prompt and no second proposal call. When the raw text of the
    /// first attempt says `intimate` is true (`states_intimate`, no
    /// parsing), the retry is checked as intimate too, so a retry cannot
    /// drop the tag (plan deviation 4, review M2). A retry that
    /// fails the stance rule only is an `Unwilling` Refusal.
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
        let mut wait_asked = None;
        let mut tagged = false;
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
            self.log_stats(op_id, attempt)?;
            let raw = raw?;

            let step = Instant::now();
            let retry = attempt == "propose_retry";
            // The tag sticks once any attempt's raw text says it, valid
            // JSON or not: cut, fenced, with trailing text, or with the key
            // twice (review M2). The retry is then checked as intimate.
            tagged |= states_intimate(&raw);
            let parsed = match parse(&raw) {
                Ok(mut p) => {
                    p.intimate |= tagged;
                    // The items the Intent names: the only ones `settles`
                    // may close (review M1).
                    p.offered.clone_from(&ctx.handles.open_items);
                    // `settle` first: it may drop `move_to`, and a
                    // contested move with no destination is an act for
                    // `engine_time` (review minor).
                    let (p, mut notes) = checks::settle(p, view);
                    let (mut p, asked) = engine_time(p, view, intent);
                    wait_asked = asked;
                    // A contested act with no Check: one repair note on the
                    // first attempt, a default Check on the retry (ADR 0007).
                    // A contested `check` verdict gets it at once.
                    if retry || checks::checks_now(&p) {
                        notes.extend(checks::default_check(&mut p, view, intent));
                    }
                    notes.extend(checks::oppose(&mut p, view, intent));
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
                    return self.resolve(op_id, valid, view, seed, wait_asked, timings);
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
            if let Some((kind, blocked)) = mature::final_refusal(&errs) {
                return Err(TurnError::Refused { kind, blocked });
            }
            if retry && mature::only_unwilling(&errs) {
                return Err(TurnError::Refused {
                    kind: RefusalKind::Unwilling,
                    blocked: Vec::new(),
                });
            }
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
    /// Events, for a valid proposal. Then G3 (ADR 0008): for an intimate
    /// proposal, everyone at the PC's place once these ops are committed,
    /// arrivals of this Turn included, must be cleared too. A failure is a
    /// final Refusal before the Commit, so a due World Event fires in a
    /// later Turn.
    fn resolve(
        &mut self,
        op_id: &str,
        valid: Validated<'_>,
        view: &WorldView,
        seed: u64,
        wait_asked: Option<u32>,
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

        if p.intimate {
            let step = Instant::now();
            let presence = mature::after(view, &ops);
            if let Err(blocked) = mature::check_after(p, view, &presence) {
                let raw = format!("adults only after the ops: {:?}", blocked.who);
                self.log(op_id, "gate_after", &Prompt::default(), &raw, step)?;
                return Err(TurnError::Refused {
                    kind: RefusalKind::AdultsOnly,
                    blocked: blocked.who,
                });
            }
            self.log(op_id, "gate_after", &Prompt::default(), "ok", step)?;
        }

        let proposal_json = json!({
            "proposal": p,
            "reactions": resolved.reactions,
            "events": events,
            "roll": roll,
            "wait_asked": wait_asked,
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
            wait_asked,
            intimate: p.intimate,
        })
    }

    /// One `metrics` row after a model call: the server's numbers for it
    /// (`Gm::last_call_stats`), as `call=<step> prompt_eval_count=…`. No row
    /// when the adapter reports none.
    fn log_stats(&self, op_id: &str, call: &str) -> Result<(), StoreError> {
        let Some(stats) = self.gm.last_call_stats() else {
            return Ok(());
        };
        self.store.log_debug(DebugEntry {
            op_id: op_id.to_owned(),
            step: METRICS_STEP.to_owned(),
            prompt: String::new(),
            raw: format!("call={call} {stats}"),
            ms: stats.prompt_ms + stats.eval_ms,
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
///
/// A raw proposal without the `intimate` key is a schema error: the field
/// keeps `serde(default)` for stored proposals, so without this rule a
/// missing tag would read as `false` and skip the age rule (plan
/// deviation 5).
fn parse(raw: &str) -> Result<DecisionProposal, Vec<Rejection>> {
    let raw = raw.trim();
    let p: DecisionProposal =
        serde_json::from_str(raw).map_err(|e| schema_error(format!("invalid JSON: {e}")))?;
    let tagged = serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .is_some_and(|v| v.get("intimate").is_some_and(serde_json::Value::is_boolean));
    if !tagged {
        return Err(schema_error(
            "intimate is required: true or false".to_owned(),
        ));
    }
    Ok(p)
}

/// True when the raw model text says `intimate` is true anywhere, read
/// without parsing (review M2): the word `intimate`, then any quotes,
/// spaces, `:` or `=`, then `true`, in any case. A text that holds the key
/// twice with two values says it. The tag then sticks: a wrong tag only
/// refuses a Turn, a lost tag skips the age rule.
fn states_intimate(raw: &str) -> bool {
    let text = raw.to_lowercase();
    let mut rest = text.as_str();
    while let Some(at) = rest.find("intimate") {
        rest = &rest[at + "intimate".len()..];
        let value = rest
            .trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ':' | '='));
        if value.starts_with("true") {
            return true;
        }
    }
    false
}

/// The engine owns the time of a Turn, so the model cannot freeze the World
/// Clock with `minutes: 0`. A wait takes `world::wait_minutes`, but only when
/// the Intent asks for one (`world::asks_to_wait`): the model labels a
/// passive act `wait`, and "I warm my hands at the fire" took 60 minutes
/// (bug pass 3). Such a wait is an act. A contested wait is an act too, and
/// so is a contested move with no destination: the QA arm-wrestle came as
/// `move` with `move_to: null`, and the narrator told a move that did not
/// happen. Any other Turn with no minutes takes the Ruleset default of its
/// action (`rules::default_minutes`). It runs after `checks::settle`, which
/// may drop `move_to`, and before the Commit checks, so they see the final
/// action and minutes. Relabelling the action adds no PC act.
///
/// Bug pass 5 (M3): QA saw "wait 30 minutes" take 60, "sleep for eight
/// hours" take 10 and a one-line talk take 90. So:
/// - when a wait Intent states a duration (`world::intent_minutes`), the
///   Turn is a wait of that many minutes, whatever the model labelled it
///   or wrote in `minutes`, unless it is contested or moves the PC; the
///   wait still stops at a World Event the PC witnesses. The second value
///   is the minutes asked, so the narrator can say that a wait ended early;
/// - any other action takes at most `rules::max_minutes` of the model's
///   minutes, or the duration the Intent states when that is longer;
/// - a move to the place where the PC already stands is an act ("I walk
///   over to Sefa's corner" came as a move to the inn, and the narrator was
///   told that the move did not happen).
fn engine_time(
    mut p: DecisionProposal,
    view: &WorldView,
    intent: &str,
) -> (DecisionProposal, Option<u32>) {
    if p.action == ActionKind::Move && p.move_to.is_some_and(|h| world::is_pc_place(view, h)) {
        p.move_to = None;
        p.action = ActionKind::Act;
    }
    let asks_to_wait = !p.contested && world::asks_to_wait(intent);
    let stated = if asks_to_wait {
        world::intent_minutes(intent, view.clock_min)
    } else {
        None
    };
    if stated.is_some() && p.move_to.is_none() {
        p.action = ActionKind::Wait;
    }
    let not_a_wait = p.contested || !asks_to_wait;
    if p.action == ActionKind::Wait && not_a_wait {
        p.action = ActionKind::Act;
    }
    if p.action == ActionKind::Move && p.contested && p.move_to.is_none() {
        p.action = ActionKind::Act;
    }
    let mut asked = None;
    if p.action == ActionKind::Wait {
        let requested = stated.unwrap_or(p.minutes);
        p.minutes = world::wait_minutes(view, requested);
        asked = stated;
    } else if p.minutes == 0 {
        p.minutes = crate::rules::default_minutes(p.action);
    } else {
        // The Player's own duration may be longer than the cap: "I work at
        // the mill for four days".
        let stated = world::intent_minutes(intent, view.clock_min).unwrap_or(0);
        p.minutes = p
            .minutes
            .min(crate::rules::max_minutes(p.action).max(stated));
    }
    (p, asked)
}

/// The outcome line of a wait that a World Event (or the longest wait,
/// `world::WAIT_MAX_MIN`) cut short (bug pass 5, M3): QA asked for an hour,
/// got 25 minutes, and the story did not say so.
pub fn wait_cut_line(minutes: u32, asked: u32) -> Option<String> {
    (minutes < asked)
        .then(|| format!("- The wait ends early: {minutes} of the {asked} minutes asked pass.\n"))
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

/// The minutes a wait Intent stated, stored in a Turn's `proposal_json`
/// (bug pass 5, M3); `None` for any other Turn and for a Turn stored before.
pub fn stored_wait_asked(proposal_json: &str) -> Option<u32> {
    let record: serde_json::Value = serde_json::from_str(proposal_json).ok()?;
    serde_json::from_value(record.get("wait_asked")?.clone()).ok()?
}

/// The `settles` stored in a Turn's `proposal_json`, if its proposal had
/// one. The Decisions rule reads it (`world::sight::decision`).
pub fn stored_settles(proposal_json: &str) -> Option<crate::domain::Settle> {
    let record: serde_json::Value = serde_json::from_str(proposal_json).ok()?;
    serde_json::from_value(record.get("proposal")?.get("settles")?.clone()).ok()
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

/// The OOC prompt: rules, the narration prompt's stable block, then the
/// OOC sheet (`view::ooc_sheet`): what the Play screen shows, made from
/// `PcSight` only (bug pass 4, MB), then the question.
fn ooc_prompt(view: &WorldView, sight: Option<&PcSight>, question: &str) -> Prompt {
    let stable = adventure_block(view, None);
    let scene = sight.map_or_else(
        || "NOW\nPlace: unknown\n".to_owned(),
        crate::view::ooc_sheet,
    );
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
