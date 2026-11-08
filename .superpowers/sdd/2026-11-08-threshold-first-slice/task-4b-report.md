# Task 4b report

- Renamed the domain types, functions, variables, SQLite column and fact source to Adventure Seed terminology. Renamed the fixture-loading turn kind to `Setup`, serialized as `setup`; the version-1 migration is updated in place.
- Disabled default features for `rand` and `rand_chacha`. The `threshold-core` dependency tree no longer includes `getrandom`.
- Added `golden_rolls_are_stable` and a setup-turn serialization assertion. The golden test passes on its first assertion run by construction: its expected values were recorded from the existing implementation on 2026-11-09, before changing dependency features.
- Golden values at difficulty 6: seed `0` → `7`; `1` → `6`; `42` → `3`; `u64::MAX` → `9`.
- Item 5: `grep -rni charter crates/` produced no output (exit 1, no matches).
- Verification: `cargo fmt --check && cargo clippy --workspace -- -D warnings && cargo test --workspace` passed; 60 tests passed, one live Ollama test ignored, zero failures.
- Concern: the workspace build still pulls `getrandom v0.2.17` through `threshold-ollama` → `ureq` → TLS (`rustls`/`ring`). Removing it would disable or replace existing TLS support, beyond the requested rules dependency change and no-other-behavior-change constraint. `cargo tree -p threshold-core -i getrandom` finds no matching package.
