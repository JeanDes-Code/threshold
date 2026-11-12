# Threshold: notes for coding agents

## Before calling work done

Run both lines and show the output.

```sh
cargo fmt --check && cargo clippy --workspace -- -D warnings && cargo test --workspace
npm --prefix ui run typecheck && npm --prefix ui run lint && npm --prefix ui test
```

The pre-commit hook (`.githooks/pre-commit`) runs the same checks, except `npm --prefix ui test`.
