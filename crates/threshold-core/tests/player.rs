//! Notes and Pins: the Player's marks, outside the world (data shape 3.7).

use std::path::{Path, PathBuf};

use threshold_core::{
    domain::{AdventureSeed, PcSeed, StartSeed},
    player::{Marks, NOTE_MAX_CHARS, PlayerError, PlayerStore, Target},
    store::Store,
};

fn adventure_seed() -> AdventureSeed {
    AdventureSeed {
        premise: "A frontier".into(),
        entities: vec![],
        pc: PcSeed {
            name: "Ada".into(),
            age: 30,
            other_names: vec![],
            concept: "Scout".into(),
        },
        boundaries: vec![],
        decided: vec![],
        proposed: vec![],
        story_language: "en".into(),
        themes: vec![],
        gm_agreement: String::new(),
        resolution: String::new(),
        min_age: None,
        start: StartSeed::default(),
        open_details: vec![],
        detail: Default::default(),
    }
}

/// A Campaign file in its own folder under the target dir, and its Store.
fn campaign_file(name: &str) -> (PathBuf, Store, u64) {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("player-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("campaign.sqlite");
    let mut store = Store::open(&path).unwrap();
    let campaign = store.create_campaign(&adventure_seed(), 7).unwrap();
    (path, store, campaign)
}

fn rust_files(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
}

/// Shape 3.7, proof step 3: no module of the Turn can read a Note, because
/// none of them names the Player store or its tables.
#[test]
fn no_turn_module_names_the_player_store() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = vec![src.join("turn.rs")];
    for dir in ["context", "checks", "world", "build"] {
        rust_files(&src.join(dir), &mut files);
    }
    assert!(files.len() > 10, "the scan found too few files: {files:?}");
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        for word in [
            "PlayerStore",
            "Marks",
            "player::",
            "player_note",
            "player_pin",
        ] {
            assert!(!text.contains(word), "{} names {word}", file.display());
        }
    }
}

#[test]
fn rebuild_keeps_notes_and_pins() {
    let (path, mut store, campaign) = campaign_file("rebuild");
    let player = PlayerStore::open(&path).unwrap();
    let note = player
        .add_note(
            "Ask about the ford.",
            Some(Target::Entity { entity_id: 1 }),
            10,
        )
        .unwrap();
    player
        .set_pin(&Target::Note { note_id: note.id }, true)
        .unwrap();
    player.set_pin(&Target::Scene { turn_id: 1 }, true).unwrap();
    let before = player.marks().unwrap();
    assert_eq!(before.notes.len(), 1);
    assert_eq!(before.pins.len(), 2);

    store.rebuild_projections(campaign).unwrap();

    assert_eq!(player.marks().unwrap(), before);
    drop(player);
    assert_eq!(PlayerStore::open(&path).unwrap().marks().unwrap(), before);
}

/// Review Focus 4: the edges of a Note body. Each failure writes nothing.
#[test]
fn note_body_rules() {
    let (path, _store, _campaign) = campaign_file("body");
    let player = PlayerStore::open(&path).unwrap();
    let kept = player.add_note("First.", None, 1).unwrap();
    let marks = player.marks().unwrap();
    let unchanged = |player: &PlayerStore| assert_eq!(player.marks().unwrap(), marks);

    assert_eq!(player.add_note("", None, 2), Err(PlayerError::Empty));
    unchanged(&player);
    assert_eq!(
        player.add_note("   \n\t ", None, 2),
        Err(PlayerError::Empty)
    );
    unchanged(&player);
    let too_long = "a".repeat(NOTE_MAX_CHARS + 1);
    assert_eq!(NOTE_MAX_CHARS, 2_000);
    assert_eq!(
        player.add_note(&too_long, None, 2),
        Err(PlayerError::TooLong { max: 2_000 })
    );
    unchanged(&player);
    let about_a_note = Some(Target::Note { note_id: kept.id });
    assert_eq!(
        player.add_note("About a note.", about_a_note, 2),
        Err(PlayerError::AboutANote)
    );
    unchanged(&player);
    assert_eq!(player.edit_note(kept.id, " ", 2), Err(PlayerError::Empty));
    unchanged(&player);
    assert_eq!(
        player.edit_note(kept.id, &too_long, 2),
        Err(PlayerError::TooLong { max: 2_000 })
    );
    unchanged(&player);
    assert_eq!(
        player.edit_note(999, "Elsewhere.", 2),
        Err(PlayerError::NoSuchNote(999))
    );
    unchanged(&player);

    // Exactly the limit passes, in letters and in emoji: a char is one.
    let letters = player.add_note(&"a".repeat(2_000), None, 3).unwrap();
    assert_eq!(letters.body.chars().count(), 2_000);
    let emoji = "\u{1F56F}".repeat(2_000);
    assert!(emoji.len() > 2_000);
    let candles = player.add_note(&emoji, None, 3).unwrap();
    assert_eq!(candles.body, emoji);
    assert_eq!(
        player.add_note(&format!("{emoji}\u{1F56F}"), None, 3),
        Err(PlayerError::TooLong { max: 2_000 })
    );

    // A body is stored trimmed; an edit keeps the target and the creation time.
    let about = Some(Target::Entity { entity_id: 3 });
    let note = player
        .add_note("  Orsel lies.  ", about.clone(), 4)
        .unwrap();
    assert_eq!(note.body, "Orsel lies.");
    let edited = player.edit_note(note.id, "Orsel lied twice.", 9).unwrap();
    assert_eq!(edited.about, about);
    assert_eq!((edited.created_ms, edited.updated_ms), (4, 9));
}

#[test]
fn delete_note_removes_its_pin() {
    let (path, _store, _campaign) = campaign_file("delete");
    let player = PlayerStore::open(&path).unwrap();
    let gone = player.add_note("Gone soon.", None, 1).unwrap();
    let stays = player.add_note("Stays.", None, 1).unwrap();
    for note in [&gone, &stays] {
        player
            .set_pin(&Target::Note { note_id: note.id }, true)
            .unwrap();
    }
    player.delete_note(gone.id).unwrap();
    assert_eq!(
        player.marks().unwrap(),
        Marks {
            notes: vec![stays.clone()],
            pins: vec![Target::Note { note_id: stays.id }],
        }
    );
    assert_eq!(
        player.delete_note(gone.id),
        Err(PlayerError::NoSuchNote(gone.id))
    );
    assert_eq!(
        player.set_pin(&Target::Note { note_id: gone.id }, true),
        Err(PlayerError::NoSuchNote(gone.id))
    );
}

/// A Pin target is the table key, so a Belief key is stored in the one form
/// the store writes keys: "Door" and "door" are one Pin.
#[test]
fn belief_target_key_is_normalized() {
    let (path, _store, _campaign) = campaign_file("belief-key");
    let player = PlayerStore::open(&path).unwrap();
    let belief = |key: &str| Target::Belief {
        subject_id: 2,
        key: key.into(),
    };
    player.set_pin(&belief("Door"), true).unwrap();
    player.set_pin(&belief("door"), true).unwrap();
    player.set_pin(&belief("  DOOR "), true).unwrap();
    assert_eq!(player.marks().unwrap().pins, [belief("door")]);
    let note = player
        .add_note("Locked at night.", Some(belief("Pending Trip")), 1)
        .unwrap();
    assert_eq!(note.about, Some(belief("pending_trip")));
    player.set_pin(&belief("DOOR"), false).unwrap();
    assert!(player.marks().unwrap().pins.is_empty());
}

#[test]
fn player_store_opens_only_a_campaign_file() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("player-foreign");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    assert!(PlayerStore::open(&dir.join("missing.sqlite")).is_err());
    let foreign = dir.join("foreign.sqlite");
    rusqlite::Connection::open(&foreign)
        .unwrap()
        .execute_batch("CREATE TABLE other (id INTEGER);")
        .unwrap();
    assert!(PlayerStore::open(&foreign).is_err());
}
