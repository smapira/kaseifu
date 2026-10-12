
# Phase 5 — Safe Execution

## Purpose

Jevの判断をそのまま実行せず、
ローカルPolicy Engineを通して安全にファイルを移動する。

---

# Pipeline

```text
Jev Decision
     │
     ▼
Policy Engine
     │
 ┌───┼────────┐
 ▼   ▼        ▼
AUTO REVIEW  LEAVE
 │
 ▼
Journal
 │
 ▼
Move
```

---

# Initial Policy

```text
confidence >= 0.85
AND
margin >= 0.40

=> AUTO
```

```text
confidence >= 0.60

=> REVIEW
```

それ以下:

```text
LEAVE
```

閾値は将来実データから調整する。

---

# Dry Run

初期値:

```text
dry_run = true
```

ユーザーが明示的に実行するまで移動しない。

---

# Transaction

移動前に記録:

```json
{
  "operation": "move",
  "source": "...",
  "destination": "...",
  "timestamp": "..."
}
```

記録成功後にmoveする。

---

# Collision

destinationに同名ファイルが存在する場合:

初期バージョンでは上書きしない。

```text
REVIEW
```

へ送る。

---

# Undo

```bash
kaseifu undo

kaseifu undo --last 10
```

を提供する。

---

# Modules

```text
src/policy/
src/operations/
src/storage/
```

---

# Completion Criteria

```bash
cargo run -- organize FILE --dry-run
```

で移動予定を確認できる。

その後、安全な明示オプションで実際に移動できる。

Undo可能であること。

