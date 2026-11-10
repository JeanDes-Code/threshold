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
