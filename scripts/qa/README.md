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
