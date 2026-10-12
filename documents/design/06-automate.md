
# Phase 6 — Automation

## Purpose

手動scanから、
イベント駆動型のファイル整理へ移行する。

---

# Initial Target

監視対象:

```text
~/Desktop
~/Downloads
```

---

# Architecture

```text
macOS
 │
 ▼
FSEvents
 │
 ▼
Changed Path
 │
 ▼
Debounce
 │
 ▼
Stable File Check
 │
 ▼
Observe
 │
 ▼
Normalize
 │
 ▼
Discover
 │
 ▼
Decide
 │
 ▼
Policy
```

---

# Important

ダウンロード中ファイルを処理しない。

例:

```text
.partial
.download
.crdownload
```

またはsize/mtimeが安定するまで待つ。

---

# Incremental Processing

全Desktopを毎回scanしない。

変更されたファイルだけ処理する。

---

# Cache

key候補:

```text
path
size
mtime
```

変更されていなければ:

```text
CACHE HIT
```

として再解析しない。

---

# Future CLI

```bash
kaseifu watch ~/Desktop
```

または:

```bash
kaseifu daemon
```

---

# Completion Criteria

Desktopへファイルを置くと、

```text
detected
→ classified
→ candidate discovered
→ decision
```

まで自動実行される。

初期段階ではAUTO MOVEを無効にしてもよい。

