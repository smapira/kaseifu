# Jev API Contract

Status: adapter implemented (Phase 4D). No live API call is performed by any
test.

## Canonical sources

The kaseifu design documents are the canonical contract:

- `docs/design/04-jev.md` — request/response examples, choice set, local
  margin calculation, `JEV_API_KEY`
- `docs/JEV.md` — request/response examples, "local policy engine decides"

Both documents describe the same shape: the request carries the target and
the offered candidates, and the response carries the chosen option plus a
**per-choice probability distribution**. That answers the Phase 4D gate
question ("does Jev return per-choice probabilities?") with **yes**, per the
canonical docs.

## Verified contract (as implemented)

Request (POST, JSON):

```json
{
  "target": { "filename": "...", "extension": "png", "type": "..." },
  "semantic_signals": ["CBX", "カンナビノイド"],
  "candidates": [ { "id": "D1", "related_files": 2 } ]
}
```

Response (JSON):

```json
{
  "choice": "D1",
  "probabilities": { "D1": 0.91, "D2": 0.06, "NONE": 0.03 },
  "confidence": 0.88
}
```

- `choice` and `destination_id` are both accepted (the two design documents
  spell it differently); a response without a selection is rejected as
  `InvalidResponse`.
- `confidence` from the API is **informational only**. kaseifu computes
  `confidence = P1` and `margin = P1 - P2` in `models::decision`; the API
  value is never copied into the domain decision.
- `NONE` is a valid decision. Errors (HTTP failures, transport failures,
  malformed responses) are never converted into `NONE`.

## Adapter implementation

`src/classifier/jev.rs`:

- `RealJevIntegration::new(endpoint, api_key)` — endpoint is injected; tests
  use a local mock HTTP server.
- `RealJevIntegration::from_env()` — reads `JEV_API_BASE_URL` (full decision
  endpoint URL) and `JEV_API_KEY` (optional). Credentials never appear in
  source, fixtures, or git.
- Request timeout: 15s.
- Error mapping: 401/403 → `Authentication`, other 4xx → `ApiError`,
  5xx → `ServerError`, connection/timeout → `TransportError`, malformed
  body or missing selection → `InvalidResponse`.

Authorization is sent as `Authorization: Bearer <JEV_API_KEY>` when the key
is configured.

## Not yet verified (Phase 4E gates)

The following items are **not** confirmable from this repository and must be
confirmed against the official API before a live call:

1. The real endpoint URL and whether it is a single POST endpoint.
2. The authentication header scheme (Bearer is provisional).
3. Exact optional request fields (the docs show `description`/`path` for
   candidates; the adapter sends `id` + `related_files`, which both
   documents agree on).
4. Real-world error body schema (only status-based mapping is implemented).

Because the API schema is isolated in private DTOs inside the adapter,
confirming these items requires changes in one file only; the domain and
pipeline do not change.

## Known limitation

`DecisionContext::from_candidates` (and therefore the pipeline) currently
leaves `target` and `semantic_signals` empty; the adapter sends what the
domain provides. Populating them from `NormalizedFileContext` is a
follow-up outside Phase 4D.
