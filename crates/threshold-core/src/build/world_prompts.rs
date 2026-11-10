//! The prompts and schemas of the World Build stages (design 6.3). A prompt
//! never shows an open item without its marker, nor its `pending_` key.
//! Each schema field that names a roster entry is an `enum` of main names
//! from the approved roster.

use super::{
    SeedDraft, Stage,
    checks::{FactBook, Member, Roster},
    stages::{Need, StageCall, name_field, name_list, schema_of},
    world_stages::{EventsOut, HiddenOut, NpcCall, NpcOut, NpcState, StartOut},
};
use crate::{
    checks::PC_INVOLUNTARY_KEYS,
    domain::{EntityKind, FactEntry},
    model::Prompt,
};

const HIDDEN_SYSTEM: &str = "\
You extract the hidden side of a role-playing world from one section of a Source Text, as JSON. The Player never reads it.
Rules:
- Use only what this section states. Never invent a past event, a link between two entities, or a decision of the Player or of the player character (PC).
- Extract what the PC does not know: the private feelings and plans of other characters, hidden causes, what happened where the PC was not, who knows what. One fact per item.
- Never repeat a fact listed under WHAT THE PC KNOWS or HIDDEN FACTS ALREADY FOUND.
- subject: one name from the ROSTER, spelled as written there. key: a short English word in snake_case, such as role, debt, fear, plan. value: a few words. text: one sentence that states the fact, in the story language.
- known_by: the names of the characters from the ROSTER who know the fact. Never the PC and never another name of the PC. Empty when only the Game Master knows.
- open: true when the item is planned, proposed, promised or pending, so it has not happened. false when the text says that it happened or that it is true.
- Mature content becomes neutral facts. Do not remove it and do not refuse.";

const NPC_SYSTEM: &str = "\
You are the Game Master. You prepare the hidden side of one non-player character (NPC) before play starts, as JSON. The Player never reads it.
Rules:
- Stay consistent with the WORLD FACTS. Never contradict a fact. Never invent a past event. Never decide what the player character (PC) does, says, feels or wants.
- Give this NPC their own life, one that does not turn around the PC: their work, their family, their money, their fears, their other relations. Many NPCs care little about the PC. Name the PC in at most one goal or in the secret, and only when the WORLD FACTS give a reason.
- goals: one to three things this NPC wants now, each in one sentence. against: the ROSTER name of the character the goal works against, or null.
- secret: one thing this NPC hides, plausible and not melodramatic, as a fact. subject: a name from the ROSTER, often the NPC. key: a short English word in snake_case. value: a few words. text: one sentence. open: true when it is planned or pending, not done. It must be new: never restate a WORLD FACT. null when the NPC hides nothing.
- attitudes: how this NPC feels toward other characters or groups from the ROSTER, the PC included, from -5 (hostile) to 5 (devoted).
- voice_card: speech habits, typical words, and two to four short sample lines. Make this voice clearly different from every voice under VOICES ALREADY WRITTEN.
- Write every text in the story language.";

const EVENTS_SYSTEM: &str = "\
You are the Game Master. You schedule the World Events: what the non-player characters (NPCs) do offscreen after play starts, whether or not the player character (PC) is there. Answer as JSON.
Rules:
- Three to six events. Each follows from an open item, a goal or a secret given below.
- actor: the ROSTER name of one NPC. Never the PC, a place or a group.
- text: one short line about what the actor does, at most 80 characters. Never narrate a scene of the PC.
- effects: zero to two changes to the world, as data. subject: a name from the ROSTER. key: a short English word in snake_case. value: a few words.
- seen_by: at most two NPC names who witness the event. Never the PC, never the actor. Empty when nobody sees it.
- in_minutes: minutes after play starts, from 10 to 20160 (two weeks). Spread them: some within a day, some later.
- Never schedule what must wait for a choice of the Player. Never decide what the PC does.
- Write every text in the story language.";

const START_SYSTEM: &str = "\
You place the non-player characters (NPCs) who are not present when play starts, as JSON.
Rules:
- placements: for each NPC under ABSENT NPCS whose place the WORLD FACTS suggest, the NPC name and one name from PLACES, spelled as written there.
- Leave an NPC out when you do not know where they are. Never invent a reason.
- Never place the PC or an NPC listed as present.";

/// `base`, plus the rule on the keys of a fact about the PC (`checks::agency`).
fn with_pc_keys(base: &str) -> String {
    format!(
        "{base}\n- A fact or an effect whose subject is the PC may only use these keys: {}. Never a choice, a word, a feeling or a motive of the PC.",
        PC_INVOLUNTARY_KEYS.join(", ")
    )
}

/// The premise, the story language and the roster.
fn context(draft: &SeedDraft, roster: &Roster) -> String {
    let lines: Vec<String> = roster
        .members()
        .iter()
        .map(|m| {
            let mut line = format!("- {}: ", m.main);
            line.push_str(match m.kind {
                EntityKind::Pc => "the player character (PC)",
                EntityKind::Npc => "npc",
                EntityKind::Place => "place",
                EntityKind::Faction => "group",
                EntityKind::Item => "item",
            });
            if let Some(age) = m.age {
                line.push_str(&format!(", age {age}"));
            }
            if !m.others.is_empty() {
                line.push_str(&format!("; also called {}", m.others.join(", ")));
            }
            line
        })
        .collect();
    format!(
        "PREMISE\n{}\n\nSTORY LANGUAGE\n{}\n\nROSTER\n{}",
        draft.seed.premise,
        draft.seed.story_language,
        lines.join("\n")
    )
}

/// One line per fact. An open item always carries its marker, and never
/// its `pending_` key.
fn fact_lines<'a>(facts: impl Iterator<Item = &'a FactEntry>, holders: bool) -> String {
    let lines: Vec<String> = facts
        .map(|f| {
            let marker = if f.open { "(open, not done) " } else { "" };
            let mut line = format!(
                "- {} {} = {}: {marker}{}",
                f.subject, f.key, f.value, f.text
            );
            if holders {
                if f.known_by.is_empty() {
                    line.push_str(" [Game Master only]");
                } else {
                    line.push_str(&format!(" [known by {}]", f.known_by.join(", ")));
                }
            }
            line
        })
        .collect();
    if lines.is_empty() {
        "none".into()
    } else {
        lines.join("\n")
    }
}

fn open_details(draft: &SeedDraft) -> String {
    let lines: Vec<String> = draft
        .seed
        .open_details
        .iter()
        .map(|d| format!("- (open, not done) {}", d.text))
        .collect();
    if lines.is_empty() {
        "none".into()
    } else {
        lines.join("\n")
    }
}

/// The main names of the roster members that `keep` accepts, the PC first.
fn names(roster: &Roster, keep: impl Fn(&Member) -> bool) -> Vec<&str> {
    roster
        .members()
        .iter()
        .filter(|m| keep(m))
        .map(|m| m.main.as_str())
        .collect()
}

fn prompt(system: String, recent: String) -> Prompt {
    Prompt {
        system,
        recent,
        ..Prompt::default()
    }
}

pub(super) fn hidden_call(
    draft: &SeedDraft,
    roster: &Roster,
    book: &FactBook,
    (n, of): (usize, usize),
    part: &str,
) -> StageCall {
    let pc = &roster.pc().main;
    let (known, hidden): (Vec<&FactEntry>, Vec<&FactEntry>) =
        book.facts().partition(|f| f.known_by.contains(pc));
    // Only an NPC holds a hidden fact, so the PC is no holder by schema.
    let mut schema = schema_of::<HiddenOut>();
    let fact = "/$defs/FactOut/properties";
    name_field(
        &mut schema,
        &format!("{fact}/subject"),
        &names(roster, |_| true),
        false,
    );
    let npcs = names(roster, |m| m.kind == EntityKind::Npc);
    name_list(&mut schema, &format!("{fact}/known_by"), &npcs);
    StageCall {
        stage: Stage::HiddenFacts,
        step: format!("hidden_facts {n}/{of}"),
        prompt: prompt(
            with_pc_keys(HIDDEN_SYSTEM),
            format!(
                "{}\n\nWHAT THE PC KNOWS\n{}\n\nHIDDEN FACTS ALREADY FOUND\n{}\n\nSECTION {n} OF {of}\n{part}",
                context(draft, roster),
                fact_lines(known.into_iter(), false),
                fact_lines(hidden.into_iter(), true),
            ),
        ),
        schema,
        need: Need::Optional,
    }
}

pub(super) fn npc_call(
    draft: &SeedDraft,
    roster: &Roster,
    book: &FactBook,
    call: &NpcCall,
) -> StageCall {
    let voices: Vec<String> = call
        .others
        .iter()
        .map(|s| {
            let v = &s.voice_card;
            format!("- {}: {}; {}", v.npc, v.habits, v.vocabulary)
        })
        .collect();
    let npc = call.npc;
    let mut who = npc.main.clone();
    if let Some(age) = npc.age {
        who.push_str(&format!(", age {age}"));
    }
    let (n, of) = call.at;
    let mut recent = format!(
        "{}\n\nWORLD FACTS\n{}\n\nOPEN DETAILS\n{}\n\nVOICES ALREADY WRITTEN\n{}\n\nNPC TO PREPARE\n{who}",
        context(draft, roster),
        fact_lines(book.facts(), true),
        open_details(draft),
        if voices.is_empty() {
            "none".into()
        } else {
            voices.join("\n")
        },
    );
    // A goal works against another character or the PC. An attitude points
    // at another character, the PC or a group; never a place or an item.
    let against = names(roster, |m| {
        matches!(m.kind, EntityKind::Pc | EntityKind::Npc) && m.main != npc.main
    });
    let toward = names(roster, |m| {
        matches!(
            m.kind,
            EntityKind::Pc | EntityKind::Npc | EntityKind::Faction
        ) && m.main != npc.main
    });
    let mut schema = schema_of::<NpcOut>();
    name_field(
        &mut schema,
        "/$defs/GoalOut/properties/against",
        &against,
        true,
    );
    name_field(
        &mut schema,
        "/$defs/AttitudeOut/properties/to",
        &toward,
        false,
    );
    let all = names(roster, |_| true);
    name_field(
        &mut schema,
        "/$defs/SecretOut/properties/subject",
        &all,
        false,
    );
    let mut step = format!("npc_state {n}/{of}");
    let mut need = Need::Essential;
    if let Some(items) = call.autonomy {
        recent.push_str(&format!(
            "\n\nWORLD AUTONOMY\n- WorldAutonomy: in an earlier answer for this NPC, {} named the PC. \
             Too many goals and secrets in this world are about the PC. Write goals and a secret \
             about this NPC's own life, and name the PC in at most one of them.",
            items.join(", ")
        ));
        step.push_str(" autonomy");
        need = Need::Optional;
    }
    StageCall {
        stage: Stage::NpcState,
        step,
        prompt: prompt(with_pc_keys(NPC_SYSTEM), recent),
        schema,
        need,
    }
}

pub(super) fn events_call(
    draft: &SeedDraft,
    roster: &Roster,
    book: &FactBook,
    states: &[NpcState],
) -> StageCall {
    let goals: Vec<String> = states
        .iter()
        .flat_map(|s| &s.goals)
        .map(|g| format!("- {}: {}", g.npc, g.text))
        .collect();
    let npcs = names(roster, |m| m.kind == EntityKind::Npc);
    let mut schema = schema_of::<EventsOut>();
    name_field(
        &mut schema,
        "/$defs/EventOut/properties/actor",
        &npcs,
        false,
    );
    name_list(&mut schema, "/$defs/EventOut/properties/seen_by", &npcs);
    let all = names(roster, |_| true);
    name_field(
        &mut schema,
        "/$defs/EffectOut/properties/subject",
        &all,
        false,
    );
    StageCall {
        stage: Stage::WorldEvents,
        step: "world_events".into(),
        prompt: prompt(
            with_pc_keys(EVENTS_SYSTEM),
            format!(
                "{}\n\nWORLD FACTS\n{}\n\nOPEN DETAILS\n{}\n\nNPC GOALS\n{}",
                context(draft, roster),
                fact_lines(book.facts(), true),
                open_details(draft),
                goals.join("\n"),
            ),
        ),
        schema,
        need: Need::Optional,
    }
}

/// `places`: the places an absent NPC may be at, never the start place.
pub(super) fn start_call(
    draft: &SeedDraft,
    roster: &Roster,
    book: &FactBook,
    present: &[&str],
    absent: &[&Member],
    places: &[&str],
) -> StageCall {
    let list = |names: &[&str]| {
        if names.is_empty() {
            "none".into()
        } else {
            names
                .iter()
                .map(|n| format!("- {n}"))
                .collect::<Vec<_>>()
                .join("\n")
        }
    };
    let absent: Vec<&str> = absent.iter().map(|m| m.main.as_str()).collect();
    let mut schema = schema_of::<StartOut>();
    name_field(
        &mut schema,
        "/$defs/PlacementOut/properties/npc",
        &absent,
        false,
    );
    name_field(
        &mut schema,
        "/$defs/PlacementOut/properties/place",
        places,
        false,
    );
    StageCall {
        stage: Stage::StartState,
        step: "start_state".into(),
        prompt: prompt(
            START_SYSTEM.into(),
            format!(
                "{}\n\nSTART\nThe PC is at {}. Present there: {}.\n\nABSENT NPCS\n{}\n\nPLACES\n{}\n\nWORLD FACTS\n{}",
                context(draft, roster),
                roster.main(&draft.seed.start.place),
                if present.is_empty() {
                    "nobody".into()
                } else {
                    present.join(", ")
                },
                list(&absent),
                list(places),
                fact_lines(book.facts(), true),
            ),
        ),
        schema,
        need: Need::Essential,
    }
}
