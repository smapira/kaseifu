# Development Guide

Developer setup for kaseifu: `.env` files, environment variables, local
builds, tests, CI, API mocks, and the configuration resolution order.
End users should start with `kaseifu setup` (see `README.md`).

## Environment variables

| Variable | Purpose |
|---|---|
| `JEV_API_URL` | Jev endpoint URL (base URL or full `/v1/systemone` URL; both work) |
| `JEV_API_BASE_URL` | Legacy alias for `JEV_API_URL` (kept for compatibility) |
| `JEV_API_KEY` | Bearer token (never stored in source, tests, fixtures, or git) |
| `JEV_MODEL` | Model alias (defaults to `jev-latest`) |
| `KASEIFU_KEY_SOURCE` | API key source: `auto` (env/.env, then Keychain), `env` (env/.env only, Keychain never touched), `keychain` (Keychain only). Process environment first, then `.env`. Debug builds default to `env`, release builds to `auto` |
| `KASEIFU_DEBUG` | `1` enables debug output on stderr |
| `KASEIFU_COMPLETION_DIR` | zsh completion directory override (skips the Homebrew/Prezto/XDG detection) |
| `KASEIFU_NO_COMPLETION_UPDATE` | Non-empty disables the automatic completion refresh |

`JEV_API_URL` wins over `JEV_API_BASE_URL` when both are set.

## `.env` files (developers only)

Copy `.env.example` to `.env` and fill in local credentials:

```bash
cp .env.example .env
```

```text
JEV_API_KEY=your-key-here
JEV_API_URL=https://your-jev-endpoint.example.com
```

The project-local `.env` is git-ignored and is read only by the
configuration resolver (CLI > environment > `.env` > user TOML/Keychain >
defaults). The Jev adapter itself never searches the working directory.

Debug builds (`cargo run`, `cargo test`) default to `KASEIFU_KEY_SOURCE=env`,
so development and tests never read or write the macOS Keychain — no
approval dialog, no hang. `auth status` and `doctor` show the effective mode
(`KASEIFU_KEY_SOURCE: env (default)`) and the source that resolved the key.
The `keychain` mode remains available explicitly
(`KASEIFU_KEY_SOURCE=keychain cargo run -- auth status`) for checking the
stored entry; release builds default to `auto` and keep the historical
Keychain behavior for end users. A `.env` may also pin the mode itself
(`KASEIFU_KEY_SOURCE=env`), which the process environment overrides.

## Local build and run

```bash
cargo build
cargo run -- classify ~/Desktop/example.pdf
```

Debug builds resolve the API key from the environment/`.env` by default; a
`.env` with `JEV_API_KEY=...` is the normal development setup (see above).

Override the endpoint or model for one invocation without touching files:

```bash
cargo run -- --jev-endpoint http://127.0.0.1:8080 --jev-model jev-latest classify ~/Desktop/example.pdf
```

## Tests

```bash
cargo test --all-targets --all-features
```

No live API key is needed: the suite uses mock Jev servers and an
in-memory secret store. Keychain tests use the reserved service id
`com.routeflags.kaseifu.jev.test` and never touch the production entry
`com.routeflags.kaseifu.jev` / `default` (verified by
`tests/v33_config_auth_test.rs`). The key-source tests inject a counting
store and assert zero `get`/`set`/`delete` calls for `env` mode
(`tests/key_source_test.rs`); the CLI tests run the real binary inside a
temporary directory with an isolated HOME and a deadline, so a regression
that reintroduces a blocking Keychain prompt fails instead of hanging
(`tests/key_source_cli_test.rs`).

## Quality gate and CI

One script runs every check, locally and in CI
(`.github/workflows/rust-quality.yml`, macOS runner):

```bash
bash scripts/quality_check.sh
```

See `README.md` ("Code quality") for the individual checks and the
complexity policy (new functions must stay under CC 16; keep new code at
CC 10 or below).

## Jev API mocks

`RealJevIntegration::new(endpoint, api_key)` plus `with_model(model)`
inject any base URL, so tests point the adapter at a local mock HTTP
server (see `tests/phase4_real_jev_adapter_test.rs`). Production code
prefers `RealJevIntegration::from_resolved(&resolved.jev)`; the legacy
`from_env()` constructor is kept for compatibility.

## Shell completion (developers)

`kaseifu completions zsh` prints the script; `--install` writes `_kaseifu`
into the first writable directory of the documented detection order (see
`README.md`, "Shell completion"). Any other command also refreshes the
installed script when it is missing or stale, best effort: failures never
change the exit code or the output, and the only diagnostic is
`[debug] completion refresh …` with `KASEIFU_DEBUG=1`.

When developing the CLI surface:

- `tests/completion_test.rs` asserts that the script carries every command
  from `--help`, that nested `auth`/`config` subcommands are present, and that
  no hidden command leaks. Run it after adding or hiding a command.
- Hidden commands are excluded by a zsh post-processor (`src/completion/zsh.rs`)
  because `clap_complete` renders hidden subcommands and `clap` cannot remove
  a subcommand from a tree (clap-rs/clap#1335). When upstream fixes this the
  filter becomes a no-op and can be deleted.
- Tests must never point at a real site-functions directory:
  `KASEIFU_COMPLETION_DIR` always addresses a temporary directory, or the
  library API is driven directly with an injected `RefreshPlan`.

## Resolution order (reference)

```text
CLI flags (--jev-endpoint, --jev-model)
  -> JEV_API_URL / JEV_API_BASE_URL / JEV_MODEL (environment)
  -> project-local .env (developers/CI)
  -> ~/Library/Application Support/kaseifu/config.toml + macOS Keychain
  -> application defaults (src/config/defaults.rs)

API key, selected by KASEIFU_KEY_SOURCE:
  auto     -> JEV_API_KEY (environment or .env) -> macOS Keychain
  env      -> JEV_API_KEY (environment or .env) only (never the Keychain)
  keychain -> macOS Keychain only (JEV_API_KEY ignored)
```

The API key is never accepted as a CLI argument (it would linger in shell
history) and is never written to TOML or logs.

## Local Jev mocks and the key check

`classify` / `organize` / `watch` fail fast when an explicit source
(`env` / `keychain`) resolves no key. When pointing kaseifu at a local mock
that ignores authentication, set any dummy value, e.g.

```bash
JEV_API_KEY=dummy cargo run -- --jev-endpoint http://127.0.0.1:8080 classify file.pdf
```
