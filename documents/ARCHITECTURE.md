# Architecture

## Core pipeline

```text
macOS filesystem
        |
        v
Change / Target File
        |
        v
Context Builder
        |
        +--- std::fs
        +--- read_dir
        +--- xattr
        +--- Spotlight metadata
        |
        v
FileContext
        |
        v
Evidence Store
        |
        v
Candidate Discovery
        |
        +--- directory topology
        +--- Spotlight search
        +--- similar files
        |
        v
CandidateDestination[]
        |
        v
Local deterministic rules
        |
        v
Jev semantic decision
        |
        v
Policy Engine
        |
   +----+-----+
   |          |
 AUTO       REVIEW
   |
   v
Transactional Move
   |
   v
SQLite History
   |
   v
Undo
````

## Evidence sources

| Source           | Origin                    |
| ---------------- | ------------------------- |
| FS               | Rust std::fs              |
| DIR              | Rust read_dir             |
| XATTR            | macOS extended attributes |
| SPOTLIGHT_META   | mdls / Spotlight metadata |
| SPOTLIGHT_SEARCH | mdfind / Spotlight index  |
| CONTENT          | extracted file contents   |
| DERIVED          | locally calculated value  |

## Trust boundary

Jev never receives unrestricted filesystem control.

The local application:

1. collects evidence;
2. discovers existing directories;
3. generates candidate destination IDs;
4. validates candidate paths.

Jev only chooses among those IDs.

The Rust process owns filesystem mutations.
