//! Two measures of what a Narration does with the PC (polish pass,
//! 2026-11-12), read by `speech` and `play_basics`:
//!
//! - **Echo**: the Narration repeats `ECHO_MIN_WORDS` or more words in a
//!   row of the Intent's speech. The speech is each quoted span of the
//!   Intent; an Intent with no quoted span (the repository's scenarios say
//!   "I ask Hessa where …") counts its words after the first word, with "I",
//!   "me", "my", "mine", "myself" and "am" read as "you", "you", "your",
//!   "yours", "yourself" and "are". So it also counts a Narration that
//!   restates the Intent's act in the Intent's own words.
//! - **PC feeling**: a sentence states a feeling or a thought of the PC.
//!   A sentence counts when "you" or the PC's first name is followed by a
//!   verb of `FEELING_VERBS` (one helper word of `HELPERS` may stand
//!   between), or when "your" is followed by a word of `INNER_PARTS`
//!   ("your heart", "your mind").
//!
//! Both are word rules, with known limits: "she gives you hope" counts as a
//! feeling, "a wave of relief" with no "you" before it does not; an echo
//! of three or four words does not count, and a paraphrase never does.

/// The shortest run of the Intent's words that counts as an echo.
pub const ECHO_MIN_WORDS: usize = 5;

/// Verbs of feeling and thought, in the forms a Narration uses.
const FEELING_VERBS: &[&str] = &[
    "feel",
    "feels",
    "felt",
    "feeling",
    "sense",
    "senses",
    "sensed",
    "wonder",
    "wonders",
    "wondered",
    "wondering",
    "think",
    "thinks",
    "thought",
    "realize",
    "realizes",
    "realized",
    "realise",
    "realises",
    "realised",
    "hope",
    "hopes",
    "hoped",
    "fear",
    "fears",
    "feared",
    "worry",
    "worries",
    "worried",
    "doubt",
    "doubts",
    "doubted",
    "know",
    "knows",
    "knew",
    "remember",
    "remembers",
    "remembered",
    "recall",
    "recalls",
    "recalled",
    "relax",
    "relaxes",
    "relaxed",
    "regret",
    "regrets",
    "regretted",
    "want",
    "wants",
    "wanted",
    "wish",
    "wishes",
    "wished",
    "long",
    "longs",
    "longed",
    "enjoy",
    "enjoys",
    "enjoyed",
    "love",
    "loves",
    "loved",
    "dread",
    "dreads",
    "dreaded",
    "suspect",
    "suspects",
    "suspected",
    "believe",
    "believes",
    "believed",
    "savor",
    "savour",
    "savors",
    "savours",
    "savored",
    "savoured",
];

/// Words that may stand between the subject and the verb.
const HELPERS: &[&str] = &[
    "can", "cannot", "can't", "could", "couldn't", "almost", "still", "suddenly", "now", "also",
    "just", "only", "do", "don't", "did", "didn't", "begin", "began", "start", "started",
    "already", "briefly", "finally", "somehow",
];

/// What "your" names when the sentence speaks of the PC's inner state.
/// Body words ("your breath", "your chest") are left out: on the baseline
/// run they named only effort ("your breath coming in ragged gasps").
const INNER_PARTS: &[&str] = &[
    "heart", "mind", "thoughts", "nerves", "spirits", "mood", "doubts", "fears", "hopes", "unease",
    "relief",
];

/// The words of `text`, lower case, apostrophes kept inside words.
fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '\u{2019}'))
        .map(|w| w.trim_matches(['\'', '\u{2019}']).replace('\u{2019}', "'"))
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect()
}

/// The quoted spans of `text` (`"…"`, `“…”`, `«…»`), without their marks.
fn quoted(text: &str) -> Vec<String> {
    let pairs = [('"', '"'), ('\u{201c}', '\u{201d}'), ('\u{ab}', '\u{bb}')];
    let mut out = Vec::new();
    let mut open: Option<(char, String)> = None;
    for c in text.chars() {
        match &mut open {
            Some((end, span)) if c == *end => {
                out.push(std::mem::take(span));
                open = None;
            }
            Some((_, span)) => span.push(c),
            None => {
                if let Some((_, end)) = pairs.iter().find(|(o, _)| *o == c) {
                    open = Some((*end, String::new()));
                }
            }
        }
    }
    out
}

/// The word runs of the Intent's speech (see the module comment).
fn speech_runs(intent: &str) -> Vec<Vec<String>> {
    let spans = quoted(intent);
    if !spans.is_empty() {
        return spans.iter().map(|s| words(s)).collect();
    }
    let person = |w: String| -> String {
        match w.as_str() {
            "i" | "me" => "you".into(),
            "my" => "your".into(),
            "mine" => "yours".into(),
            "myself" => "yourself".into(),
            "am" => "are".into(),
            _ => w,
        }
    };
    vec![words(intent).into_iter().skip(1).map(person).collect()]
}

/// True when `narration` repeats `ECHO_MIN_WORDS` or more words in a row of
/// the Intent's speech.
pub fn echoes(intent: &str, narration: &str) -> bool {
    let told = words(narration);
    if told.len() < ECHO_MIN_WORDS {
        return false;
    }
    speech_runs(intent).iter().any(|run| {
        run.windows(ECHO_MIN_WORDS)
            .any(|gram| told.windows(ECHO_MIN_WORDS).any(|t| t == gram))
    })
}

/// The sentences of `text`: split at `.`, `!`, `?` and line ends.
fn sentences(text: &str) -> impl Iterator<Item = &str> {
    text.split(['.', '!', '?', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// True when `sentence` states a feeling or a thought of the PC whose first
/// name is `pc` (see the module comment).
pub fn feeling_sentence(sentence: &str, pc: &str) -> bool {
    let pc = pc.to_lowercase();
    let w = words(sentence);
    w.iter().enumerate().any(|(i, word)| {
        let subject = word == "you" || (!pc.is_empty() && *word == pc);
        let verb_at = |j: usize| {
            w.get(j)
                .is_some_and(|v| FEELING_VERBS.contains(&v.as_str()))
        };
        let helper_at = |j: usize| w.get(j).is_some_and(|v| HELPERS.contains(&v.as_str()));
        let feels = subject && (verb_at(i + 1) || (helper_at(i + 1) && verb_at(i + 2)));
        let inner = word == "your"
            && w.get(i + 1)
                .is_some_and(|p| INNER_PARTS.contains(&p.as_str()));
        feels || inner
    })
}

/// True when some sentence of `narration` states a feeling or a thought of
/// the PC.
pub fn states_pc_feeling(narration: &str, pc: &str) -> bool {
    let first = pc.split_whitespace().next().unwrap_or_default();
    sentences(narration).any(|s| feeling_sentence(s, first))
}

/// The PC's main name in `campaign`, or an empty name.
pub(crate) fn pc_name(
    engine: &threshold_core::turn::Engine,
    campaign: threshold_core::store::CampaignId,
) -> String {
    engine
        .store()
        .snapshot(campaign)
        .ok()
        .and_then(|view| {
            view.entities
                .iter()
                .find(|e| e.kind == threshold_core::domain::EntityKind::Pc)
                .map(|e| e.name.clone())
        })
        .unwrap_or_default()
}

/// The shortest run of words of a quoted line that counts as a repeat of an
/// earlier line (QA polish pass, M5).
pub const REPEAT_MIN_WORDS: usize = 8;

/// Narrations that repeat a line of speech word for word (QA polish pass,
/// M5): a quoted span holds `REPEAT_MIN_WORDS` or more words in a row of a
/// quoted span of an earlier Narration of the same Campaign. Case and
/// punctuation do not count. Call `new_campaign` before each Campaign.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Repeats {
    said: Vec<Vec<String>>,
    pub narrations: usize,
    pub repeats: usize,
}

impl Repeats {
    /// Forgets the lines of the Campaign before; the counts stay.
    pub fn new_campaign(&mut self) {
        self.said.clear();
    }

    pub fn add(&mut self, narration: &str) {
        let spans: Vec<Vec<String>> = quoted(narration).iter().map(|s| words(s)).collect();
        let hit = spans.iter().any(|run| {
            run.windows(REPEAT_MIN_WORDS).any(|gram| {
                self.said
                    .iter()
                    .any(|old| old.windows(REPEAT_MIN_WORDS).any(|o| o == gram))
            })
        });
        self.narrations += 1;
        self.repeats += usize::from(hit);
        self.said.extend(spans);
    }

    /// The report line.
    pub fn line(&self) -> String {
        format!(
            "- Narrations with a line of speech that repeats {REPEAT_MIN_WORDS} or more words in a row of an earlier Narration's speech: {} of {} ({})\n",
            self.repeats,
            self.narrations,
            super::pct(self.repeats, self.narrations)
        )
    }
}

/// The two counts over some Narrations.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EchoCount {
    pub narrations: usize,
    pub echoes: usize,
    pub feelings: usize,
}

impl EchoCount {
    pub fn add(&mut self, intent: &str, narration: &str, pc: &str) {
        self.narrations += 1;
        self.echoes += usize::from(echoes(intent, narration));
        self.feelings += usize::from(states_pc_feeling(narration, pc));
    }

    /// The two report lines.
    pub fn lines(&self) -> String {
        format!(
            "- Narrations that repeat {ECHO_MIN_WORDS} or more words in a row of the Intent's speech: {} of {} ({})\n- Narrations with a sentence that states a feeling or a thought of the PC (word rule): {} of {} ({})\n",
            self.echoes,
            self.narrations,
            super::pct(self.echoes, self.narrations),
            self.feelings,
            self.narrations,
            super::pct(self.feelings, self.narrations)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_said_again_word_for_word_counts_once_per_narration() {
        let mut r = Repeats::default();
        r.add("Hessa: \"Drink up, love, the river has been unkind today.\"");
        r.add("Hessa looks up.\n\nHessa: \"Drink up, love! The river has been unkind today, so sit.\"");
        r.add("Hessa: \"The roll is on the shelf by the door, behind the ledgers.\"");
        assert_eq!((r.repeats, r.narrations), (1, 3));
        // Seven words in a row ("the roll is on the shelf by") are not a
        // repeat.
        r.add("Doul: \"The roll is on the shelf by a window.\"");
        assert_eq!(r.repeats, 1);
        // A new Campaign forgets the lines before.
        r.new_campaign();
        r.add("Hessa: \"Drink up, love, the river has been unkind today.\"");
        assert_eq!((r.repeats, r.narrations), (1, 5));
    }

    #[test]
    fn an_echo_of_quoted_speech_counts() {
        let intent = "I say \"the ink on the third charter is fading fast\" to Orsel.";
        assert!(echoes(
            intent,
            "You tell him the ink on the third charter is fading. He frowns."
        ));
        assert!(!echoes(intent, "Orsel frowns at the charter and sighs."));
    }

    #[test]
    fn an_intent_without_quotes_reads_in_the_second_person() {
        let intent = "I ask Hessa the clerk where I can set up my writing desk.";
        assert!(echoes(
            intent,
            "You ask where you can set up your writing desk, and Hessa points."
        ));
        assert!(!echoes(intent, "Hessa points to a desk by the window."));
    }

    #[test]
    fn a_feeling_of_the_pc_counts_and_an_npc_feeling_does_not() {
        assert!(states_pc_feeling("You feel a wave of relief.", "Wren"));
        assert!(states_pc_feeling("Wren wonders if she lies.", "Wren"));
        assert!(states_pc_feeling("You can sense his doubt.", "Wren"));
        assert!(states_pc_feeling("Your heart beats fast.", "Wren"));
        assert!(!states_pc_feeling("Hessa feels tired. She sighs.", "Wren"));
        assert!(!states_pc_feeling("You ask her and she nods.", "Wren"));
        assert!(!states_pc_feeling(
            "Orsel looks at you. He thinks it over.",
            "Wren"
        ));
        assert!(!states_pc_feeling(
            "You strain, your breath coming in ragged gasps.",
            "Wren"
        ));
    }
}
