
# Phase 1 — Observe

## Purpose

対象ファイルについてmacOSから客観的な事実を収集する。

このレイヤーでは意味判断を行わない。

---

# Inputs

```text
PathBuf
```

例:

```text
/Users/user/Desktop/example.pdf
```

---

# Data Sources

## FS

Rust:

```text
std::fs
std::fs::metadata
```

取得:

* canonical path
* filename
* extension
* size
* modified time
* file type

---

## DIR

Rust:

```text
std::fs::read_dir
```

取得:

* current directory
* parent directory
* directory entries

shellの `ls` は使用しない。

---

## SPOTLIGHT_META

macOS:

```text
mdls
```

初期対象:

```text
kMDItemContentType
kMDItemKind
kMDItemTitle
kMDItemAuthors
kMDItemKeywords
kMDItemWhereFroms
kMDItemFSCreationDate
kMDItemFSContentChangeDate
```

---

## XATTR

取得候補:

```text
com.apple.quarantine
com.apple.metadata:kMDItemWhereFroms
com.apple.metadata:_kMDItemUserTags
```

binary attributeを無条件に文字列化しない。

---

# Output

```text
RawFileEvidence
```

重要:

Raw EvidenceはJevへ直接送らない。

---

# Current Implementation

```text
src/platform/macos/filesystem.rs
src/platform/macos/spotlight.rs
src/platform/macos/xattr.rs
src/context/builder.rs
```

---

# Completion Criteria

以下が安定して取得できること:

```bash
cargo run -- inspect FILE
```

対象:

* PNG
* JPEG
* PDF
* TXT
* MD
* ZIP
* directory

