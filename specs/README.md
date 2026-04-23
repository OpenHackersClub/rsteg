## rsteg specs

Design specifications for rsteg — a minimal-dependency, plugin-based steganography tool in Rust.

Specs are numbered for reading order. Implementation follows these specs; when reality diverges, update the spec in the same PR as the code change.

| # | File | Topic |
|---|------|-------|
| 00 | [overview.md](00-overview.md) | Goals, non-goals, comparison targets |
| 01 | [architecture.md](01-architecture.md) | Workspace layout, hexagonal boundaries, plugin model |
| 02 | [dependency-policy.md](02-dependency-policy.md) | Supply-chain rules, allowlist, CI gates |
| 03 | [cli.md](03-cli.md) | CLI surface and UX |
| 04 | [core-traits.md](04-core-traits.md) | `rsteg-core` trait contracts |
| 05 | [formats.md](05-formats.md) | PNG / BMP / WAV / JPEG format adapters |
| 06 | [crypto.md](06-crypto.md) | AEAD default + steghide-compat detection/read |
| 07 | [testing.md](07-testing.md) | TDD strategy, corpus, fuzzing |
| 08 | [benchmarking.md](08-benchmarking.md) | Bench harness, comparison methodology |
| 09 | [roadmap.md](09-roadmap.md) | Phased delivery plan |

Also see [`REVIEW_NOTES.md`](REVIEW_NOTES.md) — captures the phase-0 review team's findings and which ones reshaped the spec.

### Status

Phase 0 — specification **confirmed** (2026-04-23) after a 5-agent review pass. Implementation (phase 1) in progress; no Rust code merged yet at the time this was updated.
