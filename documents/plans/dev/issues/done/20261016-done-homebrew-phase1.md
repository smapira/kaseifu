# Homebrew Phase 1 (source install through the tap) — done

- status: done
- completed: 2026-10-16
- milestone: M4 - Measurement and release hygiene
- next owner: none
- blocker: none
- plan: `documents/plans/dev/20261016d-homebrew-support.md`
- tap handoff: `documents/plans/dev/20261016e-tap-agent-handoff.md`
- tap issue: <https://github.com/routeflags/homebrew-tap/issues/1> (still OPEN on their side)

## Objective

`brew tap routeflags/tap` → `brew trust --formula routeflags/tap/kaseifu` → `brew install kaseifu` → `kaseifu --version` works from GitHub.

## What changed (tool side)

- `LICENSE` (MIT) so the release archive carries it.
- `build.rs` drops `SDKROOT` for its Swift compiler invocations (v0.12.2): Homebrew exports it pointing at the Command Line Tools SDK, where the `@Generable` macro plugin cannot be resolved.
- `build.rs` passes `-disable-sandbox` to its own Swift compile (v0.12.3): the Swift driver's nested `sandbox-exec` is denied inside Homebrew's build sandbox (`sandbox_apply: Operation not permitted`).
- `build.rs` still falls back to the Rust stub with an explicit reason when there is no toolchain or no usable SDK.
- `.github/workflows/homebrew-acceptance.yml`: disposable `macos-26` runner, manual plus weekly, captures the Homebrew build logs as an artifact on failure.
- README documents the tap install path with the honest prerequisites (Xcode 26 or later selected, Command Line Tools insufficient, build-only dependencies such as `llvm`).
- CHANGELOG `[Unreleased]` records the tap.

Two real build failures were found and fixed through the acceptance runs (both reproduced locally before fixing).

## Verification (2026-10-16)

- Tap formula at v0.12.3, sha256 `11b397b9...e54c5e3` (measured).
- CI run [38844700758](https://github.com/smapira/kaseifu/actions/runs/38844700758): completed/success. Tap + trust OK, source install OK in 207 seconds, version match `formula=0.12.3 installed=0.12.3` on both binaries, `brew test` OK, build-dependency footprint only `rust 1.98.0`.
- `brew audit --strict --new`, `brew style`, `brew livecheck` (`0.12.3 ==> 0.12.3`): all green from this side without touching the prefix.
- `cargo test --all-targets --all-features`: 886 passed / 0 failed; `bash scripts/quality_check.sh`: OVERALL PASS.

## Known remaining items (not Phase 1)

- Prebuilt bottles (Phase 2, held): `documents/plans/dev/issues/waiting-user/20261016-waiting-bottle-distribution.md`.
- A machine that never had Xcode, and a clean-machine install cost: `documents/plans/dev/issues/waiting-user/20261016-waiting-clean-mac-verification.md`.
- `build.rs` SDK probe hardening (CLT still hard-fails instead of degrading): `documents/plans/dev/issues/open/20261016-open-build-rs-sdk-probe.md`.
- The `label` guidance path creating the state database: `documents/plans/dev/issues/open/20261016-open-label-guidance-touches-state.md`.
