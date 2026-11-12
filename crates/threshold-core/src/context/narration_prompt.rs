//! The Narration prompt and the narration filter (design §6.1 step 7).
//!
//! The narrator gets only what the PC can perceive: the PC's place, who the
//! PC knows is here (`seen_present`), the PC's own Beliefs about them, their
//! Voice Cards, Content Pack fragments,
//! the committed ops the PC perceived, the texts of the World Events the PC
//! witnessed fire this Turn (`world::with_witnessed_events`, filtered by
//! `guard_events`), and short NPC reaction intents (filtered by
//! `guard_plans`). It never gets a fact by id, a hidden fact, a key list, a
//! goal, or a World Event that is still scheduled or that the PC did not
//! witness. This is the real protection against leaks. The opening prompt
//! (Begin) gets the same blocks, with no Turn: the start's situation and
//! time take the place of the outcome.

use std::collections::HashSet;

use super::{
    NamesAudience, PC_KNOWS_MAX_CHARS, PC_KNOWS_STEP, Retrieval, adventure_block, bounded,
    fact_text, names_block, one_line, pack_block, render_voice, seen_present, turns_window,
    with_fragments,
};
use crate::{
    domain::{
        ActionKind, DetailLevel, EntityId, EntityKind, EntityRef, Finds, FoundItem, Handle,
        Knowing, LedgerOp, Reaction, Verdict, is_open_key, is_state_key, outcome_key,
    },
    mature::{Cleared, Told, effective},
    model::Prompt,
    normalize,
    pack::{ContentPack, Target, scene_fragments},
    store::WorldView,
    world::{hidden, name_parts, openly_here, pc_can_name, pc_knowledge},
};

/// The text of `INTIMATE_RULE`, as a literal, so `NARRATION_RULES` can
/// hold it with `concat!`.
macro_rules! intimate_rule_text {
    () => {
        "Tell intimate or sexual content only as far as a line headed INTIMATE TURN allows; with no such line, tell none."
    };
}
pub(crate) use intimate_rule_text;

/// The narrator's rules. The last three lines are `context::PACK_RULE`,
/// `INTIMATE_RULE` and `speech::RULE`, the speech mark.
pub const NARRATION_RULES: &str = concat!(
    "\
You are the narrator of a single-player tabletop role-playing game. Write what the player character (PC) perceives, in the second person, in the story language.
Narrate only the outcome given under THIS TURN. Never add a fact that changes the world, and never reveal what the PC cannot perceive.
Never decide the PC's actions, words, or feelings beyond the player's intent.
NPC intentions tell you how to portray an NPC. Show them through words and manner; never state them as fact.
Give each NPC the voice of their Voice Card.
Only the people the outcome lists as here are present. Nobody comes or goes unless the outcome says so.
Never say that more time passed than the time line of THIS TURN states. Do not open with the time; tell it only when it matters.
Write two short paragraphs at most. Plain prose, no headings, no lists.
",
    super::pack_rule_text!(),
    "\n",
    intimate_rule_text!(),
    "\n",
    crate::speech::rule_text!(),
    "\n"
);

/// The narrator's one stable line on intimate content (data shape 3.10),
/// in neutral words. It is part of `NARRATION_RULES`, placed before
/// `speech::RULE`, and never changes, so the cached prefix stays the same
/// on every Turn. The owner reads it before the merge.
pub const INTIMATE_RULE: &str = intimate_rule_text!();

/// The head of the narration's `recent` block when the Turn is told by
/// implication (`mature::Told::Implied`): the narrator passes over the
/// intimate moment without describing it. One line, in neutral words. The
/// owner reads it before the merge.
pub const IMPLIED_LINE: &str = "INTIMATE TURN (implied): tell only that the moment happens and how the characters are with each other, then pass over it without describing it. Add no act, word or feeling of the PC beyond the player intent.\n";

/// The head of the narration's `recent` block when the Turn is told at an
/// effective `explicit` level (`mature::Told::Explicit`, ADR 0009), in place
/// of `IMPLIED_LINE`: the narrator tells the moment as far as the Content
/// Pack lines that follow allow, and no further. It is headed INTIMATE TURN,
/// so `INTIMATE_RULE` reads it as the line that allows. One line, in
/// neutral words; it holds no description itself. The owner reads it
/// before the merge.
pub const EXPLICIT_LINE: &str = "INTIMATE TURN (explicit): tell this moment only as far as the Content Pack lines below allow, and no further. Add no act, word or feeling of the PC beyond the player intent.\n";

/// The head of the narration's `recent` block for a Closing Turn (data
/// shape 3.7 step 6): the Player ended the scene; the narrator tells briefly
/// how it ends, with no Intent to tell. One line, in neutral words. The
/// owner reads it before the merge.
pub const CLOSING_LINE: &str = "SCENE ENDS: the player chose to end the intimate scene. In a few plain sentences, tell how it closes and what the NPCs here do, then pass over the time. Tell nothing intimate, and no act, word or feeling of the PC.\n";

/// The lines that follow `NARRATION_RULES` in the opening prompt.
pub const OPENING_RULES: &str = "\
This is the opening of the adventure: there is no THIS TURN. Narrate the start given under OPENING.
Describe where the PC is and what the PC perceives as play begins.
Do not decide anything the PC does, says, or feels.
Do not move time forward.
End without asking what the PC does.
";

/// The ops of one Turn that the PC perceives, in their order. A fact or a
/// supersession counts when the PC is among its witnesses; a Claim when the
/// PC hears it now; a new entity when the PC knows it is here after the
/// Commit (`seen_present`); a Roll
/// and the passing of time always. Everything else (hidden facts, scheduled
/// events, goals, attitudes, knowledge rows, Voice Cards) is dropped.
pub fn perceived_by_pc(ops: &[LedgerOp], view_after: &WorldView) -> Vec<LedgerOp> {
    let Some(pc) = view_after
        .entities
        .iter()
        .find(|e| e.kind == EntityKind::Pc)
        .map(|e| e.id)
    else {
        return Vec::new();
    };
    let pc_ref = EntityRef::Id(pc);
    let present = seen_present(view_after);
    ops.iter()
        .filter(|op| match op {
            LedgerOp::AddFact { seen_by, .. } => seen_by.contains(&pc_ref),
            LedgerOp::SupersedeFact { new, .. } => new.seen_by.contains(&pc_ref),
            LedgerOp::AddClaim {
                listeners,
                delay_min,
                ..
            } => *delay_min == 0 && listeners.contains(&pc_ref),
            LedgerOp::AddEntity { kind, name, .. } => {
                *kind != EntityKind::Pc
                    && view_after
                        .entities_by_name(name)
                        .any(|e| present.contains(&e.id))
            }
            LedgerOp::RecordRoll { .. } | LedgerOp::AdvanceClock { .. } => true,
            _ => false,
        })
        .cloned()
        .collect()
}

/// The reactions as the narrator may see them. A plan is model text and can
/// quote a GM-only fact, so a plan that names a secret is dropped; the NPC
/// and the stance stay.
///
/// A secret is a live fact on a key that is not a state key (or that is an
/// open key), with no PC
/// Belief on the same slot and value. A plan names it when the fact's key
/// or value appears in the plan as whole words, after normalization
/// (case, articles, `_` and punctuation count as word breaks). A key or
/// value that is an entity name is not a secret word: names are public.
/// A plan that names an entity the PC must not hear of (`names_unheard`:
/// a hidden NPC by a part of its name, a place the PC cannot name) is
/// dropped too, as an event text is (review of bug pass 4, M1).
pub fn guard_plans(view: &WorldView, reactions: &[Reaction]) -> Vec<Reaction> {
    let secrets = secret_terms(view);
    let unheard = unheard_names(view);
    reactions
        .iter()
        .map(|r| {
            let mut r = r.clone();
            if quotes(&secrets, &r.plan) || names_any(&unheard, &r.plan) {
                r.plan.clear();
            }
            r
        })
        .collect()
}

/// The texts of witnessed World Events as the narrator may see them. An
/// event text is model text, like a plan, and it is the visible part of the
/// event: it is told unless it names something the PC must not learn,
/// whether the event is public or not. A text that holds a secret value
/// (`secret_values`, the same whole-word match as `guard_plans`) is
/// dropped; a secret key alone does not drop it ("Ludo counts the silver
/// for his debt" is seen). So is a text that names an entity the PC must
/// not hear of (`names_unheard`).
pub fn guard_events(view: &WorldView, events: &[String]) -> Vec<String> {
    let secrets = secret_values(view);
    let unheard = unheard_names(view);
    events
        .iter()
        .filter(|text| !quotes(&secrets, text) && !names_any(&unheard, text))
        .cloned()
        .collect()
}

/// What the PC found in a Turn, as the Turn records it (ADR 0007,
/// 2026-11-12). Each item is model text, like an event text, so it gets a
/// guard: an item whose title or line names an entity the PC must not hear
/// of (`names_unheard`) or holds a secret value of at least
/// `FOUND_SECRET_MIN_WORDS` content words (`found_secret_values`) goes. A
/// source that does either drops everything, and so does a result with no
/// item left: `None`. The second value holds one note per item that went,
/// with its place in the list and never its text. Only removes model
/// output.
///
/// The value rule is narrower than the one of `guard_events` (live run of
/// b245f71): a found notice says "the magistrate" or "the clerk" without
/// stating a secret, and a one-word value such as a role is a common word.
pub fn guard_found(view: &WorldView, finds: &Finds) -> (Option<Finds>, Vec<String>) {
    if !finds.found() {
        return (None, Vec::new());
    }
    let secrets = found_secret_values(view);
    let unheard = unheard_names(view);
    let guarded = |text: &str| quotes(&secrets, text) || names_any(&unheard, text);
    if guarded(&finds.source) {
        return (
            None,
            vec!["removed finds: its source names what the PC must not hear of".into()],
        );
    }
    let mut notes = Vec::new();
    let items: Vec<FoundItem> = finds
        .items
        .iter()
        .enumerate()
        .filter(|(at, item)| {
            let hidden = guarded(&item.title) || guarded(&item.line);
            if hidden {
                notes.push(format!(
                    "removed finds item {}: it names what the PC must not hear of",
                    at + 1
                ));
            }
            !hidden
        })
        .map(|(_, item)| item.clone())
        .collect();
    if items.is_empty() {
        return (None, notes);
    }
    (
        Some(Finds {
            from: finds.from,
            source: finds.source.clone(),
            items,
        }),
        notes,
    )
}

/// The start of the outcome lines that give the narrator what the PC found
/// (ADR 0007, 2026-11-12). The source follows, then one line per item.
pub const FOUND_LINE: &str = "- What the PC finds (";

/// What the PC found this Turn, as outcome lines of THIS TURN: the source,
/// then one line per item, title in quotes. The narrator tells them; the
/// screens show them too, so a brief telling is enough.
pub fn found_lines(finds: &Finds) -> String {
    let mut out = format!(
        "{FOUND_LINE}{}). Tell what it shows, briefly; add no other entry:\n",
        one_line(&finds.source)
    );
    for item in &finds.items {
        out.push_str(&format!("  - {}\n", item_text(item)));
    }
    out
}

/// One found item on one line: `"<title>": <line>`, or the title alone.
pub(crate) fn item_text(item: &FoundItem) -> String {
    let line = one_line(&item.line);
    if line.is_empty() {
        format!("\"{}\"", one_line(&item.title))
    } else {
        format!("\"{}\": {line}", one_line(&item.title))
    }
}

/// The line the narrator gets when a World Event stopped the clock and
/// every part of it is guarded away: its text dropped, and no effect the PC
/// learned. The outcome is then never empty.
pub const QUIET_EVENT_LINE: &str = "Nothing the PC notices happens.";

/// True when `text` names an entity the PC must not hear of: an NPC the
/// Adventure Seed hides, or one that neither stands openly here
/// (`world::openly_here`) nor is known to the PC (seen here, or the subject
/// of a PC Belief); or a place, a Faction or a thing the PC cannot name
/// (`world::pc_can_name`, review B1: "Orsel slips down to the Smugglers
/// Cellar"). `guard_events` and `guard_plans` drop such a text, and a
/// public event's effect whose fact text or value names such an entity is
/// not learned (`world::with_witnessed_events`).
///
/// A text names an entity by one of its name parts (`world::name_parts`,
/// the rule of a speech tag: the main name, its leading words for a
/// character, every other name), as whole words with the name's own
/// capitals (`normalize::name_in`): "Brann whispers to Maren" names the
/// hidden Maren Voss, "a rose" does not name Rose Hale (review of bug pass
/// 4, M1). A part that is also a name part of an entity the PC may hear of
/// is left out, so a hidden "Orsel Voss" does not guard the visible Orsel.
pub fn names_unheard(view: &WorldView, text: &str) -> bool {
    names_any(&unheard_names(view), text)
}

/// The name parts of every entity the PC must not hear of, cased
/// (`normalize::name_pattern`).
fn unheard_names(view: &WorldView) -> Vec<Vec<String>> {
    let present = seen_present(view);
    let known: HashSet<EntityId> = pc_knowledge(view).into_iter().map(|b| b.subject).collect();
    let named = pc_can_name(view);
    let (unheard, heard): (Vec<&crate::store::Entity>, Vec<&crate::store::Entity>) =
        view.entities.iter().partition(|e| match e.kind {
            EntityKind::Npc => {
                hidden(view, e.id)
                    || !(openly_here(view, e.id)
                        || present.contains(&e.id)
                        || known.contains(&e.id))
            }
            EntityKind::Place | EntityKind::Faction | EntityKind::Item => !named.contains(&e.id),
            EntityKind::Pc => false,
        });
    let lower =
        |part: &[String]| -> Vec<String> { part.iter().map(|w| w.to_lowercase()).collect() };
    let public: HashSet<Vec<String>> = heard
        .into_iter()
        .flat_map(name_parts)
        .map(|part| lower(&part))
        .collect();
    unheard
        .into_iter()
        .flat_map(name_parts)
        .filter(|part| !part.is_empty() && !public.contains(&lower(part)))
        .collect()
}

/// True when `text` names one of `parts` (`normalize::name_in`).
fn names_any(parts: &[Vec<String>], text: &str) -> bool {
    let text = normalize::cased_words(text);
    parts.iter().any(|part| normalize::name_in(&text, part))
}

/// The fewest content words a secret value needs before a found item that
/// states it is dropped (`guard_found`).
pub const FOUND_SECRET_MIN_WORDS: usize = 2;

/// Words that carry no content of their own: a value's content words are
/// its other words.
const FUNCTION_WORDS: &[&str] = &[
    "a", "about", "after", "all", "an", "and", "any", "are", "as", "at", "be", "been", "before",
    "but", "by", "for", "from", "her", "his", "in", "into", "is", "it", "its", "my", "no", "nor",
    "not", "of", "off", "on", "onto", "or", "our", "out", "over", "so", "some", "than", "that",
    "the", "their", "them", "then", "there", "these", "they", "this", "those", "to", "under", "up",
    "upon", "very", "was", "were", "with", "yet", "your",
];

/// The secret values (`secret_values`) that have at least
/// `FOUND_SECRET_MIN_WORDS` content words: what a found item must not
/// state.
fn found_secret_values(view: &WorldView) -> Vec<Vec<String>> {
    secret_values(view)
        .into_iter()
        .filter(|term| {
            term.iter()
                .filter(|w| !FUNCTION_WORDS.contains(&w.as_str()))
                .count()
                >= FOUND_SECRET_MIN_WORDS
        })
        .collect()
}

/// The values of `secret_facts`: what an event text must not state.
fn secret_values(view: &WorldView) -> Vec<Vec<String>> {
    let names: HashSet<Vec<String>> = view.entities.iter().map(|e| words(&e.name)).collect();
    secret_facts(view)
        .map(|f| words(&f.slot.value))
        .filter(|term| !term.is_empty() && !names.contains(term))
        .collect()
}

/// Every live fact the PC holds no Belief about (same slot, same value), on
/// a key that is not a state key or that is an open key. An open key is a
/// state key only so that its fact stays replaceable; a plan the PC does
/// not know stays a secret.
fn secret_facts(view: &WorldView) -> impl Iterator<Item = &crate::store::Fact> {
    let known: HashSet<(EntityId, String, String)> = pc_knowledge(view)
        .into_iter()
        .map(|b| (b.subject, b.key, normalize::value(&b.value)))
        .collect();
    view.live_facts
        .iter()
        .filter(|f| !is_state_key(&f.slot.key) || is_open_key(&f.slot.key))
        .filter(move |f| {
            !known.contains(&(
                f.slot.subject,
                f.slot.key.clone(),
                normalize::value(&f.slot.value),
            ))
        })
}

/// The words of every secret fact (`secret_facts`): its key (an open key
/// without its prefix, `outcome_key`) and its value. Entity names are
/// public, so a key or value that is an entity name is left out.
fn secret_terms(view: &WorldView) -> Vec<Vec<String>> {
    let names: HashSet<Vec<String>> = view.entities.iter().map(|e| words(&e.name)).collect();
    secret_facts(view)
        .flat_map(|f| {
            // An open key names its outcome; the bare prefix names nothing.
            let key = &f.slot.key;
            let term = if is_open_key(key) {
                outcome_key(key).unwrap_or_default()
            } else {
                key
            };
            [words(term), words(&f.slot.value)]
        })
        .filter(|term| !term.is_empty() && !names.contains(term))
        .collect()
}

/// True when `text` contains one of `terms` as whole words.
fn quotes(terms: &[Vec<String>], text: &str) -> bool {
    let text = words(text);
    terms.iter().any(|term| {
        text.windows(term.len())
            .any(|window| window == term.as_slice())
    })
}

/// The words of a text after normalization: lowercase, article dropped, and
/// split on everything that is not a letter or a digit.
fn words(text: &str) -> Vec<String> {
    let text = text.replace('_', " ");
    normalize::value(&text)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub fn narration_prompt(
    view_after: &WorldView,
    packs: &[ContentPack],
    perceived: &[LedgerOp],
    events: &[String],
    reactions: &[Reaction],
    intent: &str,
    turn: Option<(ActionKind, Verdict)>,
    told: &Told,
) -> Prompt {
    narration_prompt_with(
        view_after,
        packs,
        perceived,
        events,
        reactions,
        intent,
        turn,
        &[],
        told,
        None,
        false,
        effective(view_after.detail, packs),
    )
}

/// `narration_prompt` with outcome lines the engine adds after the
/// perceived ops: a wait cut short (`turn::wait_cut_line`, bug pass 5).
/// `closing` tells a Closing Turn: `CLOSING_LINE` opens `recent`, and an
/// empty `intent` gives no `Player intent` line.
/// `told` is the telling `mature::told` chose (G4): `Implied` opens the
/// `recent` block with `IMPLIED_LINE`, then, inside a cleared scene, the
/// always-on narration scene fragments of `packs`; `Explicit` opens it with
/// `EXPLICIT_LINE` and every narration scene fragment of `packs`, the
/// explicit-only ones included, and no `IMPLIED_LINE` (data shape 3.10).
/// `cleared` is the clearance of the open scene
/// (`mature::narration_clearance`): with `Some`, the earlier Turns of that
/// scene are shown in full; with `None`, each Turn committed with
/// `intimate` is its Neutral Line (`mature::turn_text`). The rules, stable
/// and scene blocks depend on neither. `level` is the effective Detail
/// Level the call reads past Turns at (`mature::reading_level`): an
/// intimate Turn told above it is its Neutral Line (final review M3).
#[allow(clippy::too_many_arguments)]
pub fn narration_prompt_with(
    view_after: &WorldView,
    packs: &[ContentPack],
    perceived: &[LedgerOp],
    events: &[String],
    reactions: &[Reaction],
    intent: &str,
    turn: Option<(ActionKind, Verdict)>,
    extra: &[String],
    told: &Told,
    cleared: Option<&Cleared>,
    closing: bool,
    level: DetailLevel,
) -> Prompt {
    let r = Retrieval::new(view_after, "", cleared, level);
    let head = match told {
        Told::Plain if closing => CLOSING_LINE.to_owned(),
        Told::Plain => String::new(),
        Told::Implied => {
            let fragments = cleared.map_or_else(Vec::new, |cleared| {
                scene_fragments(packs, Target::Narration, cleared, false)
            });
            format!("{IMPLIED_LINE}{}", pack_block(&fragments))
        }
        Told::Explicit(cleared) => {
            let fragments = scene_fragments(packs, Target::Narration, cleared, true);
            format!("{EXPLICIT_LINE}{}", pack_block(&fragments))
        }
    };
    Prompt {
        system: with_fragments(NARRATION_RULES, packs, Target::Narration),
        stable: format!(
            "{}{}",
            adventure_block(view_after, None),
            names_block(view_after, NamesAudience::Narrator)
        ),
        scene: scene(&r),
        recent: format!(
            "{head}{}",
            recent(
                &r, perceived, events, reactions, intent, turn, extra, cleared, level
            )
        ),
    }
}

/// The outcome line of an impossible attempt (changed after QA,
/// 2026-11-10). It carries the verdict only: the blockers can be GM-only
/// facts, so their text never reaches the narrator.
pub const IMPOSSIBLE_LINE: &str = "- The attempt does not happen: the world prevents it. Nothing changes beyond what is listed here.\n";

/// The outcome line that says where the PC is after the Turn, from the
/// Commit (ADR 0007). The place's name follows.
pub const PC_PLACE_LINE: &str = "- After this Turn the PC is at ";

/// The outcome line of a move that the Commit did not make: the PC does not
/// arrive (ADR 0007). The place's name follows.
pub const NOT_MOVED_LINE: &str =
    "- The move does not happen: the PC does not arrive. After this Turn the PC is still at ";

/// The words after a lost Check's result line.
pub const FAILED_CHECK_LINE: &str =
    " Tell the attempt and how it fails. Nothing changes beyond what is listed here.";

/// The words after a Check's result line when the Check named no stake
/// (bug pass 5): the result is about what the player intended.
pub const NO_STAKE_LINE: &str = " The result is about what the player intent tries to do.";

/// The words after a won Check's result line.
pub const WON_CHECK_LINE: &str = " Tell the attempt and how it succeeds.";

/// The line that gives the narrator the Turn's game time, after the
/// outcome and never as an outcome item: as the first item, the narrator
/// opened most Narrations with it (bug pass 3). The minutes follow.
pub const TIME_LINE: &str = "Time this Turn takes: ";

/// The opening prompt (data shape 3.7): the narration rules with the
/// opening lines after them, the same stable and scene blocks as a
/// Narration, then the start's situation and time. It shows no Turn and no
/// outcome: Begin commits nothing for the PC. An empty situation or time
/// leaves its line out.
pub fn opening_prompt(view: &WorldView, packs: &[ContentPack]) -> Prompt {
    let r = Retrieval::new(view, "", None, DetailLevel::Excluded);
    let mut recent = String::from("OPENING\n");
    if let Some(start) = view.adventure_seed.as_ref().map(|seed| &seed.start) {
        for (label, text) in [("Situation", &start.situation), ("When", &start.when)] {
            let text = one_line(text);
            if !text.is_empty() {
                recent.push_str(&format!("{label}: {text}\n"));
            }
        }
    }
    Prompt {
        system: with_fragments(
            &format!("{NARRATION_RULES}{OPENING_RULES}"),
            packs,
            Target::Narration,
        ),
        stable: format!(
            "{}{}",
            adventure_block(view, None),
            names_block(view, NamesAudience::Narrator)
        ),
        scene: scene(&r),
        recent,
    }
}

fn scene(r: &Retrieval<'_>) -> String {
    let mut out = String::from("SCENE\n");
    out.push_str(&format!(
        "Place: {}\n",
        r.location.as_deref().unwrap_or("unknown")
    ));
    // Only who the PC knows is here (`seen_present`), never world truth: a
    // hidden watcher must not reach the narrator.
    let others: Vec<String> = r
        .seen_present
        .iter()
        .filter(|id| Some(**id) != r.pc)
        .map(|id| r.name(*id))
        .collect();
    if !others.is_empty() {
        out.push_str(&format!("Present: {}\n", others.join(", ")));
    }
    let voices = r.voice_cards(&r.seen_present);
    if !voices.is_empty() {
        out.push_str("VOICES\n");
        for card in voices {
            out.push_str(&render_voice(&r.name(card.npc), card));
        }
    }
    let mut shown: Vec<EntityId> = r.seen_present.clone();
    shown.extend(r.place);
    let known: Vec<String> = pc_knowledge(r.view)
        .into_iter()
        .filter(|b| shown.contains(&b.subject))
        .map(|b| match (b.how, b.source) {
            (Knowing::Told, Some(speaker)) if Some(speaker) != r.pc => {
                format!(
                    "- {} (told by {})\n",
                    fact_text(&b.key, &b.text),
                    r.name(speaker)
                )
            }
            _ => format!("- {}\n", fact_text(&b.key, &b.text)),
        })
        .collect();
    if !known.is_empty() {
        out.push_str("WHAT THE PC KNOWS HERE\n");
        out.push_str(&bounded(&known, PC_KNOWS_MAX_CHARS, PC_KNOWS_STEP).concat());
    }
    out
}

fn entity_name(r: &Retrieval<'_>, entity: &EntityRef, perceived: &[LedgerOp]) -> String {
    match entity {
        EntityRef::Id(id) => r.name(*id),
        EntityRef::New(local) => new_name(*local, perceived),
    }
}

fn new_name(local: u8, perceived: &[LedgerOp]) -> String {
    perceived
        .iter()
        .find_map(|op| match op {
            LedgerOp::AddEntity {
                local: Some(l),
                name,
                ..
            } if *l == local => Some(name.clone()),
            _ => None,
        })
        .unwrap_or_else(|| "a newcomer".into())
}

/// The outcome line of an NPC who leaves the PC's place (bug pass 5, M1):
/// "- Ludo Brenn leaves." or "- Ludo Brenn leaves for the stone quay.",
/// built from the slot, never from the fact's text. `None` for any other
/// write.
fn leave_line(
    r: &Retrieval<'_>,
    slot: &crate::domain::Slot,
    perceived: &[LedgerOp],
) -> Option<String> {
    if normalize::key(&slot.key) != "location"
        || r.pc.is_some_and(|pc| slot.subject == EntityRef::Id(pc))
    {
        return None;
    }
    let to = normalize::value(&slot.value);
    if r.location.as_deref().map(normalize::value) == Some(to.clone()) {
        return None;
    }
    let name = entity_name(r, &slot.subject, perceived);
    if to.is_empty() {
        return Some(format!("- {name} leaves.\n"));
    }
    let place = r
        .view
        .entities_by_name(&slot.value)
        .find(|e| e.kind == EntityKind::Place)
        .map_or_else(|| one_line(&slot.value), |e| e.name.clone());
    Some(format!("- {name} leaves for {place}.\n"))
}

/// Who the PC knows is here after the Turn, besides the PC, and that nobody
/// else comes or goes (bug pass 5, M1: the narrator sent NPCs out of the
/// room with no trace in the world).
pub const HERE_AFTER_LINE: &str = "- Here after this Turn besides the PC: ";

/// `HERE_AFTER_LINE` when nobody else is here.
pub const NOBODY_HERE_LINE: &str = "- Nobody else is here after this Turn.";

fn here_line(r: &Retrieval<'_>) -> String {
    let others: Vec<String> = r
        .seen_present
        .iter()
        .filter(|id| Some(**id) != r.pc)
        .map(|id| r.name(*id))
        .collect();
    if others.is_empty() {
        format!("{NOBODY_HERE_LINE}\n")
    } else {
        format!(
            "{HERE_AFTER_LINE}{}. Nobody else comes or goes.\n",
            others.join(", ")
        )
    }
}

fn outcome_line(r: &Retrieval<'_>, op: &LedgerOp, perceived: &[LedgerOp]) -> Option<String> {
    let slot = match op {
        LedgerOp::AddFact { slot, .. } => Some(slot),
        LedgerOp::SupersedeFact { new, .. } => Some(&new.slot),
        _ => None,
    };
    if let Some(line) = slot.and_then(|slot| leave_line(r, slot, perceived)) {
        return Some(line);
    }
    Some(match op {
        LedgerOp::AddFact { slot, text, .. } => format!("- {}\n", fact_text(&slot.key, text)),
        LedgerOp::SupersedeFact { new, .. } => {
            format!("- {}\n", fact_text(&new.slot.key, &new.text))
        }
        // The PC's own Claim is the outcome of a `settles` (review M2): an
        // outcome line, never words the PC says.
        LedgerOp::AddClaim { claim, .. }
            if r.pc.is_some_and(|pc| claim.speaker == EntityRef::Id(pc)) =>
        {
            format!("- {}\n", fact_text(&claim.slot.key, &claim.text))
        }
        LedgerOp::AddClaim { claim, .. } => format!(
            "- {} says: {}\n",
            entity_name(r, &claim.speaker, perceived),
            fact_text(&claim.slot.key, &claim.text)
        ),
        LedgerOp::AddEntity { name, .. } => format!("- {} is here.\n", one_line(name)),
        LedgerOp::RecordRoll {
            outcome,
            against,
            stakes,
            ..
        } => {
            // The opponent is named only when the PC sees them here.
            let against = against
                .as_ref()
                .filter(|a| match a {
                    EntityRef::Id(id) => r.seen_present.contains(id),
                    EntityRef::New(_) => true,
                })
                .map(|a| format!(" against {}", entity_name(r, a, perceived)))
                .unwrap_or_default();
            // QA (bug pass 5): a won Roll with no stake that the PC
            // perceives was told as nothing at all. The line says what the
            // result is about: the player's intent.
            let named = ["win", "lose"]
                .iter()
                .any(|k| stakes.get(k).is_some_and(|v| !v.is_null()));
            let about = if named { "" } else { NO_STAKE_LINE };
            if outcome == "lose" {
                format!("- Check{against}: the PC loses.{about}{FAILED_CHECK_LINE}\n")
            } else {
                format!("- Check{against}: the PC wins.{about}{WON_CHECK_LINE}\n")
            }
        }
        _ => return None,
    })
}

/// Where the PC is after the Turn, from the Commit (`view_after`). A move
/// whose Commit wrote no PC `location` did not happen, unless the Turn keeps
/// found content (`found`): then the act happened inside the place ("I walk
/// over to the wall and read the notices"), and the line only says where
/// the PC is (live run of b245f71).
fn place_line(
    r: &Retrieval<'_>,
    perceived: &[LedgerOp],
    action: Option<ActionKind>,
    found: bool,
) -> Option<String> {
    let pc = EntityRef::Id(r.pc?);
    let place = r
        .place
        .map_or_else(|| r.location.clone(), |id| Some(r.name(id)))?;
    let moved = perceived.iter().any(|op| {
        let slot = match op {
            LedgerOp::AddFact { slot, .. } => slot,
            LedgerOp::SupersedeFact { new, .. } => &new.slot,
            _ => return false,
        };
        slot.subject == pc && normalize::key(&slot.key) == "location"
    });
    let line = if action == Some(ActionKind::Move) && !moved && !found {
        NOT_MOVED_LINE
    } else {
        PC_PLACE_LINE
    };
    Some(format!("{line}{}.\n", one_line(&place)))
}

#[allow(clippy::too_many_arguments)]
fn recent(
    r: &Retrieval<'_>,
    perceived: &[LedgerOp],
    events: &[String],
    reactions: &[Reaction],
    intent: &str,
    turn: Option<(ActionKind, Verdict)>,
    extra: &[String],
    cleared: Option<&Cleared>,
    level: DetailLevel,
) -> String {
    // A Closing Turn has no Intent, so no Player intent line.
    let mut tail = String::from("THIS TURN\n");
    let intent = one_line(intent);
    if !intent.is_empty() {
        tail.push_str(&format!("Player intent: {intent}\n"));
    }
    let verdict = turn.map(|(_, verdict)| verdict);
    let impossible = (verdict == Some(Verdict::Impossible)).then(|| IMPOSSIBLE_LINE.to_owned());
    let mut outcome: Vec<String> = impossible
        .into_iter()
        .chain(
            perceived
                .iter()
                .filter_map(|op| outcome_line(r, op, perceived)),
        )
        .chain(events.iter().map(|text| format!("- {}\n", one_line(text))))
        .chain(extra.iter().cloned())
        .collect();
    let found = extra.iter().any(|line| line.starts_with(FOUND_LINE));
    if let Some(line) = place_line(r, perceived, turn.map(|(action, _)| action), found) {
        outcome.push(line);
    }
    outcome.push(here_line(r));
    if !outcome.is_empty() {
        tail.push_str("Outcome:\n");
        tail.push_str(&outcome.concat());
    }
    let minutes: u32 = perceived
        .iter()
        .filter_map(|op| match op {
            LedgerOp::AdvanceClock { minutes } => Some(*minutes),
            _ => None,
        })
        .sum();
    tail.push_str(&format!("{TIME_LINE}{minutes} minutes.\n"));
    if !reactions.is_empty() {
        tail.push_str("NPC intentions (portray, do not state):\n");
        for reaction in reactions {
            let name = match reaction.npc {
                Handle::Entity(id) => r.name(id),
                Handle::NewNpc(n) => {
                    u8::try_from(n).map_or_else(|_| "a newcomer".into(), |n| new_name(n, perceived))
                }
                _ => continue,
            };
            let stance = serde_json::to_value(reaction.stance)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default();
            let plan = one_line(&reaction.plan);
            if plan.is_empty() {
                tail.push_str(&format!("- {name} ({stance})\n"));
            } else {
                tail.push_str(&format!("- {name} ({stance}): {plan}\n"));
            }
        }
    }
    // The Turn being narrated is the last of the view: THIS TURN tells it,
    // so the window stops before it (bug pass 5, M4).
    format!(
        "{}{tail}",
        turns_window(r.view, r.view.turn_seq, cleared, level)
    )
}
