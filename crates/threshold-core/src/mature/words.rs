//! The deterministic net under the intimacy tag (ADR 0008, changed after
//! the live evaluation of 2026-11-12): the core reads the Player's Intent
//! for a short list of neutral romance words and phrases, and raises the
//! Turn's tag to at least `advance` when one is there, whatever the model
//! says. It never lowers a model value. It over-blocks by design: a false
//! `advance` costs one plain Turn, or one Refusal where a character is not
//! cleared; a missed tag runs no age rule.

use crate::{domain::Intimacy, normalize, pack::ContentPack};

/// The Romance Words (glossary): neutral words and phrases that mark a
/// romantic or sexual advance or invitation in an Intent. Each term is
/// matched as whole words, in order, with case and accents folded
/// (`normalize::name_words`). Nothing on it is more explicit than these
/// words. Why each group is here, and why some near words are not:
///
/// - Spending the night: "night with" and "night together", not "spend the
///   night" alone ("I spend the night on watch"); "stay with me tonight",
///   not "with me tonight" ("finish the roll with me tonight").
/// - Going home or to bed together: "come home with me"; "to bed with",
///   "come to bed", "into bed with", "share a bed", "share my bed", "share
///   your bed", "share her bed", "share his bed"; not "to bed" alone ("I go
///   to bed early" is a wait) and not "my bed".
/// - Sleeping with someone: the phrase with a person only ("sleep with
///   her"), not "sleep with" alone ("I sleep with my sword at hand").
/// - Acts of affection and courtship: kiss, embrace, caress, undress,
///   seduce, flirt, make love, lover, romance, court her or him, intimate,
///   intimacy, with their usual forms. "court" alone is a place of law.
pub const ROMANCE_WORDS: &[&str] = &[
    "night with",
    "night together",
    "stay with me tonight",
    "come home with me",
    "to bed with",
    "come to bed",
    "into bed with",
    "share a bed",
    "share my bed",
    "share your bed",
    "share her bed",
    "share his bed",
    "sleep with me",
    "sleep with you",
    "sleep with her",
    "sleep with him",
    "sleep with them",
    "slept with me",
    "slept with you",
    "slept with her",
    "slept with him",
    "kiss",
    "kisses",
    "kissed",
    "kissing",
    "embrace",
    "embraces",
    "embraced",
    "embracing",
    "caress",
    "caresses",
    "caressed",
    "caressing",
    "undress",
    "undresses",
    "undressed",
    "undressing",
    "make love",
    "makes love",
    "made love",
    "making love",
    "seduce",
    "seduces",
    "seduced",
    "seducing",
    "seduction",
    "lover",
    "lovers",
    "flirt",
    "flirts",
    "flirted",
    "flirting",
    "romance",
    "romantic",
    "court her",
    "court him",
    "courting",
    "intimate",
    "intimacy",
];

/// The Act Words (glossary; QA polish pass, M4): neutral phrases that name
/// the intimate act itself, not an advance or an invitation. An Intent that
/// holds one is `intimate` whatever the model says, as a pack word list
/// makes it (ADR 0008). QA typed "I make love with Maren." at `excluded`:
/// the model tagged it `advance` in one run of two, and the Turn passed.
/// "I ask Maren to make love" is `intimate` too: the net over-blocks by
/// design. A kiss and an invitation ("share my bed") stay Romance Words.
pub const ACT_WORDS: &[&str] = &["make love", "makes love", "made love", "making love"];

/// The verbs of the Act Word phrase "take someone to bed" (QA run 3): "I
/// take Maren to bed." was tagged `advance`, opened no Intimate Scene, and
/// the Narration told Maren undressing the PC. The intimacy definition
/// already counts going to bed together as intimate; this phrase makes the
/// core agree when the model does not. Not "put", "carry" or "tuck": those
/// are the words of putting a child or a sick person to bed.
pub const TO_BED_VERBS: &[&str] = &[
    "take", "takes", "took", "taking", "lead", "leads", "led", "leading",
];

/// The pronouns that can stand for the person taken to bed.
const TO_BED_PRONOUNS: &[&str] = &["her", "him", "them", "me"];

/// The possessives that may stand between "to" and "bed": "lead her to my
/// bed", "Maren takes me to her bed" (QA B1).
const TO_BED_POSSESSIVES: &[&str] = &["my", "her", "his", "their", "your", "our"];

/// True when `intent` takes or leads a person to bed or into bed: a
/// `TO_BED_VERBS` verb, then a pronoun of `TO_BED_PRONOUNS` or one or two
/// name words, then "to bed" or "into bed", with one possessive of
/// `TO_BED_POSSESSIVES` allowed before "bed". A name word starts with a
/// capital in the Player's own words (`normalize::cased_words`), or is a
/// word of a character's name in `names`, so "i take maren to bed" holds it
/// (QA B1). "I take Maren to bed", "I lead her to my bed" and "Maren takes
/// me to her bed" hold it; "I go to bed", "I take my book to bed" and "i
/// take tea to bed" do not. Cost: "I take Pim to bed" holds it too, so the
/// age rule refuses it when Pim is a child; the net over-blocks by design.
/// `names` holds only the characters the PC can name (`turn::plan`): a
/// lower-case name of a hidden character would tell the Player it exists.
fn takes_to_bed(intent: &str, names: &[&str]) -> bool {
    let cased = normalize::cased_words(intent);
    let lower: Vec<String> = cased.iter().map(|w| w.to_lowercase()).collect();
    let name_words: Vec<String> = names.iter().flat_map(|n| folded(n)).collect();
    let word = |at: usize| lower.get(at).map(String::as_str);
    let ends_in_bed = |at: usize| match (word(at), word(at + 1), word(at + 2)) {
        (Some("to" | "into"), Some("bed"), _) => true,
        (Some("to" | "into"), Some(owner), Some("bed")) => TO_BED_POSSESSIVES.contains(&owner),
        _ => false,
    };
    let named = |at: usize| {
        let Some(w) = word(at) else {
            return false;
        };
        let capital = cased[at].chars().next().is_some_and(char::is_uppercase);
        (capital || name_words.iter().any(|n| n == w))
            && !matches!(w, "the" | "a" | "an" | "my" | "your" | "our" | "i")
    };
    (0..lower.len())
        .filter(|&i| TO_BED_VERBS.contains(&lower[i].as_str()))
        .any(|i| {
            let person = i + 1;
            if word(person).is_some_and(|w| TO_BED_PRONOUNS.contains(&w)) {
                return ends_in_bed(person + 1);
            }
            named(person)
                && (ends_in_bed(person + 1) || (named(person + 1) && ends_in_bed(person + 2)))
        })
}

/// The words of `text` as the net reads them: lower case, accents dropped,
/// split on every character that is not a letter or a digit.
pub fn folded(text: &str) -> Vec<String> {
    normalize::name_words(text)
}

/// True when `words` holds the words of `term` next to each other, in
/// order. A term with no word matches nothing.
pub(crate) fn holds(words: &[String], term: &[String]) -> bool {
    !term.is_empty() && words.windows(term.len()).any(|w| w == term)
}

/// The tag the core's list gives `intent`: `advance` when it holds a
/// Romance Word, else `none`. The Turn's tag is the stronger of this and
/// the model's (`turn::plan`).
pub fn romance_floor(intent: &str) -> Intimacy {
    let words = folded(intent);
    let hit = ROMANCE_WORDS
        .iter()
        .any(|term| holds(&words, &folded(term)));
    if hit {
        Intimacy::Advance
    } else {
        Intimacy::None
    }
}

/// The tag the Intent's words give, and where it comes from (ADR 0008):
/// `intimate` when the Intent holds a term of a loaded pack's word list
/// (`ContentPack::intimate_words`), else `intimate` when it holds an Act
/// Word (`ACT_WORDS`) or takes a person to bed (`takes_to_bed`, which
/// reads `names`, the characters the PC can name), else `advance` when
/// it holds a Romance Word, else `none`. The Turn's tag is the stronger of
/// this and the model's (`turn::plan`); the core never lowers a model value.
pub fn intimacy_floor(intent: &str, packs: &[ContentPack], names: &[&str]) -> (Intimacy, Floor) {
    let words = folded(intent);
    let pack_hit = packs
        .iter()
        .flat_map(|p| p.intimate_words.terms())
        .any(|term| holds(&words, term));
    let act_hit =
        ACT_WORDS.iter().any(|term| holds(&words, &folded(term))) || takes_to_bed(intent, names);
    if pack_hit {
        (Intimacy::Intimate, Floor::PackWords)
    } else if act_hit {
        (Intimacy::Intimate, Floor::ActWords)
    } else if romance_floor(intent) == Intimacy::Advance {
        (Intimacy::Advance, Floor::RomanceWords)
    } else {
        (Intimacy::None, Floor::Nothing)
    }
}

/// `intimacy_floor` of a Play Intent in `view`: the names it reads are the
/// names the PC can name, main or other (`world::pc_character_names`). A
/// lower-case name of a hidden character would tell the Player it exists
/// (QA B1). The engine raises the Turn's tag with it, and the proposal
/// prompt reads it to know, before the model answers, that the age rule
/// runs (review r5, M1).
pub fn intent_floor(
    view: &crate::store::WorldView,
    intent: &str,
    packs: &[ContentPack],
) -> (Intimacy, Floor) {
    let known = crate::world::pc_character_names(view);
    let names: Vec<&str> = known.iter().map(String::as_str).collect();
    intimacy_floor(intent, packs, &names)
}

/// The age rule may run for this Play Turn, as the core knows before the
/// model answers: the Intent's words gate it (`intent_floor`), or an
/// Intimate Scene is open (`view.scene_open`; G1's `Cleared` value only
/// exists then). The proposal prompt reads it once (`Retrieval::gated`),
/// so the first-word match, the `arrives` offer and the "is not here"
/// lines agree (review r5 re-check, MJ1): inside an open scene, "think of
/// little Pip" offered Pip, a minor, for `arrives`.
pub fn gated_before_answer(
    view: &crate::store::WorldView,
    intent: &str,
    packs: &[ContentPack],
) -> bool {
    view.scene_open.is_some() || intent_floor(view, intent, packs).0.gated()
}

/// Which list raised the tag. The debug log names the list, never the word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Floor {
    Nothing,
    /// The core's Romance Words: at least `advance`.
    RomanceWords,
    /// The core's Act Words: at least `intimate`.
    ActWords,
    /// A loaded Content Pack's word list: at least `intimate`.
    PackWords,
}

impl Floor {
    /// The debug line when this floor raised a model value. It names the
    /// list and the value, never the word.
    pub fn line(self) -> &'static str {
        match self {
            Self::PackWords => "the core raised the tag to intimate from a Content Pack word list",
            Self::RomanceWords => "the core raised the tag to advance from its Romance Words",
            Self::ActWords => "the core raised the tag to intimate from its Act Words",
            Self::Nothing => "",
        }
    }
}
