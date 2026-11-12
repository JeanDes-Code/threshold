# QA on the real app, headless

Every QA run of the desktop app is headless. It opens no window on the desktop, creates no compositor output, and uses no real mouse, keyboard or clipboard. A run that drives the desktop can collide with the person who uses the machine.

## How

The app runs inside Xvfb, an X display that exists only in memory. `WebKitWebDriver` starts the debug binary there, and `wd.py` sends it WebDriver commands and saves screenshots.

```sh
cargo build                                  # the debug binary, target/debug/threshold
export QA_OUT=/path/outside/the/repository   # screenshots, logs, pid files
scripts/qa/headless.sh start                 # Xvfb :97, the UI dev server, WebKitWebDriver :4455
scripts/qa/wd.py new                         # launch the app
scripts/qa/wd.py shot 001-home               # $QA_OUT/001-home.png
scripts/qa/wd.py click 't:New Campaign'
scripts/qa/wd.py quit
scripts/qa/headless.sh stop                  # stops only what start started
```

`wd.py` with no argument lists its commands.

## Rules

- `QA_OUT` is outside the repository. Nothing of a run is committed.
- `start` refuses to run with less than 10 GB of RAM available: the local model then goes to swap and the Turn times measure the machine.
- A cargo build can push the model into swap. After a build, let the model answer once before timing a Turn.
- Stop only the processes the run started.

## Needs

`Xvfb` (Arch package `xorg-server-xvfb`), `WebKitWebDriver` (Arch package `webkitgtk-6.0`), Python 3, and Ollama with the model of `threshold.toml`.

## The private text scan

`private_text_scan.sh` keeps the owner's private Content Pack out of the repository. It reads the pack folder from `THRESHOLD_PRIVATE_PACK` (a relative folder or a link to one works), takes each line of 20 characters or more of the pack's text files, and searches the tracked files and the staged content for it, as a fixed string. The text files are the fragments and the word list: every file but `pack.toml`, whose lines are format and appear in `docs/content-pack-format.md`; a `.git` folder and binary files are left out. A line that the neutral sample pack also holds is format and is left out too. A hit prints the file and the line number, never the text, and the scan exits 1; a file name with a colon prints whole. A tool error (a pack file it cannot read, a failed `git grep`) stops the scan with exit 2 and a plain message that holds no pack text and no pack file name: it never passes in silence. With the variable unset it prints one line, `private_text_scan: THRESHOLD_PRIVATE_PACK is not set; skipped`, and does nothing else, so the scan costs nothing on a machine without the pack. The pre-commit hook runs it. The repository holds no word list: the only list is the pack itself, outside the repository. The scan cannot catch model prose pasted without its pack lines.
