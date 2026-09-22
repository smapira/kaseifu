
# Phase 2 — Normalize

## Purpose

macOSから取得した生データを、
AIとCandidate Discoveryが扱いやすい構造へ変換する。

---

# Problem

現在は以下のようなデータが存在する:

```text
com.apple.macl
binary garbage
```

また:

```text
kMDItemWhereFroms =
(
    "https://...",
    ""
)
```

のようなCLI固有表現が存在する。

これらをそのままJevへ渡してはいけない。

---

# Normalization Pipeline

```text
Raw Evidence
     │
     ▼
Allowlist
     │
     ▼
Parser
     │
     ▼
Type Conversion
     │
     ▼
Noise Removal
     │
     ▼
Normalized Context
```

---

# Example

Before:

```json
{
  "key": "kMDItemWhereFroms",
  "value": "(\n \"https://example.com\"\n)"
}
```

After:

```json
{
  "source_urls": [
    "https://example.com"
  ]
}
```

---

# XATTR

Before:

```text
0081;...;Firefox;UUID
```

After:

```json
{
  "download_agent": "Firefox"
}
```

---

# Filename Signals

Filename:

```text
Screenshot ... CBX（カンナビノイド）とは？｜NATURECAN.png
```

Extract:

```json
{
  "terms": [
    "CBX",
    "カンナビノイド",
    "NATURECAN"
  ]
}
```

---

# Noise

除外候補:

```text
.DS_Store
.localized
.git
node_modules
target
```

---

# Output

```text
NormalizedFileContext
```

例:

```json
{
  "filename": "example.pdf",
  "type": "pdf",

  "semantic_signals": [
    "CBX",
    "cannabinoid"
  ],

  "provenance": {
    "download_agent": "Firefox",
    "source_urls": []
  }
}
```

---

# New Modules

```text
src/normalize/
    mod.rs
    metadata.rs
    filename.rs
    xattr.rs
    noise.rs
```

---

# Completion Criteria

Raw Evidenceから、

* binary garbageが除去される
* Spotlight値が型付きになる
* filenameから検索語を生成できる
  -不要ファイルが除外される

こと。

