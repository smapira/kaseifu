
# Data Model

## 1. RawFileEvidence

macOSから取得した生情報。

Jevには送信しない。

```text
RawFileEvidence
├── filesystem
├── directory
├── spotlight
└── xattr
```

---

## 2. NormalizedFileContext

正規化済み情報。

```text
NormalizedFileContext
├── target
├── semantic_signals
├── provenance
└── directory_summary
```

---

## 3. SimilarFile

```text
SimilarFile
├── path
├── matched_terms
├── similarity
└── source
```

---

## 4. CandidateDestination

```text
CandidateDestination
├── id
├── path
├── score
├── evidence_count
└── reasons
```

---

## 5. DecisionContext

Jev送信用。

```text
DecisionContext
├── target
├── semantic_signals
└── candidates
```

個人情報・不要なRaw metadataは極力含めない。

---

## 6. JevDecision

```text
JevDecision
├── destination_id
├── probabilities
├── confidence
└── margin
```

marginはRust側で計算する。

---

## 7. Operation

```text
Operation
├── id
├── source
├── destination
├── decision
├── timestamp
└── undone_at
```

SQLiteへ保存する。

