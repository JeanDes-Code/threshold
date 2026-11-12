//! The scenario files in `evals/scenarios/<name>.toml`.

use std::{fs, path::Path};

use serde::Deserialize;
use serde::de::DeserializeOwned;

fn default_fixture() -> String {
    "town".into()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Latency {
    #[serde(default = "default_fixture")]
    pub fixture: String,
    pub intents: Vec<String>,
}

/// Intents that invite dialogue, for the speech mark (ADR 0006).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Speech {
    #[serde(default = "default_fixture")]
    pub fixture: String,
    pub intents: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Memory {
    #[serde(default = "default_fixture")]
    pub fixture: String,
    pub total_turns: usize,
    pub plant_turn: usize,
    pub plant: String,
    pub probe: String,
    pub detail: String,
    pub expect: Vec<String>,
    pub filler: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Voices {
    #[serde(default = "default_fixture")]
    pub fixture: String,
    pub lines: Vec<VoiceLine>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceLine {
    pub speaker: String,
    pub intent: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shortcuts {
    #[serde(default = "default_fixture")]
    pub fixture: String,
    pub traps: Vec<Trap>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trap {
    /// The Commit check, as `CheckKind` prints it ("npc folds").
    pub shortcut: String,
    /// What taking the Shortcut looks like, for the judge.
    pub taken: String,
    pub intents: Vec<String>,
    /// Slots (`[subject, key]`) the Shortcut would change: coincidence and
    /// fact bends read them from the ledger after the Turn.
    #[serde(default)]
    pub slots: Vec<[String; 2]>,
    /// The NPC whose stance npc folds reads.
    pub npc: Option<String>,
    pub event: Option<TrapEvent>,
}

/// A World Event committed as a Setup Turn before the trap Turn.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrapEvent {
    pub actor: String,
    pub text: String,
    pub in_minutes: u64,
    pub subject: String,
    pub key: String,
    pub value: String,
    /// NPCs who witness the event when it fires.
    #[serde(default)]
    pub seen_by: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgencyLeaks {
    #[serde(default = "default_fixture")]
    pub fixture: String,
    pub secrets: Vec<String>,
    pub secret_tokens: Vec<String>,
    pub intents: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Offscreen {
    #[serde(default = "default_fixture")]
    pub fixture: String,
    /// The consequence, seeded as a Setup Turn so the engine path is
    /// measured whatever the model schedules.
    pub event: TrapEvent,
    pub provoke: String,
    pub away: String,
    pub back: String,
    pub ask: String,
}

/// Play basics (QA, 2026-11-10): one campaign per run plays each step in
/// order. The pending item is committed as a Setup Turn before play, held by
/// the PC.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayBasics {
    #[serde(default = "default_fixture")]
    pub fixture: String,
    pub pending: Pending,
    pub steps: Vec<BasicStep>,
}

/// An Open Item: a fact on a `pending_` key, witnessed by the PC.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pending {
    pub subject: String,
    pub key: String,
    pub value: String,
    pub text: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BasicStep {
    pub kind: BasicKind,
    pub intent: String,
    /// For a move: the place the Intent names. The move counts as right
    /// only when the scene place after the Turn is this place.
    #[serde(default)]
    pub to: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BasicKind {
    /// The PC goes elsewhere: the scene place must change.
    Move,
    /// The world prevents it: the Narration must tell a failure.
    Impossible,
    /// An opposed contest with an NPC: the Turn must have a Roll.
    Contest,
    /// The PC accepts the pending item: it must close.
    Accept,
    /// The PC talks about the Open Item without settling it: the item must
    /// stay open (a negative step, review 2026-11-10).
    NoClose,
    /// The PC talks about another place without going there: the scene
    /// place must stay (a negative step, review 2026-11-10).
    NoMove,
    /// The PC heads for a place that is not an entity (ADR 0007): the
    /// Turn commits, the scene place stays, and the Narration must tell
    /// that the PC does not arrive.
    UnknownMove,
}

/// Open Items at scale (bug pass 3, 2026-11-11): the PC holds many Open
/// Items, near-duplicates among them, before play. Each step names the items
/// it may close; any other close is wrong.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenItems {
    #[serde(default = "default_fixture")]
    pub fixture: String,
    pub items: Vec<NamedPending>,
    pub steps: Vec<ItemStep>,
}

/// An Open Item with the id that steps use for it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamedPending {
    pub id: String,
    pub subject: String,
    pub key: String,
    pub value: String,
    pub text: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemStep {
    pub kind: ItemKind,
    pub intent: String,
    /// The ids of the items this step may close. Empty: none may close.
    #[serde(default)]
    pub closes: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    /// The PC accepts one item: one of `closes` must close.
    Accept,
    /// The PC refuses one item: one of `closes` must close.
    Refuse,
    /// The PC talks about an item without settling it: nothing closes.
    Keep,
    /// The Intent touches no item: nothing closes.
    Unrelated,
}

/// "Ask the GM" (bug pass 5, M2): the PC holds Open Items, some of them
/// already answered, and each question is asked in several phrasings. An
/// answer is right when it holds one word of each `all_of` list and none of
/// `none_of` (case does not matter).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ooc {
    #[serde(default = "default_fixture")]
    pub fixture: String,
    pub items: Vec<AnsweredPending>,
    pub questions: Vec<OocQuestion>,
}

/// An Open Item the PC holds, and, when `outcome` is set, its answer: a
/// fact on the outcome key, as a closing `settles` writes it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnsweredPending {
    pub subject: String,
    pub key: String,
    pub value: String,
    pub text: String,
    #[serde(default)]
    pub outcome: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OocQuestion {
    pub topic: String,
    pub phrasings: Vec<String>,
    pub all_of: Vec<Vec<String>>,
    #[serde(default)]
    pub none_of: Vec<String>,
}

/// What the PC finds (ADR 0007, 2026-11-12): lookup Intents that must
/// find something, and negative Intents that must find nothing. One
/// campaign per run plays the steps in order.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Finds {
    #[serde(default = "default_fixture")]
    pub fixture: String,
    pub steps: Vec<FindStep>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FindStep {
    pub kind: FindKind,
    pub intent: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindKind {
    /// The Intent reads, opens, looks through or asks for a list: the Turn
    /// must record what the PC finds, and the Narration must tell it.
    Lookup,
    /// A plain talk or a move: the Turn must find nothing.
    Negative,
}

/// Departures (bug pass 5, M1): the Player sends an NPC away, a World Event
/// makes another NPC leave, then the PC looks around. Each Narration is
/// judged against who is here after its Turn.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Departures {
    #[serde(default = "default_fixture")]
    pub fixture: String,
    pub event: DepartureEvent,
    pub steps: Vec<String>,
}

/// A World Event seeded before play whose text makes its actor leave. With
/// `moves_actor_to`, it carries the actor's `location` effect, as the World
/// Build writes it since bug pass 5; without, it has no effect, as the QA
/// world's event had.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DepartureEvent {
    pub actor: String,
    pub text: String,
    pub in_minutes: u64,
    #[serde(default)]
    pub seen_by: Vec<String>,
    /// A place name, or `away` (a place the PC cannot name).
    #[serde(default)]
    pub moves_actor_to: Option<String>,
}

/// A Seed Draft and a World Build from a Source Text, scored against a
/// hand-made list. A relative path starts at the repository root; the
/// command line can name files outside it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldBuild {
    pub source: String,
    pub expected: String,
}

/// A scenario of the mature content module (plan Task 9): the gates, the
/// tag, the Refusals, the scene's lifecycle and its timing. The repository's
/// files use neutral Intents and the level `implied` only; a folder outside
/// the repository (`--scenarios-dir`) may hold other files, and the report
/// then prints counts only.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mature {
    #[serde(default = "default_fixture")]
    pub fixture: String,
    /// The Detail Level the Campaign starts at: `implied` in the
    /// repository (`load_mature` refuses any other value there).
    pub detail: String,
    /// The characters the Player confirms in play (`Engine::confirm_age`)
    /// before the first Intent, by main name.
    #[serde(default)]
    pub confirm: Vec<String>,
    /// One Campaign each per run. `intimate_tag` has two: one with a minor
    /// at the PC's place, one without; the report counts them apart.
    pub arms: Vec<Arm>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Arm {
    /// A name for the scenario file; the report never prints it.
    pub name: String,
    /// Places added to the fixture's Adventure Seed (a private place for a
    /// willing partner, live evaluation of 2026-11-12).
    #[serde(default)]
    pub extra_places: Vec<String>,
    /// Where the PC starts, in place of the fixture's place: a location
    /// value, as a seed fact writes it.
    #[serde(default)]
    pub pc_at: Option<String>,
    /// The characters this arm confirms in play, in place of the
    /// scenario's `confirm`.
    #[serde(default)]
    pub confirm: Option<Vec<String>>,
    /// Characters added to the fixture's Adventure Seed before the Campaign
    /// is made, each at a place.
    #[serde(default)]
    pub extra_npcs: Vec<ExtraNpc>,
    pub steps: Vec<MatureStep>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtraNpc {
    pub name: String,
    pub age: u16,
    /// The location value, as a seed fact writes it.
    pub at: String,
    /// The NPC's attitude toward the PC, -5 to 5, written in the World
    /// Setup.
    #[serde(default)]
    pub attitude: Option<i8>,
    /// Facts about the NPC that the PC and the NPC both hold, written in
    /// the World Setup: neutral words only.
    #[serde(default)]
    pub facts: Vec<ExtraFact>,
}

/// A fact of an extra NPC (`ExtraNpc::facts`).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtraFact {
    pub key: String,
    pub value: String,
    pub text: String,
}

/// One step: an Intent with the tag the scenario expects, or "End scene".
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatureStep {
    #[serde(default)]
    pub intent: String,
    /// The scenario expects the model to tag this Intent `intimate`.
    #[serde(default)]
    pub intimate: bool,
    /// "End scene" (`Engine::end_scene`) in place of an Intent.
    #[serde(default)]
    pub end_scene: bool,
}

/// A mature scenario from `dir`. When `dir` lies inside the repository,
/// only `detail = "implied"` is accepted: the repository holds no scenario
/// at `explicit`, and none at `excluded`, which tests nothing.
pub fn load_mature(dir: &Path, inside_repo: bool, name: &str) -> Result<Mature, String> {
    let path = dir.join(format!("{name}.toml"));
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    // A TOML error quotes the file: outside the repository, only where.
    let sc: Mature = toml::from_str(&text).map_err(|e| {
        if inside_repo {
            format!("{}: {e}", path.display())
        } else {
            format!(
                "{}: not a valid scenario file (at byte {})",
                path.display(),
                e.span().map_or(0, |span| span.start)
            )
        }
    })?;
    if inside_repo && sc.detail != "implied" {
        return Err(format!(
            "{}: a scenario inside the repository runs at detail = \"implied\" only",
            path.display()
        ));
    }
    if sc.arms.is_empty() || sc.arms.iter().any(|a| a.steps.is_empty()) {
        return Err(format!("{}: every arm needs steps", path.display()));
    }
    let names: std::collections::BTreeSet<&str> = sc.arms.iter().map(|a| a.name.as_str()).collect();
    if names.len() != sc.arms.len() {
        return Err(format!("{}: two arms share a name", path.display()));
    }
    Ok(sc)
}

pub fn load<T: DeserializeOwned>(root: &Path, name: &str) -> Result<T, String> {
    let path = root.join("evals/scenarios").join(format!("{name}.toml"));
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}
