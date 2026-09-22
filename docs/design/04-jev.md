
# Phase 4 — Jev Decision

## Purpose

Candidate Discoveryが生成した候補から、
意味的に最も適切な保存先を選択する。

---

# Jev Responsibility

Jevが行う:

```text
Candidate Selection
```

Jevが行わない:

```text
filesystem scanning
arbitrary path generation
file move
file delete
permission change
```

---

# Input

JevにはRaw Evidenceを送らない。

送信:

```text
DecisionContext
```

例:

```json
{
  "target": {
    "filename": "CBX NATURECAN.png",
    "type": "image"
  },

  "semantic_signals": [
    "CBX",
    "カンナビノイド",
    "NATURECAN"
  ],

  "candidates": [
    {
      "id": "D1",
      "description": "Research/CBX",
      "related_files": 12
    },
    {
      "id": "D2",
      "description": "Pictures/CBX",
      "related_files": 4
    }
  ]
}
```

---

# Choice

選択肢:

```text
D1
D2
D3
NONE
```

`NONE` は必須。

AIに無理な選択をさせない。

---

# Output

必要情報:

```text
choice
probabilities
confidence
```

例:

```json
{
  "choice": "D1",

  "probabilities": {
    "D1": 0.91,
    "D2": 0.06,
    "NONE": 0.03
  },

  "confidence": 0.88
}
```

---

# Local Calculation

Rust側で:

```text
margin = P1 - P2
```

を計算する。

---

# Security

API Key:

```text
JEV_API_KEY
```

`.env` はGitへcommitしない。

---

# New / Existing Modules

```text
src/classifier/jev.rs
src/models/decision.rs
```

---

# Completion Criteria

```bash
cargo run -- classify FILE
```

で:

```text
Candidate: D1
Confidence: 0.88
Margin: 0.85
```

まで取得できる。

この段階ではまだファイルを移動しない。

