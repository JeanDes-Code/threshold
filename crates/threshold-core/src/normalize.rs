use unicode_normalization::UnicodeNormalization;

/// The one normal form of a name. On top of `value`, it drops the accents
/// (the combining diacritical marks, U+0300 to U+036F, after NFD), so
/// "Chloé", "chloe" and "CHLOÉ" are one name.
pub fn name(input: &str) -> String {
    normalized(input)
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
    let input = input
        .nfkc()
        .collect::<String>()
        .to_lowercase()
        .replace(['\u{2018}', '\u{2019}'], "'");
    let input = input.split_whitespace().collect::<Vec<_>>().join(" ");

    ["the ", "a ", "an "]
        .iter()
        .find_map(|article| input.strip_prefix(article))
        .unwrap_or(&input)
        .to_owned()
}
