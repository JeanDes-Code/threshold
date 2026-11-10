//! The screen types: each one is made from `PcSight` only.

mod support;

use std::path::{Path, PathBuf};

use support::hidden::*;
use threshold_core::{
    fixture::{load_adventure_seed, load_world},
    store::Store,
    turn::create_campaign,
    view::character_view,
};

/// What `Session::get_character` gave for the town fixture before the view
/// read `PcSight`.
const TOWN_CHARACTER: &str = r#"{"name":"Wren","concept":"An outsider scribe, hired to copy the town's charters","clock_min":0,"facts":[]}"#;

/// What it gave for the hidden world, from `WorldView`.
const HIDDEN_CHARACTER: &str = r#"{"name":"Ada","concept":"Courier","clock_min":5,"facts":[{"text":"Ada plans to meet Brann at the mill.","open":true}]}"#;

fn town() -> (Store, u64) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/town");
    let seed = load_adventure_seed(&dir.join("adventure-seed.toml")).unwrap();
    let mut store = Store::open_in_memory().unwrap();
    let campaign = create_campaign(&mut store, &seed, 11).unwrap();
    load_world(&mut store, campaign, &dir.join("world.toml")).unwrap();
    (store, campaign)
}

#[test]
fn character_view_shows_pc_knowledge_only() {
    let w = HiddenWorld::new();
    let json = serde_json::to_string(&character_view(&w.sight())).unwrap();
    for hidden in HIDDEN {
        assert!(!json.contains(hidden), "{hidden} leaks: {json}");
    }
    assert_eq!(json, HIDDEN_CHARACTER);

    let (store, campaign) = town();
    let json = serde_json::to_string(&character_view(&sight_of(&store, campaign))).unwrap();
    assert_eq!(json, TOWN_CHARACTER);
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

#[test]
fn view_reads_no_world_view() {
    let mut files = Vec::new();
    rust_files(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src/view"),
        &mut files,
    );
    assert!(!files.is_empty());
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        for word in ["WorldView", "Store"] {
            assert!(!text.contains(word), "{} names {word}", file.display());
        }
    }
}
