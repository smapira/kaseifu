# A locked Keychain could hang every command - done

- status: done
- completed: 2026-11-08 (found during the #26 real-use verification; no GitHub
  issue was opened for it)
- milestone: M1 - Reliability and CLI clarity
- next owner: none (verify at merge time)
- blocker: none

## The finding

`kaseifu cluster`, `doctor` and `auth status` produced **no output** for 60
seconds or more on a machine whose login Keychain did not answer, while
`KASEIFU_KEY_SOURCE=env JEV_API_KEY=` made the same commands finish in 0.3s.
The key read sits at startup (`config::resolve` -> `SecretStore::get`), so every
command that resolves a key - including the new `cluster` - blocked before it
could print anything. The network was not involved and the Jev timeouts were
never reached.

## Decision

- The read is bounded (2 seconds) instead of relying on the Keychain or the
  Security framework to return. A call waiting on a GUI prompt cannot be
  cancelled, so the worker runs on a detached thread and the command proceeds
  with an unavailable key source; the process exit takes the thread with it.
- The failure is reported where the key source is already reported: `doctor`
  prints `FAIL API Key: ... (next: kaseifu setup ...)`, `auth status` prints the
  same reason, and `cluster` falls back to its type-based names with
  `naming: fallback (...)`.
- The message names the next step (`KASEIFU_KEY_SOURCE=env` / unlock the
  Keychain) rather than only stating that something failed.
- Only the read is bounded. `set`/`delete` run in commands a person invoked for
  that purpose and may legitimately wait for a prompt.

## What changed

- `src/auth/keychain.rs`: `KEYCHAIN_READ_TIMEOUT` (2s), `bounded()` (a detached
  worker plus `recv_timeout`), `SecretStore::get` for the Keychain uses it, and
  unit tests cover the slow (reported) and fast (returned) paths.
- No new dependency: `std::sync::mpsc` and `std::thread`.

## Verification (2026-11-08)

- `cargo test --lib auth::keychain`: the bounded tests pass (the slow path
  returns an `Unavailable` error in well under the worker's sleep).
- `KASEIFU_KEY_SOURCE=auto kaseifu doctor`: returns in 2.8s, exit status
  reports `FAIL API Key: Keychain is unavailable: the Keychain did not answer
  within 2s ... (next: kaseifu setup ...)` (before: no output after 60s).
- `KASEIFU_KEY_SOURCE=auto kaseifu auth status`: returns in 2.0s with
  `Source: unavailable (<same reason>)`.
- `bash scripts/quality_check.sh`: OVERALL PASS.
