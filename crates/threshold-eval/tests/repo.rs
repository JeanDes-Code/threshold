//! Repository guards of the mature content module (plan Task 9): the
//! repository holds no Campaign file, no Content Pack but the neutral
//! sample, and the private text scan finds a line of a private pack without
//! printing it. No word list: the scan reads the owner's pack folder, which
//! lives outside the repository.

use std::{
    path::{Path, PathBuf},
    process::Command,
};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// The folders a walk of the work tree skips: build output and installed
/// packages, never tracked.
const SKIPPED: [&str; 4] = ["target", "node_modules", "dist", ".git"];

fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if path.is_dir() {
            if !SKIPPED.contains(&name.as_str()) {
                walk(&path, found);
            }
        } else {
            found.push(path);
        }
    }
}

/// The files the repository tracks or would track: `git ls-files` of the
/// tracked files and of the untracked files that no ignore rule covers, so
/// a file about to be added counts too. Without git, a walk of the work
/// tree that skips `SKIPPED`.
fn repo_files() -> Vec<PathBuf> {
    let root = root();
    let listed = Command::new("git")
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .current_dir(&root)
        .output();
    match listed {
        Ok(out) if out.status.success() => out
            .stdout
            .split(|b| *b == 0)
            .filter(|name| !name.is_empty())
            .map(|name| root.join(String::from_utf8_lossy(name).as_ref()))
            .collect(),
        _ => {
            eprintln!("git is not available: walking the work tree");
            let mut found = Vec::new();
            walk(&root, &mut found);
            found
        }
    }
}

#[test]
fn the_repo_tracks_no_campaign_file() {
    let files = repo_files();
    assert!(files.len() > 50, "the walk found the repository");
    for file in files {
        let name = file.file_name().unwrap().to_string_lossy().into_owned();
        assert!(
            !name.contains(".sqlite") && !name.ends_with(".debug.jsonl"),
            "{} is a Campaign file or a debug dump",
            file.display()
        );
    }
}

#[test]
fn the_repo_tracks_no_other_pack() {
    let packs = root().join("fixtures/packs");
    let mut folders: Vec<String> = std::fs::read_dir(&packs)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    folders.sort();
    assert_eq!(folders, ["test-pack"]);
    // No pack anywhere else: a `pack.toml` lives in the sample folder only.
    for file in repo_files() {
        if file.file_name().is_some_and(|n| n == "pack.toml") {
            assert!(
                file.starts_with(packs.join("test-pack")),
                "{} is a Content Pack outside the sample",
                file.display()
            );
        }
    }
}

fn has(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Runs the scan with `THRESHOLD_PRIVATE_PACK` set to `pack`, or unset.
fn scan(pack: Option<&Path>) -> (i32, String) {
    let mut cmd = Command::new("sh");
    cmd.arg("scripts/qa/private_text_scan.sh")
        .current_dir(root());
    match pack {
        Some(dir) => cmd.env("THRESHOLD_PRIVATE_PACK", dir),
        None => cmd.env_remove("THRESHOLD_PRIVATE_PACK"),
    };
    let out = cmd.output().unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.code().unwrap_or(-1), text)
}

#[test]
fn the_scan_finds_a_fragment_line_and_prints_no_text() {
    // `sh` runs the script (the plan's skip rule); `git` lists the files
    // the scan searches.
    for tool in ["sh", "git"] {
        if !(tool == "sh" && Path::new("/bin/sh").exists() || has(tool)) {
            eprintln!("skipped: {tool} is missing");
            return;
        }
    }
    let base =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("private-scan-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);

    // A pack whose fragment holds a line that a tracked file holds too. The
    // line is read from the file at run time, so no source here holds it.
    let tracked = "docs/adr/0006-speaker-mark.md";
    let line = std::fs::read_to_string(root().join(tracked))
        .unwrap()
        .lines()
        .find(|l| l.trim().chars().count() >= 40)
        .unwrap()
        .trim()
        .to_owned();
    let hit = base.join("hit");
    std::fs::create_dir_all(&hit).unwrap();
    std::fs::write(hit.join("fragment.md"), format!("short\n{line}\n")).unwrap();

    // A pack whose lines no tracked file holds: made at run time too.
    let clean = base.join("clean");
    std::fs::create_dir_all(&clean).unwrap();
    let nowhere: String = format!("{}-{}", std::process::id(), "x".repeat(24))
        .chars()
        .rev()
        .collect();
    std::fs::write(clean.join("fragment.md"), format!("{nowhere}\n")).unwrap();

    let (code, out) = scan(Some(&hit));
    assert_eq!(code, 1, "{out}");
    assert!(out.contains(tracked), "the hit names its file: {out}");
    assert!(!out.contains(&line), "the scan printed the line: {out}");

    let (code, out) = scan(Some(&clean));
    assert_eq!(code, 0, "{out}");

    let (code, out) = scan(None);
    assert_eq!(code, 0, "{out}");
    assert!(out.trim().is_empty(), "{out}");

    std::fs::remove_dir_all(&base).unwrap();
}
