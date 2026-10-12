
# CLI Design

## inspect

Macから取得できるEvidenceを見る。

```bash
kaseifu inspect FILE
```

---

## discover

保存先候補を見る。

```bash
kaseifu discover FILE
```

Jevは呼ばない。

---

## classify

Jevまで実行する。

```bash
kaseifu classify FILE
```

ファイルは移動しない。

---

## organize

全パイプラインを実行する。

```bash
kaseifu organize FILE --dry-run
```

---

## undo

```bash
kaseifu undo
```

```bash
kaseifu undo --last 10
```

---

## watch

将来:

```bash
kaseifu watch ~/Desktop
```

---

# Debug Separation

このCLI分割により:

```text
inspect
   ↓
OS情報がおかしい？

discover
   ↓
候補探索がおかしい？

classify
   ↓
Jev判断がおかしい？

organize
   ↓
Policy/Moveがおかしい？
```

と問題箇所を切り分けられる。

