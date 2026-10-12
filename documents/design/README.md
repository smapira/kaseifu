# Kaseifu / Semantic File Organizer
# Development Design

## 1. Purpose

macOS上で散らかったファイルを解析し、

「このファイルは既存のどのディレクトリに保存するのが適切か」

を推定し、安全に移動するRustアプリケーションを開発する。

Jevはファイルシステムを直接操作しない。

macOSとRustが候補を発見・検証し、
Jevは候補の中から意味的に適切な保存先を判断する。

---

## 2. Core Architecture

```text
File
 │
 ▼
[Phase 1: Observe]
Filesystem / Spotlight / xattr
 │
 ▼
Raw Evidence
 │
 ▼
[Phase 2: Normalize]
Normalized FileContext
 │
 ▼
[Phase 3: Discover]
Similar Files / Candidate Directories
 │
 ▼
CandidateDestination[]
 │
 ▼
[Phase 4: Decide]
Jev
 │
 ▼
Decision
 │
 ▼
[Phase 5: Execute]
Policy / Move / History / Undo
 │
 ▼
Organized Filesystem


[Phase 6: Automate]
FSEvents
 │
 └──────→ Phase 1
````

---

## 3. Design Principles

### Local First

可能な処理はMac上で行う。

Jevへ不要なデータを送信しない。

### Evidence Based

AIにファイルパスを自由生成させない。

保存先候補には必ずローカルのEvidenceを持たせる。

### Candidate Selection

Jevは任意のパスを生成しない。

Rustが生成した

D1 / D2 / D3 / NONE

から選択する。

### Reversible

ファイル操作は履歴を保存し、Undo可能にする。

### Progressive Context

最初からすべての情報を取得しない。

安価で高速な情報から順番に取得する。

### macOS First

初期バージョンではmacOSのみを対象とする。

macOS固有機能:

* Spotlight
* extended attributes
* FSEvents

を積極的に利用する。

---

## 4. Development Phases

| Phase | Name      | Responsibility  |
| ----- | --------- | --------------- |
| 1     | Observe   | ファイルシステム情報収集    |
| 2     | Normalize | 生データの正規化        |
| 3     | Discover  | 保存先候補の発見        |
| 4     | Decide    | Jevによる意味判断      |
| 5     | Execute   | 移動・履歴・Undo      |
| 6     | Automate  | FSEventsによる自動処理 |

---

## 5. Current Status

現在実装済み:

* Rust CLI
* inspect command
* std::fs metadata
* current directory取得
* parent directory取得
* Spotlight metadata取得
* xattr取得
* FileContext JSON生成

部分実装:

* Spotlight search
* mover
* policy

未実装:

* Normalizer
* Search Term extraction
* Candidate Discovery
* Jev API integration
* History
* Undo
* FSEvents integration

---

## 6. Immediate Goal

次のマイルストーンは:

```text
FILE
 ↓
Raw Evidence
 ↓
Normalize
 ↓
Search Terms
 ↓
Spotlight Search
 ↓
Similar Files
 ↓
Candidate Directories
```

ここまでをJev API接続前に完成させる。

