//! The speech mark: the narrator writes it, `speech::parse` reads it.

use std::collections::HashSet;

use threshold_core::speech::{Line, RULE, TAG_HOLD_CHARS, TagFilter, parse, strip_tags};

fn tagged<'a>(name: &'a str, words: &'a str, line: &'a str) -> Line<'a> {
    let said = line.split_once(':').unwrap().1.trim();
    Line::Tagged {
        line,
        name,
        note: None,
        words,
        said,
    }
}

#[test]
fn parse_reads_a_full_speech_line() {
    let line = r#"Orsel: "You are late.""#;
    assert_eq!(parse(line), [tagged("Orsel", "You are late.", line)]);

    let text = "The rain stops.\n\nOrsel: \"You are late.\"\n\nHe turns away.";
    assert_eq!(
        parse(text),
        [
            Line::Prose("The rain stops."),
            tagged("Orsel", "You are late.", "Orsel: \"You are late.\""),
            Line::Prose("He turns away."),
        ]
    );
}

/// Review Focus 2: three quote styles are marks.
#[test]
fn parse_accepts_three_quote_styles() {
    for (line, words) in [
        (r#"Orsel: "Late.""#, "Late."),
        ("Orsel: \u{201c}Late.\u{201d}", "Late."),
        ("Orsel: \u{ab}Late.\u{bb}", "Late."),
        ("Orsel : \u{ab}\u{a0}En retard.\u{a0}\u{bb}", "En retard."),
        ("  Old Brann Hale: \"Come in.\"  ", "Come in."),
    ] {
        let parsed = parse(line);
        assert_eq!(parsed.len(), 1, "{line}");
        let Line::Tagged {
            name, words: got, ..
        } = parsed[0]
        else {
            panic!("{line} is not a mark: {parsed:?}");
        };
        assert!(name == "Orsel" || name == "Old Brann Hale", "{name}");
        assert_eq!(got, words, "{line}");
    }
}

/// A mark keeps the spoken words with their quote marks, for a line that
/// must lose its tag (ADR 0006).
#[test]
fn parse_keeps_the_quoted_speech_with_its_marks() {
    for (line, said) in [
        (r#"Orsel: "Late.""#, r#""Late.""#),
        ("Orsel: \u{201c}Late.\u{201d}", "\u{201c}Late.\u{201d}"),
        (
            "Orsel : \u{ab}\u{a0}En retard.\u{a0}\u{bb}  ",
            "\u{ab}\u{a0}En retard.\u{a0}\u{bb}",
        ),
    ] {
        let parsed = parse(line);
        let [Line::Tagged { said: got, .. }] = parsed[..] else {
            panic!("{line} is not a mark: {parsed:?}");
        };
        assert_eq!(got, said, "{line}");
    }
}

/// Review Focus 2: a mark is a whole line. Anything else is prose, with its
/// text unchanged.
#[test]
fn parse_keeps_a_mark_inside_a_paragraph_as_prose() {
    for line in [
        r#"Orsel looks up and says: "You are late." Then he turns away."#,
        r#"He mutters, "Not with the tide this low.""#,
        r#"Orsel: "You are late," he says."#,
        r#"Orsel: "You are late." he says."#,
        "Orsel: no quotes at all",
        r#"The old gate guard of Ostley: "Halt.""#,
        r#"Orsel: "Late." "Very late.""#,
        "Orsel: \"Late.\u{201d}",
        r#"Orsel: """#,
        r#": "Late.""#,
    ] {
        assert_eq!(parse(line), [Line::Prose(line)], "{line}");
    }
}

#[test]
fn parse_drops_blank_lines_only() {
    let text = "First.\r\n   \r\nSecond line\nThird line";
    assert_eq!(
        parse(text),
        [
            Line::Prose("First."),
            Line::Prose("Second line"),
            Line::Prose("Third line"),
        ]
    );
    assert!(parse("").is_empty());
}

#[test]
fn rule_is_one_line() {
    assert!(!RULE.contains('\n'));
    assert!(!RULE.is_empty());
}

// ---- The stream filter (data shape 3.1, changed after review) ----------------

/// The names the PC cannot use: Kestrel, and the hidden other name of a
/// toll keeper.
fn hidden() -> HashSet<String> {
    ["kestrel", "toll-wolf"].map(String::from).into()
}

const STORED: &str = "The hood hides a face.\n\nKestrel: \"Nobody knows my name.\"\n\nOrsel: \"You are late.\"\nThe Toll-Wolf: \u{ab}Pay the toll.\u{bb}\nKESTREL : \"Again.\"\nThunder: \"Boom.\"";

/// `STORED` with the tags of the unknown names removed.
const SHOWN: &str = "The hood hides a face.\n\n\"Nobody knows my name.\"\n\nOrsel: \"You are late.\"\n\u{ab}Pay the toll.\u{bb}\n\"Again.\"\nThunder: \"Boom.\"";

/// Feeds `chunks` one by one and joins what the filter emits.
fn stream(chunks: &[&str]) -> String {
    let mut filter = TagFilter::new(hidden());
    let mut out: String = chunks.iter().map(|c| filter.push(c)).collect();
    out.push_str(&filter.finish());
    out
}

fn split_at_chars(text: &str, size: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    chars.chunks(size).map(|c| c.iter().collect()).collect()
}

#[test]
fn stream_drops_the_tag_of_a_name_the_pc_cannot_use() {
    assert_eq!(strip_tags(STORED, &hidden()), SHOWN);
    assert_eq!(stream(&[STORED]), SHOWN);
}

#[test]
fn stream_gives_the_same_text_for_any_chunk_boundary() {
    // One character per chunk, and every other chunk size.
    for size in 1..=STORED.chars().count() {
        let chunks = split_at_chars(STORED, size);
        let chunks: Vec<&str> = chunks.iter().map(String::as_str).collect();
        assert_eq!(stream(&chunks), SHOWN, "chunks of {size}");
    }
    // Boundaries inside the tag, and right at the colon.
    assert_eq!(stream(&["Kes", "trel", ":", " \"Hi.\"\n"]), "\"Hi.\"\n");
    assert_eq!(stream(&["Kestrel", ": \"Hi.\""]), "\"Hi.\"");
    assert_eq!(stream(&["Kestrel:", " ", " \"Hi.\""]), "\"Hi.\"");
}

#[test]
fn stream_keeps_the_tag_of_a_name_the_pc_can_use() {
    let mut filter = TagFilter::new(hidden());
    assert_eq!(filter.push("Orsel"), "");
    assert_eq!(
        filter.push(": \"You are late.\""),
        "Orsel: \"You are late.\""
    );
    assert_eq!(filter.finish(), "");
    // A colon in prose after the line start passes at once.
    let mut filter = TagFilter::new(hidden());
    assert_eq!(
        filter.push("Kestrel waits: then she"),
        "Kestrel waits: then she"
    );
    assert_eq!(filter.push(" goes."), " goes.");
}

#[test]
fn a_line_with_no_colon_waits_for_no_more_than_the_hold() {
    assert_eq!(TAG_HOLD_CHARS, 48);
    let mut filter = TagFilter::new(hidden());
    let long = "a".repeat(TAG_HOLD_CHARS - 1);
    assert_eq!(filter.push(&long), "");
    // The 48th character releases the line: it cannot be a tag any more.
    assert_eq!(filter.push("b"), format!("{long}b"));
    assert_eq!(filter.push("Kestrel: \"Hi.\""), "Kestrel: \"Hi.\"");
    // A newline releases a short line, and the next line is held again.
    assert_eq!(filter.push("\nShort line\nKest"), "\nShort line\n");
    assert_eq!(filter.push("rel: \"Hi.\""), "\"Hi.\"");
    // The end of the stream releases what is held.
    let mut filter = TagFilter::new(hidden());
    assert_eq!(filter.push("The end"), "");
    assert_eq!(filter.finish(), "The end");
}

#[test]
fn stream_with_no_hidden_name_is_unchanged() {
    let mut filter = TagFilter::new(HashSet::new());
    let mut out: String = split_at_chars(STORED, 3)
        .iter()
        .map(|c| filter.push(c))
        .collect();
    out.push_str(&filter.finish());
    assert_eq!(out, STORED);
}

/// Bug pass 4: a short stage note between the tag and the quote is still a
/// mark. Review minor: the note is kept, apart from the words, and `said`
/// holds the note and the quote, as the stream shows them.
#[test]
fn parse_keeps_a_short_stage_note_before_the_quote() {
    let line = "Orsel: (wiping the bar) \"You are late.\"";
    assert_eq!(
        parse(line),
        [Line::Tagged {
            line,
            name: "Orsel",
            note: Some("wiping the bar"),
            words: "You are late.",
            said: "(wiping the bar) \"You are late.\"",
        }]
    );
    // A long note, a note with a quote, or one that never closes: prose.
    let long = format!("Orsel: ({}) \"Late.\"", "slowly ".repeat(10));
    for text in [
        long.as_str(),
        "Orsel: (says \"no\") \"Late.\"",
        "Orsel: (wiping the bar \"Late.\"",
    ] {
        assert_eq!(parse(text), [Line::Prose(text)], "{text}");
    }
}
