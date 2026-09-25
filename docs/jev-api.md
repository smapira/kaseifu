# Jev API Contract

## Verified API Details

**Status:** NO LIVE API ENDPOINT AVAILABLE

The design document `docs/design/04-jev.md` and `docs/JEV.md` describe a **conceptual semantic decision interface** for AI-assisted file classification, but **do not document an actual REST API endpoint**.

### Documentation Sources

1. `docs/design/04-jev.md` - Describes conceptual DecisionContext → Jev → choice/probabilities
2. `docs/JEV.md` - Describes output format and local policy decision

### Verified Contract

```rust
// Input
DecisionContext {
    target: DecisionTarget,  // filename, extension, file_type
    semantic_signals: Vec<String>,  // ["CBX", "カンナビノイド", "NATURECAN"]
    candidates: Vec<CandidateDestination>  // [{"id": "D1", ...}, ...]
}

// Output
JevResponse {
    destination_id: Option<String>,  // Some("D1") or Some("NONE")
    probabilities: HashMap<String, f64>,  // {"D1": 0.91, "D2": 0.06, "NONE": 0.03}
    confidence: f64
}
```

### Security Boundary

- **Authentication:** `JEV_API_KEY` environment variable
- **Invariant:** Jev can only select from D1/D2/D3/NONE - cannot create paths
- **Implementation:** Real HTTP integration deferred until actual API endpoint is available

---

## Implementation Notes

### Phase 4C: RealJevAdapter

Since no live API endpoint is documented, Phase 4C implements:

1. **MockJevIntegration** - Local mock returning configured values (TDD setup)
2. **RealJevIntegration** - Trait implementation with HTTP placeholder
   - Uses `JEV_API_KEY` from environment
   - Has empty implementation stub for actual HTTP call
   - Tests use mock, real integration tests skip until API available

### Test Strategy

Tests do **NOT** call real API during development:

```bash
# Unit tests use MockJevIntegration
cargo test mock_jev
# ~15 tests PASS

# Integration tests are marked skip until API available
#[tokio::test]
#[ignore = "requires live Jev API endpoint"]
async fn test_jev_integration() {
    ...
}
```

### Next Steps for Real API Integration

To enable real Jev API:

1. Obtain TypeSafe AI Jev API documentation with:
   - Endpoint URL
   - Authentication method (Bearer token or API key)
   - Request schema (JSON payload structure)
   - Response schema
   - Supported models
   - Timeout/error behavior

2. Update `RealJevIntegration` HTTP stub with real implementation

3. Remove `#[ignore]` from integration tests

---

## Current Test Status

```
Phase 4A Decision domain: ✅ GREEN (~12 tests)
Phase 4B Jev integration contract: ✅ GREEN (~8 tests)  
Phase 4C Real Jev Adapter: 🟡 MOCK (integration: SKIPPED)
Phase 4D Live E2E: ❌ (requires live API)
```

All unit and mock tests pass. Integration tests are marked skip awaiting real API documentation.
