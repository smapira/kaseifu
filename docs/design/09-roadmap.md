
# Development Roadmap

## Milestone 1 — Observation

Status: mostly complete

* [x] Rust project
* [x] CLI
* [x] filesystem metadata
* [x] directory listing
* [x] Spotlight metadata
* [x] xattr
* [x] FileContext JSON
* [ ] filter binary xattr
* [ ] normalize Spotlight values

---

## Milestone 2 — Normalization

* [ ] Normalizer module
* [ ] filename token extraction
* [ ] stopword removal
* [ ] hidden/system file filtering
* [ ] xattr parsing
* [ ] source URL normalization

---

## Milestone 3 — Candidate Discovery

* [ ] search term generation
* [ ] Spotlight search
* [ ] similar file model
* [ ] parent directory aggregation
* [ ] candidate scoring
* [ ] candidate validation
* [ ] discover CLI

---

## Milestone 4 — Jev

* [ ] API client
* [ ] request model
* [ ] response model
* [ ] candidate choice
* [ ] probability parsing
* [ ] margin calculation
* [ ] classify CLI

---

## Milestone 5 — Safe Move

* [ ] policy engine
* [ ] SQLite history
* [ ] collision protection
* [ ] dry-run
* [ ] move
* [ ] undo

---

## Milestone 6 — Automation

* [ ] FSEvents
* [ ] debounce
* [ ] incomplete-download detection
* [ ] cache
* [ ] watch command

---

# Current Priority

Do NOT implement Jev yet.

Current priority:

```text
Normalize
   ↓
Discover
```

Success condition:

```bash
cargo run -- discover FILE
```

returns useful destination candidates without AI.

