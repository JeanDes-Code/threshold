//! The Decision Proposal prompt. The GM model sees every fact it retrieves,
//! hidden ones included, marked as such.

use super::{
    GM_FACTS_MAX_CHARS, NamesAudience, Retrieval, adventure_block, fact_text, names_block,
    one_line, pack_block, render_voice, turns_window, with_fragments,
};
use crate::{
    HandleContext,
    domain::{Canon, DetailLevel, EntityId, EntityKind, Handle, Trigger, is_open_key},
    mature::{Cleared, effective},
    model::Prompt,
    pack::{ContentPack, Target, scene_fragments},
    store::{Entity, Fact},
    world::{items_named_by, known_places, open_item_ref, pc_item_text},
};

/// The text of `INTIMACY_FIELD_LINE`, as a literal, so `PROPOSAL_RULES` can
/// hold it with `concat!`.
macro_rules! intimacy_field_text {
    () => {
        concat!("- intimacy: ", crate::domain::intimacy_definition_text!())
    };
}

/// The one line that defines the required `intimacy` field, in neutral
/// words: `domain::INTIMACY_DEFINITION` after the field's name (ADR 0008).
/// Part of `PROPOSAL_RULES`. The owner reads it before the merge.
pub const INTIMACY_FIELD_LINE: &str = intimacy_field_text!();

/// The text of `WILLING_FIELD_LINE`, as a literal, so `PROPOSAL_RULES` can
/// hold it with `concat!`.
macro_rules! willing_field_text {
    () => {
        concat!("- willing: ", crate::domain::willing_definition_text!())
    };
}

/// The one line that defines a reaction's required `willing` field:
/// `domain::WILLING_DEFINITION` after the field's name (ADR 0008, changed
/// after the live evaluation of 2026-11-12). Part of `PROPOSAL_RULES`,
/// under LATER ACTIONS. The owner reads it before the merge.
pub const WILLING_FIELD_LINE: &str = willing_field_text!();

/// The text of `FINDS_FIELD_LINE`, as a literal, so `PROPOSAL_RULES` can
/// hold it with `concat!`.
macro_rules! finds_field_text {
    () => {
        concat!("- finds: ", crate::domain::finds_definition_text!())
    };
}

/// The one line that defines the required `finds` field:
/// `domain::FINDS_DEFINITION` after the field's name (ADR 0007,
/// 2026-11-12). Part of `PROPOSAL_RULES`, under LATER ACTIONS.
pub const FINDS_FIELD_LINE: &str = finds_field_text!();

/// The head of the proposal prompt's `recent` block inside a cleared
/// Intimate Scene (data shape 3.10): one line, in neutral words. It reaches
/// a prompt only with a `Cleared` value (`proposal_context`). It names the
/// longer plan limit, `domain::SCENE_PLAN_MAX_CHARS`. The owner reads it
/// before the merge.
pub const SCENE_LINE: &str = "INTIMATE SCENE (open): the Turns of this scene below are shown in full. Each NPC takes part only as far as their own stance and plan go; a reaction plan holds 1 to 200 characters. Decide nothing the PC does, says or feels.\n";

/// The line that takes the place of the Intent in the proposal prompt of a
/// Closing Turn (mature content data shape 3.7 step 1): the Player ends the
/// scene. One line, in neutral words. It gives the PC no act, word or
/// feeling. The owner reads it before the merge.
pub const CLOSING_ASK: &str = "SCENE ENDS: the player ends the intimate scene here, as a choice outside the story. Propose how each NPC here takes it (at most two reactions) and how many minutes pass until the scene is over. The PC does, says and feels nothing new.\n";

/// The rules part of the system prompt. Its first lines are the wording
/// measured by the live probe (`threshold-ollama/examples/probe.rs`); the
/// VERDICT and LATER ACTIONS parts answer two failures that probe found: a
/// Check on every proposal, and no scheduled event for a delayed action.
/// The `move_to`, `settles`, `contested` and `intimacy` lines describe
/// required fields (ADR 0007, ADR 0008), one line each. `context::PACK_RULE`
/// says that a Content Pack line is a style note only (review M1).
pub const PROPOSAL_RULES: &str = concat!(
    "\
You are the Game Master for a single-player tabletop role-playing game. You decide what the world and its NPCs do; the player decides what the player character (PC) does.
Never decide the player character's actions, words, or feelings.
Return only one-line minified JSON with no spaces or newlines, matching the supplied schema. Omit optional fields that are empty.
Reference entities, facts, events and goals only by their handles (E*, F*, W*, G*). A new NPC uses N1.
Stake values and reaction plans must be short snake_case or at most 6 words.
Facts marked hidden are known to the GM only: they may drive NPCs, but never show them to the PC.
",
    super::pack_rule_text!(),
    "
VERDICT
- allowed: the action simply happens. This is the usual verdict. Talking, greeting, asking, insulting, moving, looking and ordinary actions are allowed; NPCs answer through reactions.
- Words are never a check: whatever the PC says, even an insult or a threat, is said. The consequence is the NPCs' reactions and their later actions.
- impossible: the world prevents it outright. List the facts that block it in blockers.
- check: only when BOTH are true: the outcome is truly uncertain, AND failure has real stakes (danger, an NPC who actively opposes the PC, an open obstacle). Then include check with win and lose stakes as slot writes, and factors that cite existing facts.
- contested: true when an NPC actively opposes this act and the result is uncertain (arm-wrestling, a race, a staring match); then verdict is check, with against = that NPC. Else false.
",
    intimacy_field_text!(),
    "
- If either part is missing, do not use check: choose allowed or impossible. Include check only when verdict is check. Never set a numeric difficulty.
LATER ACTIONS
- reactions: what each affected NPC intends right now, as a short plan, not prose.
",
    willing_field_text!(),
    "
- When an NPC decides to act later (revenge, a report, a visit, fetching someone), use schedule: actor = that NPC, in_minutes = minutes until it happens, text = what happens, effects = the slot writes it causes, seen_by = the NPCs who will witness it (never the PC; omit when nobody does), public = true when everyone at the place will see or hear what it changes (an announcement, a public act), false when only seen_by notice it, moves_actor_to = the place under ALL PLACES where the actor is when it happens, away for a place not listed, null when the actor stays where they are. Never record such a plan as a fact.
- An NPC who is insulted, threatened or wronged, and who would hit back later rather than now, gets a schedule.
- facts: only named or significant new details, each with seen = pc, present or gm. Reuse the keys listed for each entity.
- A slot is subject (a handle), key (a plain word such as mood or suspicion, never a handle) and value. On the PC, only write condition, injury, fatigue, carrying, reputation, standing or debt; put other consequences on NPCs, places or things.
- This holds for schedule effects too. A grudge or a complaint lives on the NPC: subject = the NPC's handle, key = grudge, value = the PC's name.
- move_to: when the player moves the PC to a place listed under PLACES, that place's handle; else null, and the PC stays. Never write the PC's location in facts or stakes.
- leaves: each NPC who walks out of the PC's place in this Turn, with to = the handle of the place under ALL PLACES they go to, or away. Empty when nobody leaves. An NPC not listed stays here, whatever their plan says.
- settles: what the player's intent of THIS Turn does to one item under OPEN ITEMS. item none when the intent settles none of them. Else that item, with outcome accepted, refused, done, failed or dropped when this intent accepts, refuses, does, fails or gives it up; outcome open when it only asks about it, talks about it or asks for more time. An item settled in an earlier Turn is not listed.
",
    finds_field_text!(),
    "
TIME
- Always set minutes: the game time this action takes. A short exchange takes a few minutes; crossing town takes about 15.
- For wait: use the duration the player names. If the player names none, use the minutes until something happens.
- When the PC waits or stays passive, the wait ends because something happens: a present NPC acts on their goal toward the PC. They summon, question, give work or leave.
- So on a wait, each reaction plan is a concrete action, never observe or watch.
"
);

/// The prompt and the handles it shows. The per-Turn schema
/// (`proposal_schema_for`) must use exactly these handles.
#[derive(Clone, Debug)]
pub struct ProposalContext {
    pub prompt: Prompt,
    pub handles: HandleContext,
}

/// The proposal prompt of a Turn outside a cleared Intimate Scene:
/// `proposal_context` with no clearance.
pub fn proposal_prompt(
    view: &crate::store::WorldView,
    packs: &[ContentPack],
    intent: &str,
) -> Prompt {
    proposal_context(view, packs, intent, None).prompt
}

/// The handles that `proposal_prompt(view, _, intent)` shows. Packs never
/// change them.
pub fn handle_context(view: &crate::store::WorldView, intent: &str) -> HandleContext {
    handles(
        &Retrieval::new(view, intent, None, DetailLevel::Excluded),
        intent,
        false,
    )
}

/// The proposal prompt and its handles. `cleared` is G1's value
/// (`mature::scene_gate`): with `Some`, the `recent` block opens with
/// `SCENE_LINE`, then the proposal scene fragments of the Content Packs
/// (the explicit-only ones too when `mature::effective` of the stored level
/// is `explicit`, ADR 0009), and the Turns of the open scene are shown in
/// full (`mature::turn_text`), and the schema takes the longer plan. With
/// `None`, the prompt holds no scene text at all. The system, stable and
/// scene blocks do not depend on it, so the cached prefix stays the same
/// when a scene opens or ends (ADR 0001).
pub fn proposal_context(
    view: &crate::store::WorldView,
    packs: &[ContentPack],
    intent: &str,
    cleared: Option<&Cleared>,
) -> ProposalContext {
    let r = Retrieval::new(view, intent, cleared, effective(view.detail, packs));
    let prompt = Prompt {
        system: system(packs),
        stable: format!(
            "{}{}",
            adventure_block(r.view, r.pc),
            names_block(r.view, NamesAudience::Gm)
        ),
        scene: scene(&r),
        recent: recent(&r, intent, cleared, packs),
    };
    ProposalContext {
        prompt,
        handles: handles(&r, intent, cleared.is_some()),
    }
}

/// The proposal prompt of a Closing Turn and its handles (data shape 3.7
/// step 1): the proposal rules with no Content Pack fragment, the stable
/// and scene blocks of any Turn, and a `recent` block whose Intent is
/// `CLOSING_ASK`. `cleared` is G1's value, as for a Play Turn: with `Some`,
/// the scene line opens `recent` and the Turns of the scene are shown in
/// full. The schema is `closing_schema_for` of the handles. `packs` gives
/// the effective Detail Level only (`mature::effective`): the closing
/// prompt holds no pack fragment.
pub fn closing_context(
    view: &crate::store::WorldView,
    packs: &[ContentPack],
    cleared: Option<&Cleared>,
) -> ProposalContext {
    let level = effective(view.detail, packs);
    let r = Retrieval::new(view, "", cleared, level);
    let prompt = Prompt {
        system: PROPOSAL_RULES.to_owned(),
        stable: format!(
            "{}{}",
            adventure_block(r.view, r.pc),
            names_block(r.view, NamesAudience::Gm)
        ),
        scene: scene(&r),
        recent: recent_with(&r, "", CLOSING_ASK, cleared, &[], level),
    };
    ProposalContext {
        prompt,
        handles: handles(&r, "", cleared.is_some()),
    }
}

/// The prompt of `closing_context`, with no pack loaded.
pub fn closing_prompt(view: &crate::store::WorldView, cleared: Option<&Cleared>) -> Prompt {
    closing_context(view, &[], cleared).prompt
}

fn handles(r: &Retrieval<'_>, intent: &str, scene: bool) -> HandleContext {
    let places = known_places(r.view);
    let mut entities: Vec<EntityId> = r.scene_entities.clone();
    entities.extend(&r.mentioned);
    entities.extend(&places);
    entities.sort_unstable();
    entities.dedup();
    let mut facts: Vec<u64> = r
        .scene_facts
        .iter()
        .chain(&r.obstacles)
        .chain(&r.mentioned_facts)
        .map(|f| f.id)
        .collect();
    facts.sort_unstable();
    facts.dedup();
    HandleContext {
        entities: entities.into_iter().map(Handle::Entity).collect(),
        facts: facts.into_iter().map(Handle::Fact).collect(),
        events: r.events.iter().map(|w| Handle::WorldEvent(w.id)).collect(),
        goals: r.goals.iter().map(|g| Handle::Goal(g.id)).collect(),
        new_slots: 1,
        places: places.into_iter().map(Handle::Entity).collect(),
        leavers: leavers(r).into_iter().map(Handle::Entity).collect(),
        destinations: destinations(r).into_iter().map(Handle::Entity).collect(),
        open_items: offered(r, intent)
            .into_iter()
            .map(|(subject, key, _)| open_item_ref(subject, &key))
            .collect(),
        scene,
    }
}

/// The NPCs who may leave this Turn (`leaves.npc`, bug pass 5): the present
/// NPCs the PC perceives here.
fn leavers(r: &Retrieval<'_>) -> Vec<EntityId> {
    r.seen_present
        .iter()
        .copied()
        .filter(|id| r.present.contains(id))
        .filter(|id| r.entity(*id).is_some_and(|e| e.kind == EntityKind::Npc))
        .collect()
}

/// Every place entity, in id order: where a leaving NPC or an event's actor
/// may go (`leaves.to`, `schedule.moves_actor_to`).
fn destinations(r: &Retrieval<'_>) -> Vec<EntityId> {
    let mut places: Vec<EntityId> = r
        .view
        .entities
        .iter()
        .filter(|e| e.kind == EntityKind::Place)
        .map(|e| e.id)
        .collect();
    places.sort_unstable();
    places
}

/// The Open Items the Intent names (`world::items_named_by`), each with its
/// text as the PC holds it (`world::pc_item_text`): the values of
/// `settles.item` (bug pass 3). An item the PC holds no text for is not
/// offered: it could not close (review B3).
fn offered(r: &Retrieval<'_>, intent: &str) -> Vec<(EntityId, String, String)> {
    items_named_by(r.view, intent)
        .into_iter()
        .filter_map(|(subject, key)| {
            let text = pc_item_text(r.view, subject, &key)?;
            // Every prompt line of an open item goes through `fact_text`,
            // so it carries the open marker (review minor).
            let line = fact_text(&key, &text);
            Some((subject, key, line))
        })
        .collect()
}

fn system(packs: &[ContentPack]) -> String {
    with_fragments(PROPOSAL_RULES, packs, Target::Proposal)
}

/// One entity line. It takes the `Entity` itself, so an unknown id can never
/// be rendered as an empty line.
fn entity_line(r: &Retrieval<'_>, e: &Entity) -> String {
    let mut line = identity(e);
    line.push_str(&entity_state(r, e.id));
    line.push('\n');
    line
}

/// `E<id> <name> (<kind>, age <n>)`: what does not change in play.
fn identity(e: &Entity) -> String {
    let kind = serde_json::to_value(e.kind)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default();
    let mut line = format!("E{} {} ({kind}", e.id, e.name);
    if let Some(age) = e.age {
        line.push_str(&format!(", age {age}"));
    }
    line.push(')');
    line
}

/// `; attitude to PC <n>; keys: <keys>`, or nothing: what a Turn changes.
fn entity_state(r: &Retrieval<'_>, id: EntityId) -> String {
    let mut out = String::new();
    if let Some(pc) = r.pc.filter(|pc| *pc != id)
        && let Some(a) = r.view.attitudes.iter().find(|a| a.from == id && a.to == pc)
    {
        out.push_str(&format!("; attitude to PC {}", a.attitude));
    }
    let keys = r.live_keys(id);
    if !keys.is_empty() {
        out.push_str(&format!("; keys: {}", keys.join(", ")));
    }
    out
}

fn fact_line(r: &Retrieval<'_>, f: &Fact) -> String {
    let canon = match f.canon {
        Canon::Possible => "possible",
        Canon::Established => "established",
        Canon::Persistent => "persistent",
    };
    // The location of an entity the PC knows is here is not hidden, even
    // when the PC learned it from another fact (`seen_present`).
    let in_view = f.slot.key == "location" && r.seen_present.contains(&f.slot.subject);
    let seen = if in_view || r.pc.is_some_and(|pc| f.seen_by.contains(&pc)) {
        "pc knows"
    } else {
        "hidden"
    };
    let mut line = format!(
        "F{} E{} {} = {} ({canon}, {seen})",
        f.id, f.slot.subject, f.slot.key, f.slot.value
    );
    // An open item shows its text with the marker, after its key.
    if is_open_key(&f.slot.key) {
        line.push_str(&format!(": {}", fact_text(&f.slot.key, &f.text)));
    }
    line.push('\n');
    line
}

/// The scene block, the part that changes least first (ADR 0001, changed in
/// bug pass 4): Ollama reuses a prompt up to its first changed byte. QA's
/// proposal prompts broke inside this block on 5 of 20 Turns, at an
/// ENTITIES line whose key list or attitude had changed, and every line
/// after it was read again. So the entity lines hold the name only, the
/// Persistent facts come before the others, and the key lists and
/// attitudes come last, under ENTITY STATE.
fn scene(r: &Retrieval<'_>) -> String {
    let mut out = String::from("SCENE\n");
    match (&r.location, r.place) {
        (Some(location), Some(place)) => out.push_str(&format!("Place: {location} (E{place})\n")),
        (Some(location), None) => out.push_str(&format!("Place: {location}\n")),
        _ => out.push_str("Place: unknown\n"),
    }
    let places: Vec<String> = known_places(r.view)
        .into_iter()
        .map(|id| format!("E{id} {}", r.known(id).name))
        .collect();
    if !places.is_empty() {
        out.push_str(&format!("PLACES (for move_to): {}\n", places.join(", ")));
    }
    let all: Vec<String> = destinations(r)
        .into_iter()
        .map(|id| format!("E{id} {}", r.known(id).name))
        .collect();
    if !all.is_empty() {
        out.push_str(&format!(
            "ALL PLACES (for leaves and moves_actor_to): {}\n",
            all.join(", ")
        ));
    }
    out.push_str("ENTITIES\n");
    for id in &r.scene_entities {
        out.push_str(&identity(r.known(*id)));
        out.push('\n');
    }
    let voices = r.voice_cards(&r.present);
    if !voices.is_empty() {
        out.push_str("VOICES\n");
        for card in voices {
            out.push_str(&render_voice(&format!("E{}", card.npc), card));
        }
    }
    if !r.goals.is_empty() {
        out.push_str("NPC GOALS\n");
        for g in &r.goals {
            let against = match (g.against_id, r.pc) {
                (Some(a), Some(pc)) if a == pc => " (against PC)".to_owned(),
                (Some(a), _) if r.scene_entities.contains(&a) => format!(" (against E{a})"),
                _ => String::new(),
            };
            out.push_str(&format!(
                "G{} E{}: {}{against}\n",
                g.id,
                g.npc_id,
                one_line(&g.text)
            ));
        }
    }
    if !r.scene_facts.is_empty() {
        out.push_str("FACTS\n");
        // Persistent facts first: no Turn changes them, so a new or a
        // changed fact breaks the prefix after them.
        let (fixed, open): (Vec<&Fact>, Vec<&Fact>) = r
            .scene_facts
            .iter()
            .partition(|f| f.canon == Canon::Persistent);
        // Bug pass 5 (M4): the block is bounded. Over the budget, the oldest
        // facts that are not a `location` or an open item go: Established
        // ones first, then Persistent ones, each in id order.
        let may_go = |f: &Fact| f.slot.key != "location" && !is_open_key(&f.slot.key);
        let mut lines: Vec<(u8, String)> = fixed
            .into_iter()
            .map(|f| (if may_go(f) { 2 } else { 0 }, fact_line(r, f)))
            .chain(
                open.into_iter()
                    .map(|f| (if may_go(f) { 1 } else { 0 }, fact_line(r, f))),
            )
            .collect();
        for rank in [1, 2] {
            while lines.iter().map(|(_, l)| l.len()).sum::<usize>() > GM_FACTS_MAX_CHARS {
                let Some(at) = lines.iter().position(|(r, _)| *r == rank) else {
                    break;
                };
                lines.remove(at);
            }
        }
        for (_, line) in lines {
            out.push_str(&line);
        }
    }
    if !r.obstacles.is_empty() {
        out.push_str("OPEN OBSTACLES\n");
        for f in &r.obstacles {
            out.push_str(&fact_line(r, f));
        }
    }
    if !r.events.is_empty() {
        out.push_str("SCHEDULED EVENTS\n");
        for w in &r.events {
            let when = match &w.trigger {
                Trigger::At(minute) => format!("at minute {minute}"),
                Trigger::When(slot) => {
                    format!("when E{} {} = {}", slot.subject, slot.key, slot.value)
                }
            };
            let actors: Vec<String> = w.actors.iter().map(|a| format!("E{a}")).collect();
            out.push_str(&format!(
                "W{} {when}, actor {}: {}\n",
                w.id,
                actors.join(" "),
                one_line(&w.text)
            ));
        }
    }
    let state: Vec<String> = r
        .scene_entities
        .iter()
        .filter_map(|id| {
            let state = entity_state(r, *id);
            let state = state.strip_prefix("; ")?;
            Some(format!("E{id}: {state}\n"))
        })
        .collect();
    if !state.is_empty() {
        out.push_str("ENTITY STATE\n");
        out.push_str(&state.concat());
    }
    out
}

fn recent(
    r: &Retrieval<'_>,
    intent: &str,
    cleared: Option<&Cleared>,
    packs: &[ContentPack],
) -> String {
    let ask = format!("INTENT\n{}\n", intent.trim());
    let level = effective(r.view.detail, packs);
    recent_with(r, intent, &ask, cleared, packs, level)
}

/// The `recent` block: past Turns, the clock, the offered items, then `ask`
/// (the Intent of a Play Turn, or `CLOSING_ASK`). Inside a cleared scene,
/// the scene line and the pack scene fragments of `packs` come first.
fn recent_with(
    r: &Retrieval<'_>,
    intent: &str,
    ask: &str,
    cleared: Option<&Cleared>,
    packs: &[ContentPack],
    level: DetailLevel,
) -> String {
    let mut head = format!("NOW\nMinute {}\n", r.view.clock_min);
    if !r.mentioned.is_empty() {
        head.push_str("MENTIONED ENTITIES (not present)\n");
        for id in &r.mentioned {
            head.push_str(&entity_line(r, r.known(*id)));
        }
        if !r.mentioned_facts.is_empty() {
            head.push_str("MENTIONED FACTS\n");
            for f in &r.mentioned_facts {
                head.push_str(&fact_line(r, f));
            }
        }
    }
    // The offered items depend on the Intent, so they sit in the tail with
    // it: the prefix of the prompt stays stable within a Scene.
    let mut tail = String::new();
    let items = offered(r, intent);
    if !items.is_empty() {
        tail.push_str("OPEN ITEMS (for settles)\n");
        for (subject, key, text) in items {
            tail.push_str(&format!("{}: {text}\n", open_item_ref(subject, &key)));
        }
    }
    tail.push_str(ask);
    // The past Turns first, then the clock and the Intent: the next Turn's
    // prompt differs from this one only after the newest Turn (bug pass 5,
    // M4). Inside a cleared scene the scene line comes before them, then
    // the pack scene fragments (data shape 3.8).
    let scene_head = cleared.map_or_else(String::new, |cleared| {
        let explicit = effective(r.view.detail, packs) == DetailLevel::Explicit;
        let fragments = scene_fragments(packs, Target::Proposal, cleared, explicit);
        format!("{SCENE_LINE}{}", pack_block(&fragments))
    });
    format!(
        "{scene_head}{}{head}{tail}",
        turns_window(r.view, r.view.turn_seq + 1, cleared, level)
    )
}
