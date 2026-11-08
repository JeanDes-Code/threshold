---
status: accepted
date: 2026-11-08
---

# Tauri desktop app with a Rust core and a React UI

Threshold is a Tauri 2 desktop app. The Turn pipeline, Ruleset, World Clock, store (`rusqlite`, SQLite bundled) and the Ollama client are Rust. The UI is React + TypeScript in the webview. Narration streams to it through Tauri events. The reasons: the app will ship to other people as a local install, and the author wants a real Rust project. Rust does not make Turns faster, because the model takes seconds and the app code takes milliseconds.

## Considered options

- **TypeScript everywhere (Node core).** Code moves for free to Arcanum Solo and GM Trainer, which are TS, and the SNEQ parts copy without a port. Rejected because shipping a Node core inside a desktop app needs a sidecar process or Electron.
- **Rust for the UI too (Leptos, egui).** Rejected: the author is fluent in React, and the UI is not where the risk is.

## Consequences

- The SNEQ parts named in ADR 0003 are ported to Rust, not copied.
- Sharing with Arcanum Solo covers what has no language: prompts, JSON schemas, evaluation scenarios, design.
- The JSON Schema sent to Ollama's `format` field is generated from the Rust types the core validates with (`schemars`), so the schema and the validator cannot drift apart.
