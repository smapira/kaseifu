# CLI カバレッジ表（コマンド×シナリオ、サブコマンド・オプション）

原典要求（v1.2 §12-3/4/5/6）: ③ CLI コマンドとシナリオの対応表、④ サブコマンド・オプションのカバレッジ表、⑤ 既存 S01–S20 との重複・依存関係、⑥ P0/P1/P2 別優先順位。

実測基準: HEAD `6819ac9`、`--help` の 11 コマンド + 隠し `discover`。

---

## 1. 対象コマンドの確定（v1.2 §13 の「11 種類」との整合）

| # | コマンド | 一覧表示 | シナリオ | 区分 |
|---|---|---|---|---|
| 1 | `inspect` | あり | S26 (+S07/S17) | 公開 |
| 2 | `scan` | あり | S21 | 公開（実装はバナー表示のみ = F-11） |
| 3 | `organize` | あり | S01–S05、S09–S13、C03–C07 | 公開 |
| 4 | `undo` | あり | S01、S12、S13、C05、C06 | 公開 |
| 5 | `classify` | あり | S02、S03、S08、S19、C07 | 公開 |
| 6 | `watch` | あり | S28（S06/S18 を包含）、C03、C11 | 公開 |
| 7 | `setup` | あり | S22 | 公開（対話） |
| 8 | `auth` | あり | S23（+S16） | 公開（サブコマンド 3） |
| 9 | `config` | あり | S24（+S16） | 公開（サブコマンド 2） |
| 10 | `doctor` | あり | S25（+S16） | 公開 |
| 11 | `help` | あり | C01 | clap 組込（擬似コマンド） |
| (12) | `discover` | **なし（hide = true）** | S27（+S02/S14） | 隠し・デバッグ専用。**本設計では対象に含める** |
| (13) | `completions` | なし | C08 | **未実装**（並行タスクで追加中） |

判定: v1.2 §13 の「公開CLIコマンド11種類」は実測と一致する。`discover` は「公開ではないが実行可能」なので別枠で対象化し、`completions` は未実装として C08 のみで扱う（実装後の再評価とする）。

---

## 2. コマンド×シナリオの対応表

| コマンド | P0 | P1 | P2 | 主な観測点 |
|---|---|---|---|---|
| `organize` | S01、S04、S05、S09、S10、S11、S12、S13、S28（間接） | S02、S03、S07、S08、S17、S19、S20 | S18 | decision / policy / 移動結果 / ジャーナル行 |
| `watch` | S28、S06 | S18 | | watching 行 / ignored / detected / moved / pending |
| `classify` | S02、S03 | S07、S08、S19、S20 | | decision JSON / --verbose の確率分布 |
| `undo` | S01、S12、S13 | | | restored / nothing left to undo / undo 行 |
| `inspect` | | S26、S17 | | JSON キー集合 / entries 上限 / evidence |
| `discover` | | S27、S14 | | D1.. の順位とスコア / 除外件数 |
| `scan` | | S21 | | 1 行出力と exit 0 |
| `setup` | | S22 | | TOML のキー / Keychain の有無 / 部分状態 |
| `auth` | | S23、S16 | | Configured 語彙 / 秘密の非表示 |
| `config` | | S24、S16 | | show の行と JSON / set の拒否種別 |
| `doctor` | | S25、S16 | | 6 チェックの行 / connectivity 語彙 |
| `help` | | C01 | C01 | usage の集合 |
| （全コマンド） | C06、C12 | C01–C05、C07、C09–C11 | C08 | exit code / ストリーム分離 |

---

## 3. サブコマンド・オプションのカバレッジ表

`covered` の意味: `yes` = 既存テストで直接検証済み、`partial` = 構成要素のみ検証済み、`no` = 未検証（設計済み）。

| コマンド | サブコマンド / オプション | 実装 | シナリオ | covered | 備考 |
|---|---|---|---|---|---|
| （全体） | `--format <auto|rich|json>` | あり | C07、S26、S27 | partial | JSON を出さないコマンドあり（R9） |
| （全体） | `--jev-endpoint` / `--jev-model` | あり | S24、C05 | partial | resolver テストはあり、CLI 経由は未検証 |
| `inspect` | `<PATH>` 必須 | あり | S26、C03 | no | T05–T12 が手動 |
| `inspect` | `--all` / `--verbose` | あり | S26 | partial | renderer テストあり（v31） |
| `scan` | `<PATH>` 必須 | あり | S21 | no | バナー出力のみ |
| `organize` | `<FILE>` 必須 | あり | S01–S05、S09–S13 | partial | ライブラリ経路は検証済、CLI 未 |
| `organize` | `--execute` / `--dry-run`（排他） | あり | S09、C04 | partial | 排他は実測、E2E 未 |
| `undo` | `--last <N>`（既定 1） | あり | S13、C05 | partial | ループ挙動は未検証 |
| `discover` | `<FILE>` + `--all` / `--verbose`（hide） | あり | S27 | no | v042/v042c はライブラリ経路 |
| `classify` | `<FILE>` 必須 | あり | S02、S03、S08、S19 | partial | パイプラインは検証済、CLI 未 |
| `classify` | `--all` / `--verbose` | あり | C07 | partial | v31 で表示は検証済 |
| `watch` | `--path <PATHS>`（必須・複数可） | あり | S28、C03 | partial | コーディネータは検証済、実 CLI 未 |
| `watch` | `--execute` / `--dry-run`（排他） | あり | S09、S28 | partial | 同上 |
| `setup` | `--check-connection` | あり | S22、S25 | no | 対話のため自動化未 |
| `auth` | `set-key` | あり | S23 | partial | MemoryStore で検証済 |
| `auth` | `status` | あり | S23、S16 | partial | 実 Keychain 未・プロンプト懸念 |
| `auth` | `remove-key` / `--yes` | あり | S23 | partial | 同上 |
| `config` | `show` | あり | S24、S16 | partial | resolver 検証済 |
| `config` | `set <KEY> <VALUE>` | あり | S24、C05 | partial | loader 検証済、CLI 未 |
| `doctor` | なし | - | S25 | partial | doctor テスト 3 件 |
| `doctor` | `--check-connection` | あり | S25 | no | ネットワーク依存 |
| `help` | `[COMMAND]` | あり（clap） | C01 | no | |
| （未実装） | `completions` | **なし** | C08 | no | 並行タスク |

未カバー率の目安: 行数ベースで `no` 6 件 / `partial` 16 件 / `yes` 0 件。**CLI プロセスを起てるテストが 0 件**であることが最大の未カバー要因（表の `partial` の大半はライブラリ経路の検証）。

---

## 4. 既存 S01–S20 との重複・依存関係（v1.2 §12-5）

| 新規 | 既存 | 関係 | 統合の仕方 |
|---|---|---|---|
| S26（inspect） | S07（メタデータ欠損）、S17（日本語・空白） | 重複 + 依存 | S07/S17 の手順を共通ケースとして使い、S26 は CLI 契約（JSON キー、終了コード、上限）に限定 |
| S27（discover） | S02（未知 ID 拒否）、S14（システムパス除外） | 重複 + 依存 | 除外規則は S14 で固定し、S27 は候補リストの契約（順位・上限・空）を判定 |
| S28（watch） | S06（重複排除）、S18（大量ファイル） | 包含 | S28 は起動・停止・副作用（CLI 契約）。S28-11/12 は S06 を、負荷は S18 を参照して再実行 |
| S21（scan） | なし | 独立 | 現行契約（バナー）を固定。走査期待は S26/S27 へ移管 |
| S22（setup） | S16（Keychain） | 依存 | 保存順序の検証で S16 の観測を使う |
| S23（auth） | S16 | 包含 | S23 はライフサイクル、S16 は未設定時の非漏洩 |
| S24（config） | S03（閾値境界） | 依存 | S24-06 で閾値を変え、S03 の境界表がその値で再現することを確認 |
| S25（doctor） | S16 | 依存 | Keychain チェックは S16 と同じ観測点 |
| C01–C12 | 全 S | 横断 | 各 S の実行時に C06/C07/C12 を併せて観測してよい（重複実施を避ける） |
| C09/C10 | S17 | 実質同一 | 観測手順は S17 を正とする |
| C11 | S05、S28 | 実質同一 | 中断点の定義は S05、CLI 操作は S28 |

---

## 5. 優先度別一覧（P0 / P1 / P2）

| 優先度 | シナリオ | 件数 |
|---|---|---|
| P0 | S01、S02、S03、S04、S05、S06、S09、S10、S11、S12、S13、S28、C06、C12 | 14 |
| P1 | S07、S08、S14、S15、S16、S17、S19、S20、S21、S22、S23、S24、S25、S26、S27、C03、C04、C05、C07、C09、C10、C11 | 22 |
| P2 | S18、C01、C02、C08 | 4 |

優先度の根拠: P0 は「ファイルが消失しない / 上書きしない / 履歴と FS の不整合を検出できる / 不正な分類で移動しない」（v1.1 §1）に直接関わるもの、および v1.2 が P0 と指定した `S28`。C 系のうち終了コード（C06）と秘密情報（C12）は自動化・安全の契約なので P0 とした（v1.2 に優先度指定が無いため、これは設計上の提案である）。
