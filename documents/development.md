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
| `KASEIFU_DEBUG` | `1` enables debug output on stderr |

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

## Local build and run

```bash
cargo build
cargo run -- classify ~/Desktop/example.pdf
```

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
`tests/v33_config_auth_test.rs`).

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

## Resolution order (reference)

```text
CLI flags (--jev-endpoint, --jev-model)
  -> JEV_API_URL / JEV_API_BASE_URL / JEV_MODEL / JEV_API_KEY (environment)
  -> project-local .env (developers/CI)
  -> ~/Library/Application Support/kaseifu/config.toml + macOS Keychain
  -> application defaults (config/default.toml values)
```

The API key is never accepted as a CLI argument (it would linger in shell
history) and is never written to TOML or logs.
