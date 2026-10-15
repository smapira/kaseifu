# 残タスク: デバッグシナリオ W2〜W5 の実装

- status: done (moved from `open/` 2026-10-15)
- milestone: M3 - Workflow semantics
- next owner: none (verify at merge time)
- blocker: actionable
- 出典: `documents/plans/dev/20261013*/20261014d-*`（デバッグシナリオ設計 v1.1 / v1.2 と CLI シナリオ）
- 優先度: 中（製品コード修正ゼロでリスクを減らせる。ただし設計資料は古く、裏取りが必須）

## 現状

- W0（S05/S01/S04/S02/S06）と W1（S28/S03/S11/S12/S13/S09/S10）は `tests/v091_debug_scenarios_w0w1_test.rs` に実装済み（30 テスト、製品コード修正ゼロ）。
- **W2〜W5 は未実装を確認**（`tests/` に該当テストファイルなし）。
- 設計資料（`20261014d-*`）は HEAD `6819ac9`（v0.4.2 相当）時点の実測。現在（`f519188`）は v0.9.x まで進んでおり、**R ノートの多くが古くなっている**（例: R5 のジャーナル相対パスは ZC-3 で解消、R8 の `is_system_directory` は削除/統合、C08 の `completions` は実装済みで W5 実施可）。

## やること（順序は設計資料どおり）

- W2: S07 → S08
- W3: S14 → S27 → S21 → S22 → S23 → S24 → S25
- W4: S26 → S17 → S15 → S16 → S19 → S20
- W5: S18 → C01-C12 → C08
- 各シナリオについて、**先に現行コードで裏取り**し、期待値が古ければ修正してから実装する（設計の期待をそのままテストにしない）。

## 受け入れ条件

- 各グループのシナリオが `tests/` のテストとして存在し、緑である。
- 製品コードの修正は「シナリオが実際の不具合を見つけた場合のみ」。その場合は計画書に根拠を残す。
- 見つけた不具合はこのディレクトリの issue として 1 件ずつ起こす。

## 制約

- テストは隔離 HOME と一時ディレクトリのみ。実ユーザーのファイル・Keychain・ジャーナルを読み書きしない。
- 既存テストの assert を不要に書き換えない。テスト名の版数接頭辞は次の空き（`v097` まで使用済み → `v098`）。

## 検証

- `cargo test --all-targets --all-features` と `bash scripts/quality_check.sh`。

## Verification (2026-10-15, worktree `kaseifu`)

No product-code change: no scenario exposed a real bug, so no new issue was
filed (per the issue's own rule). Two environment findings, both non-bugs:

- Watch survives SIGINT here, but so does `sleep`: spawned shells inherit
  SIGINT-ignore in this environment. C11 pins SIGTERM prompt death instead.
- Temp dirs sit under `/var/folders`, which `is_excluded_destination`
  refuses as a system root tree (intended); discovery/`--to` tests use
  `target/` fixtures (phase4 pattern) or relative paths, documented in-file.

Recheck table (design -> current):

| Scenario | Result |
|---|---|
| W2 S07 | Covered by `v32_image_metadata_test` (35 green). No new test. |
| W2 S08 | Covered by `phase4_*` (401/422/500/transport/malformed + pipeline propagation). No new test. |
| W3 S14 | Delta pinned: system hits vanish, own parent excluded, D1 numbering (`v103`, 2 tests). `is_system_directory`/is gone (R8 resolved); stale R8 note recorded. |
| W3 S27 | Delta pinned: 50-cap after ranking, determinism, empty list (`v103`, 2 tests). |
| W3 S21 | New JSON contract (M1) supersedes the design's "no JSON" note; missing-path exit 0 + no journal + usage exit 2 pinned (`v103`, 2 tests; core in `v099`). |
| W3 S22 | Pinned: non-TTY setup fails fast with guidance, no TOML written (`v103`). |
| W3 S23/S16 | Pinned: status not-configured (text+JSON), remove-key quiet success (`v103`, + S16 vocab in `v106`). Keychain-interactive paths stay manual (env quirk). |
| W3 S24 | Pinned: unknown key / bad URL / range / API-key refusal + show without value (`v103`, 2 tests). |
| W3 S25 | Pinned: exit 0 with FAIL, `Not tested` connectivity, check names (`v103`). Drift: 9 checks now, not 6. |
| W4 S26 | Pinned: missing-path exit 1 + empty stdout, 120-entry dir capped at 100 (`v106`, 2 tests). |
| W4 S17/C09/C10 | Pinned: Japanese+space JSON byte-exact, `--to` round trip of a Japanese name (`v106`, 2 tests). |
| W4 S15 | Jev path covered by `v32_jev_sends_*`; FM path structurally pinned (no location key, `v106`). |
| W4 S19 | Pinned: 5 identical verdicts with a fixed mock (`v106`). Near-threshold flapping is input variance (R6), untested by design. |
| W4 S20 | Pinned: `kaseifu 0.9.0`, journal schema columns (`v106`, 2 tests). |
| W5 S18 | Pinned at coordinator level: 1000-event coalescing, 100-path bound, in-flight + self suppression (`v106`, 3 tests). |
| W5 C02-C07 | Pinned: version/usage/resolve-failure/bad-value exit codes, stream separation (`v106`, 2 tests; C01 in `v070`). |
| W5 C08 | Covered by `completion_test` (green). No new test. |
| W5 C11 | Pinned: SIGTERM prompt death, dry-run moves nothing, 0 journal rows (`v106`). |
| W5 C12 | Pinned: key absent from doctor/config/auth outputs incl. JSON (`v106`). |

- New suites: `tests/v103_debug_scenarios_w2w3_test.rs` (12 pass),
  `tests/v106_debug_scenarios_w4w5_test.rs` (16 pass). Prefix note: the issue
  text says `v098`, but `v098` was already taken by `v098_rename_timeout_test`,
  so W2/W3 took `v103` and W4/W5 took `v106` (both free `v099+`).
- `cargo clippy --all-targets --all-features -- -D warnings` clean;
  `cargo fmt --check` clean.
