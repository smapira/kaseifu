
# Phase 3 — Candidate Discovery

## Purpose

「このファイルをどこへ保存するべきか」

を直接AIへ質問するのではなく、

Mac内に既に存在するファイル配置から
保存先候補を発見する。

---

# Pipeline

```text
NormalizedFileContext
        │
        ▼
Search Terms
        │
        ▼
Spotlight Search
        │
        ▼
Similar Files
        │
        ▼
Parent Directories
        │
        ▼
Aggregation
        │
        ▼
CandidateDestination[]
```

---

# Example

Target:

```text
CBX NATURECAN.png
```

Terms:

```text
CBX
カンナビノイド
NATURECAN
```

Spotlight results:

```text
~/Research/CBX/a.pdf
~/Research/CBX/b.png
~/Research/CBX/c.md
~/Pictures/CBX/d.png
```

Aggregate:

```text
~/Research/CBX
3 matches

~/Pictures/CBX
1 match
```

Output:

```json
[
  {
    "id": "D1",
    "path": "~/Research/CBX",
    "evidence_count": 3
  },
  {
    "id": "D2",
    "path": "~/Pictures/CBX",
    "evidence_count": 1
  }
]
```

---

# Candidate Scoring

初期スコア:

```text
score =
    related_file_count
  + filename_similarity
  + metadata_similarity
  + directory_frequency
```

AIを使わずローカルで計算する。

---

# Safety

候補は必ず:

```text
canonicalize()
```

して存在確認する。

候補として許可しないもの:

```text
/System
/private
/bin
/sbin
/usr
```

など。

---

# Spotlight

Spotlightは候補発見に使用する。

Filesystemの正解情報源としては使用しない。

検索結果は必ずFilesystemで存在確認する。

---

# New Modules

```text
src/discovery/
    candidates.rs
    similarity.rs
    search_terms.rs
    aggregation.rs
```

---

# Completion Criteria

以下で:

```bash
cargo run -- discover FILE
```

次のような結果を取得できること:

```text
D1 ~/Research/CBX       score=0.89 evidence=12
D2 ~/Pictures/CBX       score=0.51 evidence=4
D3 ~/Documents/Research score=0.32 evidence=2
```

Jevはまだ使用しない。

