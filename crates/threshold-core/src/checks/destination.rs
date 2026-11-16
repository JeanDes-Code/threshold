//! Where a move goes, and who comes along (ADR 0007, changed after QA run
//! 4, 2026-11-16). The schema keeps `move_to` inside the known places, but
//! it cannot check that the place is the one the Player named. QA run 4 saw
//! both errors: the model sent `unknown_place` for the Cooper house, which
//! PLACES listed, and sent the Cooper house for "the reeve's house", which
//! the world does not have. These rules read the Intent's own words, so
//! they hold whatever the model writes. They change `move_to` only toward
//! the place the Intent names, or to "stay": they add no PC act.

use super::{CheckKind, Rejection, leavers};
use crate::{
    domain::{ActionKind, DecisionProposal, EntityId, EntityKind, Handle, Verdict, Willing},
    normalize,
    store::WorldView,
    world,
};

/// The words before a place phrase: "walk to the ferry house", "go back
/// into the inn".
const TOWARD: &[&str] = &["to", "into", "onto", "toward", "towards"];

/// A word that ends a place phrase: "the Cooper house on the lane", "the
/// reeve's house and knock".
const PHRASE_END: &[&str] = &[
    "and", "to", "for", "with", "at", "in", "on", "by", "near", "behind", "past", "through",
    "from", "across", "along", "over", "under", "beside", "around", "where", "so", "then",
    "before", "after", "because", "while", "until", "but", "or", "up", "down", "inside", "outside",
    "as", "if", "when",
];

/// Words of a place phrase that name no place: articles, possessives, the
/// `s` of "reeve's".
const NOT_A_NAME: &[&str] = &[
    "the", "a", "an", "my", "our", "his", "her", "their", "its", "your", "this", "that", "s",
    "back",
];

/// The words just before a character's name that take it along on a move:
/// "with Ilse", "I take Ilse by the hand".
const TAKES: &[&str] = &[
    "with", "take", "takes", "took", "bring", "brings", "lead", "leads", "escort", "escorts",
    "guide", "guides",
];

/// The words just after a character's name that make it go too: "Ilse
/// follows me", "Ilse comes along".
const GOES: &[&str] = &[
    "follows", "follow", "comes", "come", "joins", "join", "walks", "walk", "goes", "go",
];

/// Checks `move_to` against the places the Intent names (QA run 4). Only a
/// move is read. `retry` is true on the second proposal. Returns the notes
/// for the debug log and, on a first proposal, a rejection that asks the
/// model again.
///
/// - The Intent names a known place (main name or an other name the PC
///   holds, any case, with or without an article) other than the PC's own,
///   inside a place phrase ("to the mill"), or anywhere when the Intent
///   has no place phrase: `move_to` must be that place. The core never
///   sets a place the model did not judge (review M2, 2026-11-16). A model
///   that said `unknown_place`, or another known place, gets the first
///   proposal back with a note, whatever its verdict. A retry that still
///   does not go there stays put with `not_reached` (the place is known:
///   never `unknown_place`), and no Check. A retry that sent another known
///   place loses its verdict and blockers: they were judged for that
///   place. A retry that said `unknown_place` with `impossible` keeps them:
///   it judged the Intent's own words. A
///   model that kept the PC in place (`null`, not `unknown_place`) is kept:
///   a name in the Intent is no move ("I watch Tobin reach the Cooper
///   house"). A place phrase that names the PC's own place removes the
///   move: the PC is already there.
/// - The Intent names no known place, but a place phrase ("to the reeve's
///   house") whose words do not name the model's place: the PC stays, as
///   for `unknown_place`. A phrase that names a character the PC knows ("to
///   Pip") is no place phrase. An Intent with no place phrase ("I go home")
///   keeps the model's place.
pub fn named_destination(
    p: &mut DecisionProposal,
    view: &WorldView,
    intent: &str,
    retry: bool,
) -> (Vec<String>, Option<Rejection>) {
    let mut notes = Vec::new();
    if p.action != ActionKind::Move {
        return (notes, None);
    }
    let phrases = place_phrases(intent);
    let all_named = named_places(view, intent);
    // The place a "to ..." phrase names is where the PC goes; another name
    // can be where it starts ("I leave the town hall and walk to the
    // mill").
    let aimed = all_named
        .iter()
        .copied()
        .find(|id| phrases.iter().any(|ph| refers(view, ph, *id)));
    if let Some(id) = aimed.filter(|id| world::is_pc_place(view, Handle::Entity(*id))) {
        // The PC already stands where the Intent goes: no move to make.
        if let Some(h) = p.move_to.filter(|h| *h != Handle::Entity(id)) {
            p.move_to = None;
            notes.push(format!(
                "removed move_to {h}: the intent goes to the place the PC is in"
            ));
        }
        return (notes, None);
    }
    let named: Vec<EntityId> = all_named
        .into_iter()
        .filter(|id| !world::is_pc_place(view, Handle::Entity(*id)))
        .collect();
    // A known place named outside a place phrase is a destination only when
    // the Intent has no place phrase at all ("I head for the Cooper house").
    // "I walk to the window and look out at the Copper Kettle" goes to the
    // window, not to the Kettle (review M2, 2026-11-16).
    let target = aimed.or_else(|| phrases.is_empty().then(|| named.last().copied()).flatten());
    if let Some(target) = target {
        let fits = |h: Handle| match aimed {
            Some(id) => h == Handle::Entity(id),
            None => named.iter().any(|id| h == Handle::Entity(*id)),
        };
        if p.move_to.is_some_and(fits) || (p.move_to.is_none() && !p.unknown_place) {
            return (notes, None);
        }
        let handle = Handle::Entity(target);
        let was = p.move_to.map_or_else(
            || crate::domain::UNKNOWN_PLACE.to_owned(),
            |h| h.to_string(),
        );
        if !retry {
            // The model judged its verdict for another place: the core never
            // commits a place the model did not judge (review M2).
            let name = place_name(view, target);
            let reason = format!(
                "the intent names {handle} {name}, a place listed under PLACES: set move_to {handle}, and judge the verdict for that place"
            );
            return (
                notes,
                Some(Rejection::new(CheckKind::Destination, Some(handle), reason)),
            );
        }
        // The retry still does not go to the named place: the PC stays. The
        // place is known, so this is `not_reached`, never `unknown_place`
        // (final review, 2026-11-16). No Check and no Roll: there is no move
        // to win.
        let judged_elsewhere = p.move_to.is_some();
        p.move_to = None;
        p.unknown_place = false;
        p.not_reached = Some(handle);
        p.contested = false;
        p.check = None;
        if judged_elsewhere || p.verdict != Verdict::Impossible {
            // The verdict and blockers were judged for another known place,
            // or were a Check: none of them is kept.
            p.verdict = Verdict::Allowed;
            p.blockers.clear();
            notes.push(format!(
                "removed move_to {was}, the verdict and the check: the retry still does not go to {handle}, the place the intent names; the PC does not reach it"
            ));
        } else {
            // `unknown_place` with `impossible`: the model judged the
            // Intent's own words, so its blockers tell why the PC does not
            // get there.
            notes.push(format!(
                "kept the verdict impossible and its blockers: the retry called {handle}, the place the intent names, unknown; the PC does not reach it"
            ));
        }
        return (notes, None);
    }
    let Some(place) = p.move_to else {
        return (notes, None);
    };
    let Handle::Entity(place_id) = place else {
        return (notes, None);
    };
    if world::is_pc_place(view, place) || phrases.is_empty() {
        return (notes, None);
    }
    let kept = phrases
        .iter()
        .any(|ph| refers(view, ph, place_id) || names_a_character(view, ph));
    if !kept {
        p.move_to = None;
        p.unknown_place = true;
        notes.push(format!(
            "removed move_to {place}: the intent names a place the PC does not know, not that one"
        ));
    }
    (notes, None)
}

/// Who comes along on this move (ADR 0007, 2026-11-16): each present NPC
/// the PC perceives that the Intent names as going with the PC ("Ilse and
/// I", "with Ilse", "I take Ilse", "Ilse follows me"), when its reaction
/// says `willing: yes`. The others the Intent names that way stay behind.
/// An NPC in `leaves` goes where `leaves` sends it. Only a move with a
/// destination has companions. Returns `(companions, left_behind)`.
pub fn companions(
    p: &DecisionProposal,
    view: &WorldView,
    intent: &str,
) -> (Vec<Handle>, Vec<Handle>) {
    if p.action != ActionKind::Move || p.move_to.is_none() {
        return (Vec::new(), Vec::new());
    }
    let said = normalize::name_words(intent);
    let mut going = Vec::new();
    let mut left = Vec::new();
    for id in leavers(view) {
        let handle = Handle::Entity(id);
        if p.leaves.iter().any(|l| l.npc == handle) {
            continue;
        }
        let taken = character_names(view, id)
            .iter()
            .any(|name| taken_along(&said, &normalize::name_words(&normalize::name(name))));
        if !taken {
            continue;
        }
        let agrees = p
            .reactions
            .iter()
            .any(|r| r.npc == handle && r.willing == Some(Willing::Yes));
        if agrees {
            going.push(handle);
        } else {
            left.push(handle);
        }
    }
    (going, left)
}

/// True when the words `said` hold `name` as one that goes with the PC.
fn taken_along(said: &[String], name: &[String]) -> bool {
    if name.is_empty() {
        return false;
    }
    let at = |i: usize| said.get(i).map(String::as_str);
    said.windows(name.len())
        .enumerate()
        .filter(|(_, w)| *w == name)
        .any(|(start, _)| {
            let end = start + name.len();
            let before = start.checked_sub(1).and_then(at);
            let before2 = start.checked_sub(2).and_then(at);
            matches!((at(end), at(end + 1)), (Some("and"), Some("i" | "me")))
                || matches!((before2, before), (Some("i" | "me"), Some("and")))
                || before.is_some_and(|w| TAKES.contains(&w))
                || at(end).is_some_and(|w| GOES.contains(&w))
        })
}

/// The NPCs the PC can name that `intent` names (QA run 5, 2026-11-16): a
/// name of `character_names` as a run of whole words, whatever the case.
/// "ask Tobin where he ran off to" names Tobin Cooper by his first name.
/// In id order.
pub fn characters_named(view: &WorldView, intent: &str) -> Vec<EntityId> {
    let said = normalize::name_words(intent);
    view.entities
        .iter()
        .filter(|e| e.kind == EntityKind::Npc)
        .filter(|e| {
            character_names(view, e.id).iter().any(|name| {
                let name = normalize::name_words(&normalize::name(name));
                !name.is_empty() && said.windows(name.len()).any(|w| w == name.as_slice())
            })
        })
        .map(|e| e.id)
        .collect()
}

/// The names the PC can call character `id` by: its main name, its first
/// word, and each other name the PC holds an `alias_` Belief for.
fn character_names(view: &WorldView, id: EntityId) -> Vec<String> {
    if !world::pc_can_name(view).contains(&id) {
        return Vec::new();
    }
    let Some(e) = view.entities.iter().find(|e| e.id == id) else {
        return Vec::new();
    };
    let mut names = vec![e.name.clone()];
    if let Some(first) = e.name.split_whitespace().next() {
        names.push(first.to_owned());
    }
    names.extend(
        world::pc_knowledge(view)
            .into_iter()
            .filter(|b| b.subject == id && crate::domain::is_alias_key(&b.key))
            .map(|b| b.value),
    );
    names
}

/// The known places the Intent names, in the order the Intent first names
/// them: a place's main name, or an other name the PC holds, as a run of
/// whole words, whatever the case, a leading article left out. The place
/// the PC stands in counts as known.
fn named_places(view: &WorldView, intent: &str) -> Vec<EntityId> {
    let said = normalize::name_words(intent);
    let mut candidates = world::known_places(view);
    let here = view
        .entities
        .iter()
        .find(|e| world::is_pc_place(view, Handle::Entity(e.id)))
        .map(|e| e.id);
    candidates.extend(here.filter(|id| !candidates.contains(id)));
    let mut found: Vec<(usize, EntityId)> = candidates
        .into_iter()
        .filter_map(|id| {
            place_names(view, id)
                .iter()
                .filter_map(|name| {
                    let words = normalize::name_words(&normalize::name(name));
                    (!words.is_empty())
                        .then(|| {
                            said.windows(words.len())
                                .position(|w| w == words.as_slice())
                        })
                        .flatten()
                })
                .min()
                .map(|at| (at, id))
        })
        .collect();
    found.sort_unstable();
    found.into_iter().map(|(_, id)| id).collect()
}

/// A place's main name and each other name the PC holds an `alias_`
/// Belief for.
fn place_names(view: &WorldView, id: EntityId) -> Vec<String> {
    let Some(e) = view
        .entities
        .iter()
        .find(|e| e.id == id && e.kind == EntityKind::Place)
    else {
        return Vec::new();
    };
    std::iter::once(e.name.clone())
        .chain(
            world::pc_knowledge(view)
                .into_iter()
                .filter(|b| b.subject == id && crate::domain::is_alias_key(&b.key))
                .map(|b| b.value),
        )
        .collect()
}

fn place_name(view: &WorldView, id: EntityId) -> String {
    view.entities
        .iter()
        .find(|e| e.id == id)
        .map(|e| e.name.clone())
        .unwrap_or_default()
}

/// The place phrases of an Intent: the words after "to", "into", "onto" or
/// "toward(s)", up to a word of `PHRASE_END` or a punctuation mark, when
/// they start like a place: an article or a possessive ("the reeve's
/// house", "my room"), a capital ("Wexmoor") or a possessive name
/// ("reeve's house"). "to ask" and "to bed" are no place phrase. Each
/// phrase is its lower-case words, with the words of `NOT_A_NAME` left out.
fn place_phrases(intent: &str) -> Vec<Vec<String>> {
    let mut phrases = Vec::new();
    for clause in intent.split(['.', ',', ';', ':', '!', '?', '(', ')', '"']) {
        let tokens: Vec<&str> = clause.split_whitespace().collect();
        for (i, token) in tokens.iter().enumerate() {
            if !TOWARD.contains(&token.to_lowercase().as_str()) {
                continue;
            }
            let run: Vec<&str> = tokens[i + 1..]
                .iter()
                .copied()
                .take_while(|t| !PHRASE_END.contains(&t.to_lowercase().as_str()))
                .collect();
            let Some(first) = run.first() else {
                continue;
            };
            let lower = first.to_lowercase().replace('\u{2019}', "'");
            let starts_like_a_place = NOT_A_NAME.contains(&lower.as_str())
                || first.chars().next().is_some_and(char::is_uppercase)
                || lower.ends_with("'s");
            if !starts_like_a_place {
                continue;
            }
            let words: Vec<String> = normalize::name_words(&run.join(" "))
                .into_iter()
                .filter(|w| !NOT_A_NAME.contains(&w.as_str()))
                .collect();
            if !words.is_empty() {
                phrases.push(words);
            }
        }
    }
    phrases
}

/// True when every word of `phrase` is a word of a name of place `id`:
/// "the inn" names the Lantern Inn; "the reeve's house" does not name the
/// Cooper house.
fn refers(view: &WorldView, phrase: &[String], id: EntityId) -> bool {
    place_names(view, id).iter().any(|name| {
        let words = normalize::name_words(name);
        phrase.iter().all(|w| words.contains(w))
    })
}

/// True when `phrase` holds a name of a character the PC can name: "over
/// to Pip" is no place phrase.
fn names_a_character(view: &WorldView, phrase: &[String]) -> bool {
    view.entities
        .iter()
        .filter(|e| matches!(e.kind, EntityKind::Npc | EntityKind::Pc))
        .flat_map(|e| character_names(view, e.id))
        .any(|name| {
            let words = normalize::name_words(&normalize::name(&name));
            !words.is_empty() && phrase.windows(words.len()).any(|w| w == words.as_slice())
        })
}
