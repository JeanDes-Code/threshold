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
