use threshold_core::{Gm, Prompt, RecordedGm, Stream};

#[test]
fn recorded_gm_returns_responses_in_order() {
    let gm = RecordedGm::new(
        vec!["proposal one".into(), "proposal two".into()],
        vec!["narration one".into(), "narration two".into()],
    );
    let prompt = Prompt::default();

    assert_eq!(
        gm.propose(&prompt, &serde_json::Value::Null).unwrap(),
        "proposal one"
    );
    assert_eq!(
        gm.propose(&prompt, &serde_json::Value::Null).unwrap(),
        "proposal two"
    );

    let mut chunks = Vec::new();
    assert_eq!(
        gm.narrate(&prompt, None, &mut |chunk| {
            chunks.push(chunk.to_owned());
            Stream::More
        })
        .unwrap(),
        "narration one"
    );
    assert_eq!(chunks, ["narration one"]);
    assert_eq!(
        gm.narrate(&prompt, None, &mut |_| Stream::More).unwrap(),
        "narration two"
    );
}

/// Task 3: the reader of a stream can stop it. A chunked `RecordedGm`
/// streams word by word; after a `Stop` it sends no more chunk and returns
/// the text received so far.
#[test]
fn a_stop_returns_the_text_so_far() {
    let gm = RecordedGm::new(vec![], vec!["One two three four five.".into()]).chunked();
    let mut chunks = Vec::new();
    let text = gm
        .narrate(&Prompt::default(), None, &mut |chunk| {
            chunks.push(chunk.to_owned());
            if chunks.len() == 3 {
                Stream::Stop
            } else {
                Stream::More
            }
        })
        .unwrap();
    assert_eq!(chunks, ["One", " two", " three"]);
    assert_eq!(text, "One two three");
}

/// A chunked stream read to the end gives the whole text, chunk by chunk.
#[test]
fn a_chunked_stream_read_to_the_end_is_the_whole_text() {
    let text = "The gate  opens.\n\nRain falls.";
    let gm = RecordedGm::new(vec![], vec![text.into()]).chunked();
    let mut chunks = Vec::new();
    let answer = gm
        .narrate(&Prompt::default(), None, &mut |chunk| {
            chunks.push(chunk.to_owned());
            Stream::More
        })
        .unwrap();
    assert_eq!(answer, text);
    assert_eq!(chunks.concat(), text);
    assert_eq!(chunks.len(), 5, "{chunks:?}");
}

#[test]
fn recorded_calls_keep_the_token_cap() {
    let gm = RecordedGm::new(vec!["{}".into()], vec!["one".into(), "two".into()]);
    let prompt = Prompt::default();
    gm.propose(&prompt, &serde_json::Value::Null).unwrap();
    gm.narrate(&prompt, Some(280), &mut |_| Stream::More)
        .unwrap();
    gm.narrate(&prompt, None, &mut |_| Stream::More).unwrap();
    let caps: Vec<Option<u32>> = gm.calls().iter().map(|c| c.max_tokens).collect();
    assert_eq!(caps, [None, Some(280), None]);
}
