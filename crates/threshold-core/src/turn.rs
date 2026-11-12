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
    closing_schema_for,
    context::{
        QUIET_EVENT_LINE, adventure_block, closing_context, guard_events, guard_plans,
        narration_prompt_with, one_line, opening_prompt, perceived_by_pc, proposal_context,
        seen_present,
    },
    domain::{
        ActionKind, AdventureSeed, ClockMin, ConfirmedAges, DecisionProposal, DetailLevel,
        EntityId, EntityKind, EntityRef, FactSource, Handle, Intimacy, LedgerOp, Reaction,
        SceneWhy, Verdict,
    },
    mature::{self, RefusalKind, Told},
    model::{Gm, GmError, Prompt, Stream},
    pack::ContentPack,
    proposal_schema_for,
    rules::{RollResult, Ruleset, turn_seed},
    speech::{self, TagFilter},
    store::{
        CampaignId, CommitError, DebugEntry, OpId, Store, StoreError, StoredTurn, TurnId, TurnKind,
        TurnRecord, WorldView,
    },
    view::RefusalView,
    world::{self, PcSight},
};

/// The longest Intent, in characters.
pub const MAX_INTENT_CHARS: usize = 2_000;

/// The debug step of a row with the server's numbers for one model call
/// (`Engine::log_stats`).
pub const METRICS_STEP: &str = "metrics";

/// The `op_id` of the Opening Turn: a Campaign has one Begin at most.
pub const BEGIN_OP_ID: &str = "__begin__";

/// The debug step of the row that records a Refusal under its `op_id`
/// (`Engine::stored_refusal`).
pub const REFUSAL_STEP: &str = "refusal";

/// The end of the `op_id` of the Closing Turn that follows a faded Turn of
/// an Intimate Scene: the faded Turn's `op_id`, then this. A retry of the
/// same op finds it.
pub const FADE_CLOSE_SUFFIX: &str = "__fade_close";

/// The raw text of a `REFUSAL_STEP` row.
#[derive(serde::Serialize, serde::Deserialize)]
struct StoredRefusal {
    kind: RefusalKind,
    blocked: Vec<EntityId>,
}

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
    /// The Narration; for a faded Turn, its Neutral Line.
    pub narration: String,
    pub timings: Timings,
    /// The Player faded this Turn's Narration (Fade to Black).
    pub faded: bool,
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
    /// "End scene" with no Intimate Scene open.
    NoScene,
    /// "Fade to black" with no Turn running and no Intimate Scene open.
    NothingToFade,
    /// `renarrate` of a faded Turn: a Fade to Black cannot be undone.
    Faded,
    /// An Age Confirmation in play that the core refuses
    /// (`mature::confirmable`): the same error for a minor, a character the
    /// PC does not know and anything that is no character.
    NotConfirmable,
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
            Self::NoScene => f.write_str("no Intimate Scene is open"),
            Self::NothingToFade => f.write_str("nothing to fade"),
            Self::Faded => f.write_str("this Turn was faded: it is not narrated again"),
            Self::NotConfirmable => f.write_str(crate::view::CANNOT_CONFIRM),
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

/// What the caller of a Turn hands the engine for its Narration (data
/// shape 3.6): the reader of the stream, and the Player's fade flag.
pub struct TurnControl<'a> {
    /// Gets each piece of the Narration the Player may read.
    pub on_chunk: &'a mut dyn FnMut(&str),
    /// True once the Player asked to fade the Narration (Fade to Black).
    /// `narrate_through` reads it before the narration call and after each
    /// chunk; the Engine reads it once more when the call returns.
    pub faded: &'a dyn Fn() -> bool,
}

/// The fade flag of a Closing Turn that is born faded: always up.
fn always_faded() -> bool {
    true
}

/// What one narration call gave.
enum Narrated {
    /// The Narration, read in full.
    Read(String),
    /// The Player faded it: no text is kept.
    Faded,
}

/// The fade flag of a caller with no fade button: never up.
fn never_faded() -> bool {
    false
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
    /// A Closing Turn: told plain under `context::CLOSING_LINE`, with no
    /// Intent and `mature::CLOSING_NARRATION_MAX_TOKENS`.
    closing: bool,
}

/// What a Turn asks the GM model to decide (data shape 3.7): the Player's
/// Intent, or the end of the open scene. Both share `plan`; a Closing Turn
/// has its own prompt (`context::closing_context`), schema
/// (`closing_schema_for`) and repairs (`checks::settle_closing`), and its
/// Commit closes the scene for `why`.
#[derive(Clone, Copy, Debug)]
enum Ask<'a> {
    Play { intent: &'a str },
    Closing { why: SceneWhy },
}

impl Ask<'_> {
    /// The Intent of a Play Turn; empty for a Closing Turn.
    fn intent(&self) -> &str {
        match self {
            Self::Play { intent } => intent,
            Self::Closing { .. } => "",
        }
    }
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
    /// The proposal's intimacy tag, after the strongest tag of the raw
    /// answers stuck.
    intimacy: Intimacy,
}

impl Planned {
    /// The core's own closing (`mature::default_close`), when the model gave
    /// no closing the checks accept. Its record holds a `wait` that is
    /// `allowed`, no reaction and no event.
    fn default_close(why: SceneWhy) -> Self {
        let minutes = mature::SCENE_CLOSE_DEFAULT_MIN;
        Self {
            ops: mature::default_close(why),
            proposal_json: json!({
                "proposal": {"action": "wait", "verdict": "allowed", "minutes": minutes, "intimacy": "none"},
                "reactions": [],
                "events": [],
                "roll": null,
                "wait_asked": null,
                "default_close": true,
            })
            .to_string(),
            roll: None,
            reactions: Vec::new(),
            events: Vec::new(),
            minutes,
            action: ActionKind::Wait,
            verdict: Verdict::Allowed,
            wait_asked: None,
            intimacy: Intimacy::None,
        }
    }
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
        let mut ctl = TurnControl {
            on_chunk,
            faded: &never_faded,
        };
        self.play_turn_with(op_id, intent, &mut ctl)
    }

    /// `play_turn` with the full control of the Narration: the stream
    /// reader and the fade flag.
    pub fn play_turn_with(
        &mut self,
        op_id: OpId,
        intent: &str,
        ctl: &mut TurnControl<'_>,
    ) -> Result<TurnOutcome, TurnError> {
        let start = Instant::now();
        if let Some(stored) = self.store.turn_by_campaign_op(self.campaign, &op_id)? {
            return self.stored_outcome(stored, ctl);
        }
        self.stored_refusal(&op_id)?;
        check_intent(intent)?;
        let view = self.store.snapshot(self.campaign)?;
        let seed = turn_seed(self.store.campaign_seed(self.campaign)?, view.turn_seq + 1);
        let mut timings = Timings::default();
        let planned = self.plan(&op_id, &view, Ask::Play { intent }, seed, &mut timings);
        let planned = self.logged_refusal(&op_id, planned)?;
        let record = TurnRecord {
            campaign_id: self.campaign,
            op_id,
            kind: TurnKind::Play,
            intent: intent.to_owned(),
            proposal_json: planned.proposal_json.clone(),
            clock_before: view.clock_min,
            clock_after: view.clock_min + ClockMin::from(planned.minutes),
        };
        self.finish(record, planned, Vec::new(), timings, start, ctl)
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
        let mut ctl = TurnControl {
            on_chunk,
            faded: &never_faded,
        };
        self.correct_last_with(op_id, clarified_intent, why, &mut ctl)
    }

    /// `correct_last` with the full control of the Narration.
    pub fn correct_last_with(
        &mut self,
        op_id: OpId,
        clarified_intent: &str,
        why: &str,
        ctl: &mut TurnControl<'_>,
    ) -> Result<TurnOutcome, TurnError> {
        let start = Instant::now();
        if let Some(stored) = self.store.turn_by_campaign_op(self.campaign, &op_id)? {
            return self.stored_outcome(stored, ctl);
        }
        self.stored_refusal(&op_id)?;
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
        let ask = Ask::Play {
            intent: clarified_intent,
        };
        let planned = self.plan(&op_id, &view, ask, seed, &mut timings);
        let mut planned = self.logged_refusal(&op_id, planned)?;
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
        self.finish(record, planned, head, timings, start, ctl)
    }

    /// "End scene" (data shape 3.7): commits a Closing Turn that ends the
    /// open Intimate Scene, then tells it in a short plain Narration. The
    /// Closing Turn has no Intent and sets no act of the PC. It runs `plan`
    /// as any Turn, with the closing prompt, schema and repairs. When the
    /// model fails, or both its attempts are rejected, the core commits its
    /// own closing (`mature::default_close`): the button never depends on a
    /// model answer. Either way the Commit closes the scene with
    /// `why: EndScene`. A second call with the same `op_id` returns the
    /// stored Turn. `NoScene` when no scene is open, with no model call.
    pub fn end_scene(
        &mut self,
        op_id: OpId,
        ctl: &mut TurnControl<'_>,
    ) -> Result<TurnOutcome, TurnError> {
        self.close_scene(op_id, SceneWhy::EndScene, ctl)
    }

    /// "Fade to black" while no Turn runs (data shape 3.6): with an
    /// Intimate Scene open, a Closing Turn born faded closes it (one
    /// proposal call, no narration call, `why: FadeToBlack`); the Turns
    /// already read keep their text. With no scene, `NothingToFade`. While a
    /// Turn runs, the shell raises the caller's fade flag instead and the
    /// running Turn fades itself (`TurnControl::faded`).
    ///
    /// `op_id` may name the Turn that was streaming when the Player pressed
    /// the button and that ended before it read the flag (review minor,
    /// 2026-11-12): when that Turn is the latest Turn of the Campaign, is
    /// not faded, and is a Play, Correction or Closing Turn
    /// (`TurnKind::fades`), it is faded now, and a scene it left open
    /// closes, faded. Any other stored Turn (an older one, the Opening, one
    /// already faded) is returned as it is stored, with no model call: its
    /// Narration is not asked for again even when it is missing. So `fade`
    /// never makes a model call but the proposal of a faded Closing Turn.
    pub fn fade(&mut self, op_id: OpId) -> Result<TurnOutcome, TurnError> {
        let mut sink = |_: &str| {};
        let mut ctl = TurnControl {
            on_chunk: &mut sink,
            faded: &always_faded,
        };
        if let Some(stored) = self.store.turn_by_campaign_op(self.campaign, &op_id)? {
            let latest = self.store.turns(self.campaign)?.last().map(|t| t.id) == Some(stored.id);
            if latest && !stored.faded && stored.record.kind.fades() {
                let narration = self.after_fade(stored.id, &op_id)?;
                return Ok(TurnOutcome {
                    turn_id: stored.id,
                    roll: stored_roll(&stored.record.proposal_json),
                    narration,
                    timings: Timings::default(),
                    faded: true,
                });
            }
            return Ok(as_stored(&stored));
        }
        if self.store.snapshot(self.campaign)?.scene_open.is_none() {
            return Err(TurnError::NothingToFade);
        }
        self.close_scene(op_id, SceneWhy::FadeToBlack, &mut ctl)
    }

    /// After the Player faded the Narration of `turn_id`: the row loses
    /// its text for good (`Store::set_faded`). When an Intimate Scene is
    /// still open and the Turn is not itself a Closing Turn, a Closing Turn
    /// born faded closes it, in the same call, under the op
    /// `<op_id>FADE_CLOSE_SUFFIX`. The faded Turn's outcome carries its
    /// Neutral Line.
    fn after_fade(&mut self, turn_id: TurnId, op_id: &str) -> Result<String, TurnError> {
        self.store.set_faded(turn_id)?;
        let stored = self
            .store
            .turn(turn_id)?
            .ok_or_else(|| StoreError::Invalid("unknown turn".into()))?;
        if stored.record.kind != TurnKind::Closing
            && self.store.snapshot(self.campaign)?.scene_open.is_some()
        {
            let mut sink = |_: &str| {};
            let mut ctl = TurnControl {
                on_chunk: &mut sink,
                faded: &always_faded,
            };
            let close = format!("{op_id}{FADE_CLOSE_SUFFIX}");
            match self.close_scene(close, SceneWhy::FadeToBlack, &mut ctl) {
                Ok(_) | Err(TurnError::NoScene) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(mature::neutral_line(&stored).unwrap_or_default())
    }

    /// The Closing Turn of "End scene" (and of a Fade to Black), for `why`.
    fn close_scene(
        &mut self,
        op_id: OpId,
        why: SceneWhy,
        ctl: &mut TurnControl<'_>,
    ) -> Result<TurnOutcome, TurnError> {
        let start = Instant::now();
        if let Some(stored) = self.store.turn_by_campaign_op(self.campaign, &op_id)? {
            return self.stored_outcome(stored, ctl);
        }
        let view = self.store.snapshot(self.campaign)?;
        // The ledger's scene, not `mature::open_scene`: closing a scene that
        // G1 no longer clears is always safe, and its prompt then holds no
        // scene text (G1 in `plan`).
        if view.scene_open.is_none() {
            return Err(TurnError::NoScene);
        }
        let seed = turn_seed(self.store.campaign_seed(self.campaign)?, view.turn_seq + 1);
        let mut timings = Timings::default();
        let planned = match self.plan(&op_id, &view, Ask::Closing { why }, seed, &mut timings) {
            Ok(planned) => planned,
            Err(TurnError::Store(e)) => return Err(TurnError::Store(e)),
            Err(e) => {
                let raw = format!("the core closes the scene: {e}");
                self.log(
                    &op_id,
                    "default_close",
                    &Prompt::default(),
                    &raw,
                    Instant::now(),
                )?;
                Planned::default_close(why)
            }
        };
        let record = TurnRecord {
            campaign_id: self.campaign,
            op_id,
            kind: TurnKind::Closing,
            intent: String::new(),
            proposal_json: planned.proposal_json.clone(),
            clock_before: view.clock_min,
            clock_after: view.clock_min + ClockMin::from(planned.minutes),
        };
        self.finish(record, planned, Vec::new(), timings, start, ctl)
    }

    /// Begin: commits the Opening Turn (zero ops, empty Intent, no World
    /// Clock move, no World Event) and narrates the start state. It decides
    /// nothing for the PC. A second call returns the stored Turn and makes no
    /// model call, unless its Narration is missing: then it narrates again.
    /// Refused once a play Turn exists.
    pub fn begin(&mut self, on_chunk: &mut dyn FnMut(&str)) -> Result<TurnOutcome, TurnError> {
        let mut ctl = TurnControl {
            on_chunk,
            faded: &never_faded,
        };
        let ctl = &mut ctl;
        let start = Instant::now();
        let op_id: OpId = BEGIN_OP_ID.to_owned();
        if let Some(stored) = self.store.turn_by_campaign_op(self.campaign, &op_id)? {
            return self.stored_outcome(stored, ctl);
        }
        if self
            .store
            .turns(self.campaign)?
            .iter()
            .any(|t| t.record.kind.has_intent())
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
            return self.duplicate_outcome(&op_id, ctl);
        };
        let view_after = self.store.snapshot(self.campaign)?;
        let step = Instant::now();
        let narration = self
            .narrate_opening(&op_id, &view_after, &view_after, ctl)
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
            faded: false,
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
        let answer = self.gm.narrate(&prompt, None, &mut |_| Stream::More);
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
        let mut ctl = TurnControl {
            on_chunk,
            faded: &never_faded,
        };
        self.renarrate_with(turn, &mut ctl)
    }

    /// `renarrate` with the full control of the Narration.
    pub fn renarrate_with(
        &mut self,
        turn: TurnId,
        ctl: &mut TurnControl<'_>,
    ) -> Result<String, TurnError> {
        let stored = self
            .store
            .turn(turn)?
            .filter(|t| t.record.campaign_id == self.campaign)
            .ok_or_else(|| StoreError::Invalid("unknown turn".into()))?;
        if stored.faded {
            return Err(TurnError::Faded);
        }
        let view_after = self.store.snapshot_before(self.campaign, stored.seq + 1)?;
        // The screens read the story against the state now, so the stream
        // filter does too.
        let now = self.store.snapshot(self.campaign)?;
        if stored.record.kind == TurnKind::Opening {
            let op_id = &stored.record.op_id;
            let narration = self.narrate_opening(op_id, &view_after, &now, ctl)?;
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
            closing: stored.record.kind == TurnKind::Closing,
        };
        let narrated = self.narrate(
            &stored.record.op_id,
            &view_after,
            &now,
            &committed,
            &stored.record.intent,
            ctl,
        )?;
        match narrated {
            Narrated::Read(narration) => {
                self.store.set_narration(turn, &narration)?;
                Ok(narration)
            }
            Narrated::Faded => self.after_fade(turn, &stored.record.op_id),
        }
    }

    /// Age Confirmation in play (data shape 3.2): the Player confirms that
    /// `entity` is an adult. The core writes `ConfirmAge` in an OOC Turn
    /// (`commit_ooc`) when `mature::confirmable` holds, and `NotConfirmable`
    /// otherwise, with no Turn row: the same answer for a character under
    /// 18, one the PC does not know, and anything that is no character. A
    /// character already confirmed, or a second call with the same `op_id`,
    /// commits nothing. It never changes an age. No model call.
    pub fn confirm_age(&mut self, op_id: OpId, entity: EntityId) -> Result<(), TurnError> {
        if self
            .store
            .turn_by_campaign_op(self.campaign, &op_id)?
            .is_some()
        {
            return Ok(());
        }
        let view = self.store.snapshot(self.campaign)?;
        if !mature::confirmable(&view, entity) {
            return Err(TurnError::NotConfirmable);
        }
        if view
            .entities
            .iter()
            .any(|e| e.id == entity && e.age_confirmed)
        {
            return Ok(());
        }
        let op = LedgerOp::ConfirmAge {
            entity: EntityRef::Id(entity),
        };
        self.commit_ooc(op_id, &view, vec![op]).map(|_| ())
    }

    /// The Player changes the Detail Level in play (data shape 3.1, 3.9):
    /// `SetDetailLevel` in an OOC Turn. At `excluded`, an open Intimate
    /// Scene closes in the same Turn (`SetScene`, `why: Excluded`, data
    /// shape 3.4 row 8). No level clears anyone: the age rule reads ages and
    /// confirmations only. A level lowered applies from the next narration
    /// on; a level raised never reaches a Turn committed before it
    /// (`mature::told`). The level the Campaign already has, or a second
    /// call with the same `op_id`, commits nothing. No model call.
    pub fn set_detail_level(&mut self, op_id: OpId, level: DetailLevel) -> Result<(), TurnError> {
        if self
            .store
            .turn_by_campaign_op(self.campaign, &op_id)?
            .is_some()
        {
            return Ok(());
        }
        let view = self.store.snapshot(self.campaign)?;
        if view.detail == level {
            return Ok(());
        }
        let mut ops = vec![LedgerOp::SetDetailLevel { level }];
        if level == DetailLevel::Excluded && view.scene_open.is_some() {
            ops.push(LedgerOp::SetScene {
                open: false,
                why: SceneWhy::Excluded,
            });
        }
        self.commit_ooc(op_id, &view, ops).map(|_| ())
    }

    /// The Refusal of `kind` as the Play screen shows it (data shape 3.11):
    /// its sentence, and for `adults_only` the characters the PC perceives
    /// here and can name that the Player may confirm (`mature::to_confirm`).
    /// It reads the state now and never the characters who blocked the
    /// Turn, so a hidden cause changes nothing the Player reads.
    pub fn refusal_view(&self, kind: RefusalKind) -> Result<RefusalView, TurnError> {
        let view = self.store.snapshot(self.campaign)?;
        let turns = self.store.turns(self.campaign)?;
        let reverted: HashSet<TurnId> = self
            .store
            .reverted_turns(self.campaign)?
            .into_iter()
            .collect();
        let confirm = mature::to_confirm(&view);
        Ok(match crate::world::pc_sight(&view, &turns, &reverted) {
            Some(sight) => crate::view::refusal(&sight, kind, &confirm),
            None => RefusalView {
                kind,
                text: crate::view::refusal_text(kind).to_owned(),
                to_confirm: Vec::new(),
            },
        })
    }

    /// Commits one OOC Turn (data shape 3.9): a setting the Player changed
    /// in play. Empty Intent, `{}`, no clock change, no model call. It is in
    /// no story, no Moment and no Turn count, and as the latest Turn it
    /// closes "Correct last Turn" until the next Play Turn (owner's
    /// decision). A Turn already stored under `op_id` is returned as it is.
    fn commit_ooc(
        &mut self,
        op_id: OpId,
        view: &WorldView,
        ops: Vec<LedgerOp>,
    ) -> Result<TurnId, TurnError> {
        let record = TurnRecord {
            campaign_id: self.campaign,
            op_id: op_id.clone(),
            kind: TurnKind::Ooc,
            intent: String::new(),
            proposal_json: "{}".into(),
            clock_before: view.clock_min,
            clock_after: view.clock_min,
        };
        let mut timings = Timings::default();
        match self.commit_logged(record, ops, &mut timings)? {
            Some(id) => Ok(id),
            None => Ok(self
                .store
                .turn_by_campaign_op(self.campaign, &op_id)?
                .ok_or_else(|| StoreError::Invalid("duplicate op without a turn".into()))?
                .id),
        }
    }

    /// A Refusal is not a failed Turn (review M3): the same `op_id` gets the
    /// same Refusal back, with no model call. `Err` with that Refusal when
    /// the debug log holds a `REFUSAL_STEP` row for `op_id`; the log is
    /// written in every Campaign file, so this holds after a restart too.
    fn stored_refusal(&self, op_id: &str) -> Result<(), TurnError> {
        let refusal = self
            .store
            .debug_log(op_id)?
            .into_iter()
            .rev()
            .find(|row| row.step == REFUSAL_STEP)
            .and_then(|row| serde_json::from_str::<StoredRefusal>(&row.raw).ok());
        match refusal {
            Some(StoredRefusal { kind, blocked }) => Err(TurnError::Refused { kind, blocked }),
            None => Ok(()),
        }
    }

    /// Writes one `REFUSAL_STEP` row when `planned` is a Refusal, then hands
    /// `planned` on.
    fn logged_refusal(
        &self,
        op_id: &str,
        planned: Result<Planned, TurnError>,
    ) -> Result<Planned, TurnError> {
        if let Err(TurnError::Refused { kind, blocked }) = &planned {
            let raw = serde_json::to_string(&StoredRefusal {
                kind: *kind,
                blocked: blocked.clone(),
            })
            .map_err(|e| StoreError::Invalid(e.to_string()))?;
            self.log(
                op_id,
                REFUSAL_STEP,
                &Prompt::default(),
                &raw,
                Instant::now(),
            )?;
        }
        planned
    }

    /// The outcome of a Turn already stored under this `op_id`.
    fn stored_outcome(
        &mut self,
        stored: StoredTurn,
        ctl: &mut TurnControl<'_>,
    ) -> Result<TurnOutcome, TurnError> {
        // A faded Turn gives its Neutral Line and makes no model call.
        let narration = match (&stored.narration, stored.faded) {
            (_, true) => mature::neutral_line(&stored).unwrap_or_default(),
            (Some(narration), false) => narration.clone(),
            (None, false) => self
                .renarrate_with(stored.id, ctl)
                .map_err(|e| narration_failed(stored.id, e))?,
        };
        let faded = stored.faded || self.store.turn(stored.id)?.is_some_and(|t| t.faded);
        Ok(TurnOutcome {
            turn_id: stored.id,
            roll: stored_roll(&stored.record.proposal_json),
            narration,
            timings: Timings::default(),
            faded,
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
        ctl: &mut TurnControl<'_>,
    ) -> Result<TurnOutcome, TurnError> {
        let stored = self
            .store
            .turn_by_campaign_op(self.campaign, op_id)?
            .ok_or_else(|| StoreError::Invalid("duplicate op without a turn".into()))?;
        self.stored_outcome(stored, ctl)
    }

    /// Commits the planned ops (after `head`), then narrates.
    fn finish(
        &mut self,
        record: TurnRecord,
        planned: Planned,
        head: Vec<LedgerOp>,
        mut timings: Timings,
        start: Instant,
        ctl: &mut TurnControl<'_>,
    ) -> Result<TurnOutcome, TurnError> {
        let op_id = record.op_id.clone();
        let intent = record.intent.clone();
        let closing = record.kind == TurnKind::Closing;
        let mut ops = head;
        ops.extend(planned.ops);
        let Some(turn_id) = self.commit_logged(record, ops.clone(), &mut timings)? else {
            return self.duplicate_outcome(&op_id, ctl);
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
            intimate: planned.intimacy.intimate(),
            closing,
        };
        let narrated = self
            .narrate(&op_id, &view_after, &view_after, &committed, &intent, ctl)
            .map_err(|e| narration_failed(turn_id, e))?;
        timings.narrate_ms = ms(step);
        timings.narration_prompt_tokens = self.gm.last_prompt_tokens();
        let (narration, faded) = match narrated {
            Narrated::Read(narration) => {
                self.store.set_narration(turn_id, &narration)?;
                (narration, false)
            }
            Narrated::Faded => (self.after_fade(turn_id, &op_id)?, true),
        };
        timings.total_ms = ms(start);
        Ok(TurnOutcome {
            turn_id,
            roll: planned.roll,
            narration,
            timings,
            faded,
        })
    }

    /// The narration call. Only what the PC perceives reaches it, and only
    /// the reactions of NPCs the PC knows are here: the others are offscreen
    /// intents. A plan or a World Event text that names a secret is dropped
    /// (`guard_plans`, `guard_events`). `now` is the current state: the
    /// stream filter reads it (`narrate_through`). G4: `mature::told`
    /// chooses how the Turn is told from the state after it, the state now
    /// and the loaded packs: an intimate Turn is told by implication, with
    /// the core's own line, or, in a cleared open scene at an effective
    /// `explicit` level, with the packs' explicit fragments behind the
    /// clearance `told` holds (ADR 0009). Inside an open scene that is
    /// still open `now`, the earlier Turns of the scene reach the prompt in
    /// full only through the clearance of the state after the Turn
    /// (`mature::narration_clearance`). A Narration told as intimate carries
    /// `mature::SCENE_NARRATION_MAX_TOKENS`, a Closing Turn's
    /// `mature::CLOSING_NARRATION_MAX_TOKENS`.
    fn narrate(
        &mut self,
        op_id: &str,
        view_after: &WorldView,
        now: &WorldView,
        committed: &Committed<'_>,
        intent: &str,
        ctl: &mut TurnControl<'_>,
    ) -> Result<Narrated, TurnError> {
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
        let told = mature::told(view_after, now, committed.intimate, &self.packs);
        let cleared = mature::narration_clearance(view_after, now);
        // One rule (review minor, 2026-11-12): a Turn told as intimate, at
        // any level, gets the scene cap, the Turn that ends its scene and a
        // `renarrate` included; a Closing Turn gets the closing cap.
        let max_tokens = if committed.closing {
            Some(mature::CLOSING_NARRATION_MAX_TOKENS)
        } else {
            (!matches!(told, Told::Plain)).then_some(mature::SCENE_NARRATION_MAX_TOKENS)
        };
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
            cleared.as_ref(),
            committed.closing,
        );
        let hidden = self.unnameable_names(now)?;
        let start = Instant::now();
        // A flag already up makes no call, so no `metrics` row either.
        let called = !(ctl.faded)();
        let (narration, stopped) =
            narrate_through(self.gm.as_ref(), &prompt, max_tokens, hidden, ctl);
        // The flag rose during the call, or after its last chunk: the Turn
        // is faded all the same.
        let faded = stopped || (ctl.faded)();
        let cut =
            !faded && narration.is_ok() && self.gm.last_done_reason().as_deref() == Some("length");
        // The debug row keeps the text received before the stop, then a
        // `[faded]` line (owner's decision 5). Only `get_debug` reads it,
        // in debug mode.
        let logged = if faded {
            format!("{}\n[faded]", answer_text(&narration))
        } else if cut {
            format!("{}\n[generation stopped: length]", answer_text(&narration))
        } else {
            answer_text(&narration).to_owned()
        };
        self.log(op_id, "narrate", &prompt, &logged, start)?;
        if called {
            self.log_stats(op_id, "narrate")?;
        }
        if faded {
            return Ok(Narrated::Faded);
        }
        let narration = narration?;
        // The token cap cut the answer: the stored Narration ends on its
        // last full sentence (data shape 3.10), without a last line that is
        // a speech mark cut short (review B2): the reader of a stored
        // Narration would show its tag as prose.
        Ok(Narrated::Read(if cut {
            stored_cut(&narration).to_owned()
        } else {
            narration
        }))
    }

    /// The opening narration call: `opening_prompt` on the state after the
    /// Opening Turn, which equals the state before it. `now` is the current
    /// state, for the stream filter.
    fn narrate_opening(
        &mut self,
        op_id: &str,
        view_after: &WorldView,
        now: &WorldView,
        ctl: &mut TurnControl<'_>,
    ) -> Result<String, TurnError> {
        let prompt = opening_prompt(view_after, &self.packs);
        let hidden = self.unnameable_names(now)?;
        let start = Instant::now();
        // No fade of the Opening Narration (data shape 10): the flag is not
        // read.
        let mut ctl = TurnControl {
            on_chunk: &mut *ctl.on_chunk,
            faded: &never_faded,
        };
        let (narration, _) = narrate_through(self.gm.as_ref(), &prompt, None, hidden, &mut ctl);
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
    ///
    /// G1 (data shape 3.5) runs first, before the proposal call: with an
    /// open scene whose place is cleared (`mature::scene_gate`), the prompt
    /// gets the clearance (the scene line and the scene Turns in full) and
    /// the schema the longer plan. When the place is not cleared, the Turn
    /// is planned as a plain one and its own Commit closes the scene
    /// (`resolve`, plan deviation 1). The clearance lives in this call
    /// only: it is never stored.
    ///
    /// A Closing Turn (`Ask::Closing`) shares every step. It reads the
    /// closing prompt and schema, its repairs are `checks::settle_closing`,
    /// and the engine's time rules of an Intent (`engine_time`, the default
    /// Check, opposition) do not run: it has no Intent.
    fn plan(
        &mut self,
        op_id: &str,
        view: &WorldView,
        ask: Ask<'_>,
        seed: u64,
        timings: &mut Timings,
    ) -> Result<Planned, TurnError> {
        let intent = ask.intent();
        let gate = mature::scene_gate(view);
        if let Err(blocked) = &gate {
            let raw = format!("scene not cleared: {:?}", blocked.who);
            self.log(
                op_id,
                "scene_gate",
                &Prompt::default(),
                &raw,
                Instant::now(),
            )?;
        }
        let gate_failed = gate.is_err();
        let cleared = gate.as_ref().ok().and_then(Option::as_ref);
        let (ctx, schema) = match ask {
            Ask::Play { intent } => {
                let ctx = proposal_context(view, &self.packs, intent, cleared);
                let schema = proposal_schema_for(&ctx.handles);
                (ctx, schema)
            }
            Ask::Closing { .. } => {
                let ctx = closing_context(view, cleared);
                let schema = closing_schema_for(&ctx.handles);
                (ctx, schema)
            }
        };
        let mut prompt = ctx.prompt;
        let mut wait_asked = None;
        let mut tagged = Intimacy::None;
        // The net under the model (ADR 0008): the Intent's own words raise
        // the tag before the gate, never lower it. A Closing Turn has no
        // Intent.
        let floor = match ask {
            Ask::Play { intent } => mature::romance_floor(intent),
            Ask::Closing { .. } => Intimacy::None,
        };
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
            // The strongest tag any attempt's raw text says sticks, valid
            // JSON or not: cut, fenced, with trailing text, or with the key
            // twice (review M2). The retry is then checked with it.
            tagged = tagged.max(stated_intimacy(&raw));
            let parsed = match parse(&raw) {
                Ok(p) if matches!(ask, Ask::Closing { .. }) => {
                    let (p, notes) = checks::settle_closing(p, view);
                    if !notes.is_empty() {
                        self.log(op_id, "settle", &Prompt::default(), &notes.join("\n"), step)?;
                    }
                    Ok(p)
                }
                Ok(mut p) => {
                    p.intimacy = p.intimacy.max(tagged);
                    if floor > p.intimacy {
                        // Which word raised it stays out of the log.
                        let raw =
                            format!("the core raised the tag to {floor:?} from the Intent's words")
                                .to_lowercase();
                        self.log(op_id, "intimacy_floor", &Prompt::default(), &raw, step)?;
                        p.intimacy = floor;
                    }
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
                    return self.resolve(
                        op_id,
                        valid,
                        view,
                        seed,
                        wait_asked,
                        gate_failed,
                        ask,
                        timings,
                    );
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
    /// later Turn. Last, `mature::scene_op` adds the one op that opens or
    /// closes the Intimate Scene, if any, to this Turn's Commit
    /// (`gate_failed`: G1 failed in `plan`).
    #[allow(clippy::too_many_arguments)]
    fn resolve(
        &mut self,
        op_id: &str,
        valid: Validated<'_>,
        view: &WorldView,
        seed: u64,
        wait_asked: Option<u32>,
        gate_failed: bool,
        ask: Ask<'_>,
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
        let (mut ops, events) = world::with_witnessed_events(view, ops);
        timings.world_ms = ms(step);
        self.log(op_id, "world", &Prompt::default(), "", step)?;

        // Who is at the PC's place once the ops are committed: G3 and the
        // scene's state read it. With no scene and no tag, neither does.
        let presence =
            (p.intimacy.gated() || view.scene_open.is_some()).then(|| mature::after(view, &ops));
        if let (true, Some(presence)) = (p.intimacy.gated(), &presence) {
            let step = Instant::now();
            if let Err(blocked) = mature::check_after(p, view, presence) {
                let raw = format!("adults only after the ops: {:?}", blocked.who);
                self.log(op_id, "gate_after", &Prompt::default(), &raw, step)?;
                return Err(TurnError::Refused {
                    kind: RefusalKind::AdultsOnly,
                    blocked: blocked.who,
                });
            }
            self.log(op_id, "gate_after", &Prompt::default(), "ok", step)?;
        }
        // A Closing Turn closes the scene for its own reason; any other
        // Turn changes it as `mature::scene_op` says.
        let scene_op = match ask {
            Ask::Closing { why } => Some(LedgerOp::SetScene { open: false, why }),
            Ask::Play { .. } => presence
                .as_ref()
                .and_then(|presence| mature::scene_op(view, p, presence, gate_failed)),
        };
        if let Some(op) = scene_op {
            self.log(
                op_id,
                "scene",
                &Prompt::default(),
                &format!("{op:?}"),
                Instant::now(),
            )?;
            ops.push(op);
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
            intimacy: p.intimacy,
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

/// The outcome of a stored Turn as it stands, with no model call: its
/// Narration, its Neutral Line when it is faded, or nothing when its
/// Narration is missing.
fn as_stored(stored: &StoredTurn) -> TurnOutcome {
    let narration = if stored.faded {
        mature::neutral_line(stored).unwrap_or_default()
    } else {
        stored.narration.clone().unwrap_or_default()
    };
    TurnOutcome {
        turn_id: stored.id,
        roll: stored_roll(&stored.record.proposal_json),
        narration,
        timings: Timings::default(),
        faded: stored.faded,
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
/// A raw proposal without the `intimacy` key, or with a value that is not
/// one of the three, is a schema error: the field keeps `serde(default)`
/// and reads the old boolean for stored proposals, so without this rule a
/// missing tag would read as `none` and skip the age rule (plan
/// deviation 5).
fn parse(raw: &str) -> Result<DecisionProposal, Vec<Rejection>> {
    let raw = raw.trim();
    let p: DecisionProposal =
        serde_json::from_str(raw).map_err(|e| schema_error(format!("invalid JSON: {e}")))?;
    let tagged = serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|v| v.get("intimacy").cloned())
        .is_some_and(|v| serde_json::from_value::<Intimacy>(v).is_ok());
    if !tagged {
        return Err(schema_error(
            "intimacy is required: none, advance or intimate".to_owned(),
        ));
    }
    Ok(p)
}

/// The strongest intimacy tag the raw model text states anywhere, read
/// without parsing (review M2): the key `intimacy`, then any quotes,
/// spaces, `:` or `=`, then `intimate` or `advance`; or the old key
/// `intimate`, then `true`; in any case. A text that holds the key twice
/// with two values states the stronger one. The tag then sticks: a wrong
/// tag only refuses a Turn, a lost tag skips the age rule.
fn stated_intimacy(raw: &str) -> Intimacy {
    let text = raw.to_lowercase();
    let value_after = |rest: &str| -> String {
        rest.trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ':' | '='))
            .to_owned()
    };
    let mut strongest = Intimacy::None;
    for (key, values) in [
        (
            "intimacy",
            &[
                ("intimate", Intimacy::Intimate),
                ("advance", Intimacy::Advance),
            ][..],
        ),
        ("intimate", &[("true", Intimacy::Intimate)][..]),
    ] {
        let mut rest = text.as_str();
        while let Some(at) = rest.find(key) {
            rest = &rest[at + key.len()..];
            let value = value_after(rest);
            for (word, tag) in values {
                if value.starts_with(word) {
                    strongest = strongest.max(*tag);
                }
            }
        }
    }
    strongest
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

/// `text` up to its last full sentence: the last `.`, `!` or `?` that ends
/// the text or comes before a space or a closing quote, with the closing
/// quotes after it. The whole text when no sentence ends in it. A decimal
/// point ("3.5") ends no sentence.
fn last_full_sentence(text: &str) -> &str {
    const CLOSING: [char; 6] = ['"', '\'', '\u{201d}', '\u{2019}', '\u{bb}', ')'];
    let mut end = None;
    let mut chars = text.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        if !matches!(c, '.' | '!' | '?') {
            continue;
        }
        let mut stop = at + c.len_utf8();
        while let Some(&(next_at, next)) = chars.peek() {
            if !CLOSING.contains(&next) {
                break;
            }
            stop = next_at + next.len_utf8();
            chars.next();
        }
        match chars.peek() {
            None => end = Some(stop),
            Some(&(_, next)) if next.is_whitespace() => end = Some(stop),
            Some(_) => {}
        }
    }
    end.map_or(text, |end| &text[..end])
}

/// A Narration that the token cap cut, as it is stored: up to its last full
/// sentence (`last_full_sentence`), then without a last line that is a
/// speech mark cut short (`speech::without_cut_mark`, review B2). The stored
/// text then holds only lines the model ended, so the reader of a stored
/// Narration (`world::pc_sight`) cleans it as it cleans a full one, and it
/// never shows a tag that the stream held back.
fn stored_cut(text: &str) -> &str {
    speech::without_cut_mark(last_full_sentence(text))
}

/// One narrator call whose stream passes through `speech::TagFilter`: a tag
/// that names someone the PC cannot name never reaches `on_chunk`. The
/// answer, which becomes the stored Narration, is returned unchanged; the
/// commit-time rule of `world::pc_sight` handles it.
///
/// Fade to Black (data shape 3.6): the fade flag is read before the call and
/// after each chunk. A flag already up makes no call. Once it is up, no more
/// text reaches `on_chunk` and the model gets `Stream::Stop`; the text
/// received so far comes back for the debug log only. The second value is
/// true when the flag stopped the Narration.
fn narrate_through(
    gm: &dyn Gm,
    prompt: &Prompt,
    max_tokens: Option<u32>,
    hidden: HashSet<String>,
    ctl: &mut TurnControl<'_>,
) -> (Result<String, GmError>, bool) {
    let faded = ctl.faded;
    if faded() {
        return (Ok(String::new()), true);
    }
    let mut filter = TagFilter::new(hidden);
    let on_chunk = &mut *ctl.on_chunk;
    let mut stopped = false;
    let answer = gm.narrate(prompt, max_tokens, &mut |chunk| {
        if stopped || faded() {
            stopped = true;
            return Stream::Stop;
        }
        let shown = filter.push(chunk);
        if !shown.is_empty() {
            on_chunk(&shown);
        }
        if faded() {
            stopped = true;
            Stream::Stop
        } else {
            Stream::More
        }
    });
    if stopped || faded() {
        return (answer, true);
    }
    // The start of the last line, which the filter still holds: no colon
    // and no line end came. When the token cap cut the answer, that line is
    // cut too, and it may be a tag whose colon never came. The stream then
    // shows of it only what the stored Narration keeps (`stored_cut`), so
    // the two agree (review B2).
    let mut rest = filter.finish();
    if let (Ok(text), Some("length")) = (&answer, gm.last_done_reason().as_deref()) {
        let line = text.rfind('\n').map_or(0, |at| at + 1);
        let kept = stored_cut(text).len().saturating_sub(line).min(rest.len());
        rest = rest.get(..kept).unwrap_or_default().to_owned();
    }
    if !rest.is_empty() {
        on_chunk(&rest);
    }
    (answer, false)
}
