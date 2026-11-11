use unicode_normalization::UnicodeNormalization;

/// The one normal form of a name. On top of `value`, it drops the accents
/// (the combining diacritical marks, U+0300 to U+036F, after NFD), so
/// "Chloé", "chloe" and "CHLOÉ" are one name; and it drops the punctuation
/// around the name, so "Ostley Crossing." and "ostley crossing" are one
/// name. Inner apostrophes and hyphens stay: "O'Brien", "Jean-Luc".
pub fn name(input: &str) -> String {
    let lowered = lowered(input);
    let trimmed = lowered.trim_matches(edge);
    let collapsed = collapse(trimmed);
    without_article(&collapsed)
        .trim_matches(edge)
        .nfd()
        .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
        .nfc()
        .collect()
}

/// The words of `text` for a whole-word name match: lower case, the
/// accents dropped as `name` drops them, split on every character that is
/// not a letter or a digit. "Chloé's" gives `["chloe", "s"]`.
pub fn name_words(text: &str) -> Vec<String> {
    lowered(text)
        .nfd()
        .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
        .nfc()
        .collect::<String>()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The words of `text` as `name_words` splits them, accents dropped, but
/// with their case kept: the input of a name match in prose
/// (`name_in`). "Chloé's" gives `["Chloe", "s"]`.
pub fn cased_words(text: &str) -> Vec<String> {
    text.nfkc()
        .collect::<String>()
        .replace(['\u{2018}', '\u{2019}'], "'")
        .nfd()
        .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
        .nfc()
        .collect::<String>()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The words a prose text must hold to name `name` (`name_in`): its
/// `cased_words`, a leading article left out ("The Circle" gives
/// `["Circle"]`). Empty for a name that is only an article.
pub fn name_pattern(name: &str) -> Vec<String> {
    let mut words = cased_words(name);
    if words.len() > 1 && ["the", "a", "an"].contains(&words[0].to_lowercase().as_str()) {
        words.remove(0);
    }
    words
}

/// True when the prose word `word` is the name word `pattern`: the same
/// word whatever the case, and a capital wherever the name has one. "Gate"
/// is not "gate"; "Gate" is "GATE"; "mill" is "Mill".
pub fn same_name_word(pattern: &str, word: &str) -> bool {
    pattern.chars().count() == word.chars().count()
        && pattern.to_lowercase() == word.to_lowercase()
        && pattern
            .chars()
            .zip(word.chars())
            .all(|(p, w)| !p.is_uppercase() || w.is_uppercase())
}

/// True when the prose words `text` (`cased_words`) hold the name words
/// `pattern` (`name_pattern`) as a run of whole words (review B1, bug pass
/// 3). A name in prose keeps its own capitals, so the place "Gate" is not
/// named by "the gate square", nor "The Circle" by "a circle of lamplight".
pub fn name_in(text: &[String], pattern: &[String]) -> bool {
    !pattern.is_empty()
        && text.windows(pattern.len()).any(|run| {
            run.iter()
                .zip(pattern)
                .all(|(word, p)| same_name_word(p, word))
        })
}

pub fn key(input: &str) -> String {
    normalized(input).replace(' ', "_")
}

pub fn value(input: &str) -> String {
    normalized(input)
}

fn normalized(input: &str) -> String {
    without_article(&collapse(&lowered(input))).to_owned()
}

/// NFKC, lower case, and one form of the apostrophe.
fn lowered(input: &str) -> String {
    input
        .nfkc()
        .collect::<String>()
        .to_lowercase()
        .replace(['\u{2018}', '\u{2019}'], "'")
}

/// One space between words, none at the ends.
fn collapse(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn without_article(input: &str) -> &str {
    ["the ", "a ", "an "]
        .iter()
        .find_map(|article| input.strip_prefix(article))
        .unwrap_or(input)
}

/// A character that a name drops at its ends: punctuation and spaces.
fn edge(c: char) -> bool {
    !c.is_alphanumeric()
}
