//! The speech mark: the narrator writes it, `speech::parse` reads it.

use threshold_core::speech::{Line, RULE, parse};

fn tagged<'a>(name: &'a str, words: &'a str, line: &'a str) -> Line<'a> {
    let said = line.split_once(':').unwrap().1.trim();
    Line::Tagged {
        line,
        name,
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
