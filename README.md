
# Semantic File Organizer

A macOS-first semantic file organizer written in Rust.

The goal is to answer:

> Where does this file belong?

using the context already available on the user's Mac.

## Signals

* filesystem metadata
* directory topology
* Spotlight metadata
* Spotlight search
* extended attributes
* similar existing files
* optional content extraction
* Jev semantic classification

## Pipeline

```text
File
 ↓
Context
 ↓
Evidence
 ↓
Candidate Discovery
 ↓
Jev
 ↓
Policy
 ↓
Move / Review / Leave
 ↓
History / Undo
```

## Inspect a file

```bash
cargo run -- inspect ~/Desktop/example.pdf
```

This prints the `FileContext` JSON that will form the basis
of later candidate discovery and Jev classification.

## Safety

Jev cannot generate arbitrary filesystem destinations.

The local Rust application discovers and validates
destination candidates first.

Filesystem mutations remain local and reversible.

## Development stages

### Phase 1

FileContext generation.

### Phase 2

Spotlight-based candidate discovery.

### Phase 3

Jev integration.

### Phase 4

Policy + dry-run.

### Phase 5

Transactional move + undo.

### Phase 6

FSEvents and continuous organization.
