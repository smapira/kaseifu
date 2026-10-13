# Jev API Contract (verified)

Status: **verified against the live API** on 2026-09-25 by `GET /openapi.json`
and a live `POST /v1/systemone` (see the Phase 4E run). All tests remain
offline; no test touches the live API.

## Official contract (api.typesafe.ai, OpenAPI 3.1, TypeSafe v0.2.0)

- **Endpoint:** `POST /v1/systemone` (also `GET /v1/models`)
- **Auth:** `Authorization: Bearer <JEV_API_KEY>`
- **Request** (`SystemOneRequest`):
  - `state`: string | object | array — the content the questions refer to
  - `model`: model name/alias (e.g. `jev-latest`)
  - `questions`: named questions; a `choice` question carries
    `instructions` and `criteria` (choice name → description)
- **Response** (`SystemOneResponse`): `model`, `usage`, and
  `answers.<question-name>`; a `ChoiceAnswer` contains
  `type: "choice"`, `choice` (highest-probability choice name),
  `confidence` 0–1, and `probabilities` (per-choice, keyed by choice name,
  summing to ≈1).

**Gate answer:** the real API **does** return per-choice probabilities
(`ChoiceAnswer.probabilities`), so the domain contract
(`confidence = P1`, `margin = P1 − P2`) holds unchanged.

## Adapter mapping (src/classifier/jev.rs)

| kaseifu | TypeSafe `/v1/systemone` |
|---|---|
| `DecisionContext.target` | `state.target` |
| `DecisionContext.semantic_signals` | `state.semantic_signals` (content signals verbatim + availability markers derived by the adapter from `image_metadata`, v0.4.1) |
| candidate `id` + `path` + `evidence_count` + `reasons` | `questions.destination.criteria[id]` description |
| — (always added) | `criteria["NONE"]` |
| `answers.destination.choice` | `JevResponse.destination_id` (verbatim; domain validates) |
| `answers.destination.probabilities` | `JevResponse.probabilities` |
| `answers.destination.confidence` | **not copied**; domain computes confidence/margin itself |

Configuration:

- `JEV_API_URL` (or legacy `JEV_API_BASE_URL`): the API base URL or the full
  endpoint URL — both are accepted without duplicating `/v1/systemone`.
- `JEV_API_KEY`: bearer token; never stored in source, tests, fixtures or git.
- `JEV_MODEL` (optional): model alias; defaults to `jev-latest`.
- Timeout: 15s. Errors: 401/403 → `Authentication`, other 4xx → `ApiError`,
  5xx → `ServerError`, transport → `TransportError`, schema → `InvalidResponse`
  (with a bounded body snippet). `NONE` is a decision; errors never become it.

## Verified live run (Phase 4E)

Real file → production Observe/Normalize/Discover → 9 discovered candidates →
`POST /v1/systemone` → `choice: "D1"`, `probabilities {D1: 0.57, NONE: 0.43, …}`
→ domain membership validation → `JevDecision { destination_id: D1,
confidence: 0.57, margin: 0.14 }`. No filesystem operation was performed.

## Known follow-ups (not Phase 4 blockers)

- Normalized signals for screenshots are generic (`K:PNG`, `K:image`), which
  makes Spotlight candidates noisy; signal quality belongs to Phase 2.
- `aggregate_by_directory` emits `reasons` matched terms in `HashSet` order;
  ordering varies between runs (recorded in Phase 4D.5, unfixed by design).
