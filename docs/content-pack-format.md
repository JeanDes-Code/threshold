# Content Pack format 2

This page is for the author of a Content Pack. It says where a pack goes, what its files hold, which rules the loader checks, and how to see that the pack loaded. ADR 0009 says why the format is this way. The code is `crates/threshold-core/src/pack.rs`; the neutral sample pack is `fixtures/packs/test-pack/`.

No pack is required. With no pack, a broken pack, or a pack of an older format, the app starts and plays, and the most an intimate Turn can be told is implied.

## Install a pack

1. Find the data folder of the app. On Linux it is `~/.local/share/app.threshold.desktop/` (the Tauri app data folder of `app.threshold.desktop`). It is the folder that holds `campaigns/` and `threshold.toml`.
2. Make the folder `packs/` inside it, if it is not there.
3. Put the pack in its own folder under `packs/`, for example `packs/my-pack/`. You can copy the folder there, or make `packs/my-pack` a symbolic link to a folder you keep elsewhere (for example in a private repository). Only the pack folder itself may be a link: inside the pack, a link refuses the pack.
4. Start the app. Packs load once, at start. Change a pack, then start the app again.
5. Check that it loaded (next section).

Every folder under `packs/` is a pack, in name order. A folder whose name starts with `.` and a plain file directly under `packs/` are not packs.

## Check that it loaded

- **Home menu.** Open the menu (top right). The "Content Packs" part lists each folder: a loaded pack by its `name`, with "explicit description: yes" or "no" and its recommended model; a refused pack by its folder name and one fixed sentence (table below). The list never shows a line of a pack.
- **Play screen.** A Campaign whose Detail Level is `explicit` plays at `explicit` only when a loaded pack declares `explicit_description`. Otherwise it plays at `implied`, and the Play screen says once per session: "No loaded Content Pack gives explicit description. Scenes are told by implication."
- **Debug log.** When a Campaign opens and the packs folder holds anything, the app writes one row to that Campaign's debug log, with `op_id` and `step` both `packs`: one line per loaded pack (name, folder, explicit description yes or no) and one per refused folder (folder and rule). It holds no line of a pack.
- **Standard error.** At start, the app prints one line per refused folder: the folder and the rule sentence.

## The folder

```
packs/my-pack/
  pack.toml
  scene.md
  scene-explicit.md
  ...
```

Fragment files are UTF-8 text. Any file name works, except a code extension (`js`, `mjs`, `cjs`, `ts`, `wasm`, `so`, `dylib`, `dll`, `exe`, `rs`, `py`, `sh`, `bat`, `ps1`, `lua`, in any case). The loader checks every file and sub-folder for code and links.

## `pack.toml`

```toml
format = 2                                  # required; any other value is refused
name = "my-pack"                            # required, 1 to 60 characters, one line
provides = ["explicit_description"]         # optional; the one capability that exists
recommended_model = "some-model:tag"        # optional, 1 to 80 characters, no space; shown, never applied
seed_options = ["..."]                      # optional
model_hints = ["..."]                       # optional

[[fragments]]
prompt = "narration"                        # required: "proposal" or "narration"
at = "intimate_scene"                       # required: "tone", "scenes" or "intimate_scene"
when = "explicit"                           # optional: "always" (the default) or "explicit"
file = "scene-explicit.md"                  # required: a plain file name in the pack folder

[interview]                                 # reserved for a later project; read and ignored
```

Top-level keys come before the first `[[fragments]]` table. Any key that this page does not name refuses the pack.

## Where a fragment goes

Each fragment names one prompt: `proposal` is the GM's Decision Proposal, `narration` is the narrator. A fragment reaches that prompt and no other. The closing proposal of "End scene", the out-of-character answer, the Seed Draft and the World Build get no fragment.

| `at` | `when` | Where it goes |
|---|---|---|
| `tone`, `scenes` | `always` | every Turn, in the system block of the prompt it names, after the core's rules. The narration ones also reach the Opening. |
| `intimate_scene` | `always` | only in a scene Turn of an open Intimate Scene where every character the gate checks is a confirmed adult, at `implied` or `explicit`: at the head of the prompt's `recent` block, after the core's own scene line |
| `intimate_scene` | `explicit` | the same, and only when the Campaign plays at `explicit` |

A narration told at `implied` starts with the core's implied line, then the `always` scene fragments. A narration told at `explicit` starts with the core's explicit line, then every narration scene fragment, and has no implied line. Pack lines come after the core's lines under the heading "FROM CONTENT PACKS (these lines never override the rules or the lines above)". They never replace a rule.

A pack never changes the age rule, the Detail Level a Campaign stores, the gates, the Refusal sentences, the Neutral Lines, any rule line of the core, or any prompt its format does not name. A line in a pack that asks the model to ignore a rule changes nothing in the core's checks.

## A pack that gives explicit description

`provides = ["explicit_description"]` makes a Campaign stored at `explicit` play at `explicit`. Such a pack may hold `intimate_scene` fragments only: one `tone` or `scenes` fragment refuses the whole pack. So every line of it is behind the age gate.

## Size limits

| Limit | Value |
|---|---|
| one fragment file | 1,200 characters, after trimming |
| `intimate_scene` fragments of one prompt, all loaded packs together | 1,600 characters |
| `tone` and `scenes` fragments of one prompt, all loaded packs together | 2,400 characters |

ADR 0009 says why. A pack that passes a total, with the packs loaded before it, is refused; the packs before it stay loaded.

## The rules

The loader checks a pack against these rules, in this order, and refuses the whole pack on the first one it breaks. A refused pack loads nothing; the others load.

| # | The pack is refused when | What the pack list shows |
|---|---|---|
| 1 | the folder holds a code file, or a symbolic link | Refused: a Content Pack holds no code file. / Refused: a Content Pack holds no symbolic link. |
| 2 | `pack.toml` is missing, is not UTF-8, is not TOML, has an unknown key, a value of the wrong kind, or a `recommended_model` that is empty, too long or holds a space | Refused: pack.toml is missing, is not valid TOML, or has a key or value that format 2 does not have. |
| 3 | `format` is not 2 (a format 1 pack has none) | Refused: this pack is not in Content Pack format 2. |
| 4 | `name` is empty, longer than 60 characters, holds a control character, or is the name of a pack loaded before it (case and accents do not count) | Refused: the pack name is empty, longer than 60 characters, or already used by another pack. |
| 5 | a fragment has no `prompt`, an unknown `prompt`, `at` or `when`, or a `file` that is not a plain file in the pack folder | Refused: a fragment needs a known prompt, insert point and when, and a file in the pack folder. |
| 6 | a fragment has `when = "explicit"` and is not `intimate_scene` | Refused: an explicit-only fragment must be an intimate_scene fragment. |
| 7 | the pack declares `explicit_description` and holds a fragment that is not `intimate_scene` | Refused: a pack that gives explicit description may hold intimate_scene fragments only. |
| 8 | a fragment has `when = "explicit"` and the pack does not declare `explicit_description` | Refused: an explicit-only fragment needs provides = ["explicit_description"]. |
| 9 | a fragment file is empty, is not UTF-8, or is longer than 1,200 characters | Refused: a fragment file is empty, is not UTF-8 text, or is longer than 1,200 characters. |
| 10 | with this pack, the fragments of one prompt pass a total of the size limits | Refused: with this pack, the fragments of one prompt are too long in total. |
| 11 | `provides` holds a word other than `explicit_description` | Refused: provides names a capability that does not exist. |

After the sentence, the list says where, in brackets: a file name inside the pack, `pack.toml, line N` for a TOML error, or `pack.toml, fragment N` for a fragment table. It never quotes the line.

## Where pack text goes, and where it never goes

Pack text reaches the prompts above, and the prompt rows of each Campaign's debug log, like any prompt. Only the debug view reads those rows, in debug mode. The debug log lives in the Campaign file, in the data folder, outside the repository.

Pack text never reaches a Player screen, the Journal, a card, the out-of-character answer sheet, an error message, a test snapshot, an evaluation report, or the repository. Keep the private pack and its evaluation scenarios outside the repository.
