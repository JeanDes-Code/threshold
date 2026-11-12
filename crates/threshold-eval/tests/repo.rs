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

/// The scan needs `sh` and a git checkout of the repository: without them
/// a scan test says why it skips, and passes.
fn scan_can_run() -> bool {
    if !Path::new("/bin/sh").exists() && !has("sh") {
        eprintln!("skipped: sh is missing");
        return false;
    }
    let checkout = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(root())
        .output()
        .is_ok_and(|o| o.status.success());
    if !checkout {
        eprintln!("skipped: no git checkout of the repository (or no git)");
    }
    checkout
}

/// A fresh folder outside the repository for one scan test.
fn scratch(case: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("private-scan-{case}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    base.canonicalize().unwrap()
}

/// A line of 40 characters or more of a tracked file, read at run time, so
/// no source here holds it.
fn tracked_line(file: &str) -> String {
    std::fs::read_to_string(root().join(file))
        .unwrap()
        .lines()
        .find(|l| l.trim().chars().count() >= 40)
        .unwrap()
        .trim()
        .to_owned()
}

#[test]
fn the_scan_finds_a_fragment_line_and_prints_no_text() {
    if !scan_can_run() {
        return;
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
    assert_eq!(
        out.trim(),
        "private_text_scan: THRESHOLD_PRIVATE_PACK is not set; skipped"
    );

    std::fs::remove_dir_all(&base).unwrap();
}

/// Review M1 (2026-11-12): the scan reads the pack's text files only. A
/// `.git` folder, a binary file and `pack.toml` that hold a tracked line
/// are left out, and do not stop the scan; a fragment that holds it is a
/// hit. A relative folder, and a folder whose path holds a colon, work.
#[test]
fn the_scan_reads_text_files_only_and_skips_git_and_binary() {
    if !scan_can_run() {
        return;
    }
    let line = tracked_line("docs/adr/0006-speaker-mark.md");
    let base = scratch("text");
    let pack = base.join("pack:one");
    std::fs::create_dir_all(pack.join(".git/objects")).unwrap();
    std::fs::write(pack.join(".git/objects/blob"), format!("{line}\n")).unwrap();
    std::fs::write(pack.join(".git/HEAD"), format!("{line}\n")).unwrap();
    let mut binary = b"\x00\x01binary\n".to_vec();
    binary.extend_from_slice(line.as_bytes());
    binary.extend_from_slice(b"\n\x00");
    std::fs::write(pack.join("image.bin"), binary).unwrap();
    std::fs::write(pack.join("pack.toml"), format!("# {line}\n")).unwrap();
    let (code, out) = scan(Some(&pack));
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains(&line), "{out}");

    std::fs::write(pack.join("words.txt"), format!("{line}\n")).unwrap();
    let (code, out) = scan(Some(&pack));
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("docs/adr/0006-speaker-mark.md:"), "{out}");
    assert!(!out.contains(&line), "{out}");

    // The same folder, named relative to the repository root.
    let up = "../".repeat(root().components().count() - 1);
    let relative = PathBuf::from(up).join(pack.strip_prefix("/").unwrap());
    let (code, out) = scan(Some(&relative));
    assert_eq!(code, 1, "relative: {out}");
    std::fs::remove_dir_all(&base).unwrap();
}

/// Review M1: a tool error stops the scan with a plain message that holds
/// no pack text and no pack file name; it is never a silent pass.
#[cfg(unix)]
#[test]
fn a_tool_error_fails_the_scan_and_prints_no_pack_text() {
    use std::os::unix::fs::PermissionsExt;
    if !scan_can_run() {
        return;
    }
    let base = scratch("error");
    let secret = base.join("secret-fragment-name.md");
    std::fs::write(&secret, "SECRETPACKTEXT and more words of it\n").unwrap();
    std::fs::set_permissions(&secret, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read(&secret).is_ok() {
        eprintln!("skipped: this user reads a file of mode 000");
        std::fs::remove_dir_all(&base).unwrap();
        return;
    }
    let (code, out) = scan(Some(&base));
    std::fs::set_permissions(&secret, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::remove_dir_all(&base).unwrap();
    assert_eq!(code, 2, "{out}");
    assert!(out.starts_with("private_text_scan: "), "{out}");
    for text in ["SECRETPACKTEXT", "secret-fragment-name"] {
        assert!(!out.contains(text), "{out}");
    }
}

/// Review M1: a hit in a file whose path holds a colon prints the whole
/// path and the line number. The scan runs in a small git repository of
/// its own, with no sample pack.
#[test]
fn the_scan_names_a_file_whose_path_holds_a_colon() {
    if !scan_can_run() {
        return;
    }
    let base = scratch("colon");
    let repo = base.join("repo");
    std::fs::create_dir_all(repo.join("dir:x")).unwrap();
    let line = "a plain line of the private pack, for the colon test";
    std::fs::write(repo.join("dir:x/a:b.md"), format!("intro\n{line}\n")).unwrap();
    let git = |args: &[&str]| {
        // Inside a git hook, git exports its own repository to child
        // processes. Without this, `init` and `add` act on the real one.
        Command::new("git")
            .args(args)
            .current_dir(&repo)
            .env_remove("GIT_DIR")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_PREFIX")
            .env_remove("GIT_COMMON_DIR")
            .output()
            .unwrap()
    };
    git(&["init", "-q"]);
    git(&["add", "dir:x/a:b.md"]);
    let pack = base.join("pack");
    std::fs::create_dir_all(&pack).unwrap();
    std::fs::write(pack.join("fragment.md"), format!("{line}\n")).unwrap();
    // The scan's own git calls must read the small repository too: in a
    // hook of a linked worktree, git exports GIT_DIR and GIT_INDEX_FILE,
    // and the scan then read the real index.
    let out = Command::new("sh")
        .arg(root().join("scripts/qa/private_text_scan.sh"))
        .env("THRESHOLD_PRIVATE_PACK", &pack)
        .current_dir(&repo)
        .env_remove("GIT_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_PREFIX")
        .env_remove("GIT_COMMON_DIR")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stderr).into_owned();
    std::fs::remove_dir_all(&base).unwrap();
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains("staged: dir:x/a:b.md:2"), "{text}");
    assert!(text.contains("work tree: dir:x/a:b.md:2"), "{text}");
    assert!(!text.contains(line), "{text}");
}

/// Review M2 (2026-11-12): a pack built exactly from the example of
/// `docs/content-pack-format.md` (its `pack.toml` block and the files it
/// names) loads, and the scan lets every commit through: `pack.toml` is not
/// scanned, so the lines it shares with the format page are no hit.
#[test]
fn a_pack_from_the_format_page_example_loads_and_passes_the_scan() {
    let page = std::fs::read_to_string(root().join("docs/content-pack-format.md")).unwrap();
    let start = page.find("## `pack.toml`").unwrap();
    let block = &page[start..];
    let toml = &block[block.find("```toml\n").unwrap() + 8..];
    let toml = &toml[..toml.find("```").unwrap()];
    let base = scratch("example");
    let pack = base.join("my-pack");
    std::fs::create_dir_all(&pack).unwrap();
    std::fs::write(pack.join("pack.toml"), toml).unwrap();
    // The files the example names, with lines made at run time.
    let made: String = format!("{}-{}", std::process::id(), "y".repeat(24));
    for line in toml.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key == "file" || key == "intimate_words" {
            let name = value.split('"').nth(1).unwrap();
            let text = if key == "file" {
                format!("Keep each moment short and plain, {made}.\n")
            } else {
                "glimmerwick\n".to_owned()
            };
            std::fs::write(pack.join(name), text).unwrap();
        }
    }
    let (packs, problems) = threshold_core::pack::load_all(&base);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(packs.len(), 1);
    assert_eq!(packs[0].intimate_words.len(), 1);
    if scan_can_run() {
        let (code, out) = scan(Some(&pack));
        assert_eq!(code, 0, "{out}");
    }
    std::fs::remove_dir_all(&base).unwrap();
}
