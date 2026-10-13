# v3.3 implementation plan (dev) - user-facing config and auth

Parent spec: documents/plans/user/20261012b.md is normative.
This file is the dev-side task array and acceptance record.

Evidence: read src/main.rs (handle_* split), src/classifier/jev.rs
(from_env, request_url no-double-append), src/presentation (ASCII panel,
resolve_mode), scripts/quality_check.sh (OVERALL PASS required).
Keychain collision check: `security find-generic-password` shows
com.routeflags.kaseifu.jev is absent. docs/ moved to documents/, so the
developer doc goes to documents/development.md.

## BDD task list (behavior units)

- T1 resolver: no env, no TOML -> Defaults for endpoint/model, no key
- T2 resolver: JEV_API_URL wins over JEV_API_BASE_URL wins over TOML wins over Defaults
- T3 resolver: JEV_MODEL wins over TOML model wins over jev-latest
- T4 resolver: JEV_API_KEY env is used without touching Keychain, never displayed
- T5 resolver: no env key -> try Keychain, missing key is a config-shortage error
- T6 resolver: CLI --jev-endpoint/--jev-model win over env and TOML
- T7 loader: missing TOML -> empty config, boot with defaults
- T8 loader: broken TOML, unknown keys, type mismatch -> error, file untouched
- T9 loader: atomic save via temp file plus rename, never writes the API key
- T10 keychain: set/get/update/delete/missing round-trip on a test service id
- T11 keychain: unavailable store -> hard error, no plaintext fallback
- T12 setup: non-TTY refuses interactively; TTY shows current values, key presence only
- T13 setup: overwrite confirmation for an existing key, abort leaves files intact
- T14 auth: set-key (no echo, no log), status (stored state only, never claims success
  without a live check), remove-key (confirmation, reports when absent)
- T15 config: show hides secrets; set rejects unknown keys and api_key; failure keeps old file
- T16 doctor: local checks by default, network only with --check-connection, ASCII output
- T17 jev: init from resolved config, no URL double-append (existing tests stay green)
- T18 compat: from_env() kept; .env lookup moves to the resolver, adapter never searches
- T19 output: --format json prints JSON for new commands; Rich uses the ASCII panel;
  NO_COLOR respected
- T20 docs: README starts from setup; .env flow moves to documents/development.md

## Build order (Phase 1-9)

Phase1 research: done (see evidence). Phase2 resolver+loader, Phase3 keychain,
Phase4 setup, Phase5 config/auth, Phase6 jev, Phase7 doctor, Phase8 README,
Phase9 validation (quality_check.sh OVERALL PASS plus new tests).

## Design decisions

- Keychain via the keyring crate (backed by macOS Keychain on macOS).
  Service com.routeflags.kaseifu.jev, account default.
  Tests use a .test service id plus MemoryStore, never touching real credentials.
- Config path: dirs data_dir / kaseifu / config.toml
  (~/Library/Application Support/... on macOS).
- CLI overrides: global --jev-endpoint / --jev-model for classify/organize/watch.
- API key via CLI args is forbidden (shell history). remove-key needs confirmation.
- JevConfig.api_key is an ApiKey wrapper with redacted Debug output.
- Connectivity check is reachability-only (light GET, no auth); default is Not tested.
- New functions stay at CC 10 or below to keep the baseline gate green.

## As-built notes (2026-10-13)

- keyring v3 has no default macOS backend: `keyring = "3"` alone links the
  in-memory mock (set/get across Entry instances fails). Shipped with
  `features = ["apple-native"]`; verified against the real Keychain.
- Resolver maps any Keychain read failure to "no key" so `doctor` and
  `auth status` can report the cause instead of aborting. Write paths
  (`setup`, `auth set-key`) still surface the error and stop safely.
  No plaintext fallback anywhere.
- User thresholds are honored by the whole execution path: `evaluate_with`
  plus `build_move_plan_with` / `organize_file_with` /
  `process_request_with`; `run_loop` takes thresholds. Legacy signatures
  delegate with defaults, so existing tests are untouched.
- `doctor` exits 0 with a FAIL-line report (a missing key is a normal
  pre-setup state, not a crash). Connectivity is reachability-only
  (light GET, no auth) behind `--check-connection`.
- Interactive `setup` on the production service was deliberately not
  smoke-tested (would mutate the real entry); covered by non-TTY refusal
  plus unit-tested pieces. Manual TTY run remains as a release checklist.
