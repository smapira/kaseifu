# plan-architect レビュー: AIリネーム P5 計画書（`20261014j-ai-rename-p5.md`）

レビュー方式: 本環境に `/plan-architect` スキルが無いため手順をなぞって実施（コードベース照合・再利用確認・制約検査・差し戻し判定）。照合は `read_file` / `grep` / `terminal` / `fetch` の実測のみに基づく。

## 判定: 条件付き承認（下記 A1〜A7 を計画書へ反映すること。反映後の再レビューは不要）

初版計画書の問題点として、(1) `resolve_date` が EXIF 正規化形を受理しないこと、(2) FS 日時優先では EXIF が `{date}` に選ばれないこと、の2点を見落としていた。いずれも本レビューで検出し、D3・I4・T1〜T3 に反映した（反映済み）。

## 確認済み項目

| 観点 | 結果 |
|---|---|
| マルチモーダル API の実測 | OK。`Attachment` が SDK（Xcode 26.6 / SDK 26.5、3 SDK 同一）に存在しないことはプローブ（typecheck 失敗）と swiftinterface grep（0 件）で確定。公式ドキュメント（fetch）は WWDC26 の新機能として `Attachment` を記述。配線見送りは正当で、推測実装の禁止とも整合する |
| FFI 契約 v1 維持 | OK。`path` は既送（原典 §3.4 の Request 例どおり）で将来 Swift が読む。未知フィールド無視（`contract.rs:9-12`）により互換拡張が可能。`schemas/` 無変更は正当 |
| 既存経路の再利用 | OK。`observe`（builder+normalize）→ `finish_plan`（EXIF capture・camera 参照済み `service.rs:304-316`）→ `execute_plan`（journal move 行＋TOCTOU）は無変更で乗る。`image_metadata::collect`・`normalize::image::validate`・`exif::parse_file` の二重実装なしは正当 |
| 新規依存なし | OK。`Cargo.toml` に追加なしで実装可能。Rust 画像デコード・画像 crate の禁止遵守は可能 |
| テスト隔離 | OK。一時 dir＋Fake 注入＋隔離 HOME（v092 と同方式）。実写真・実 Keychain・実補完 dir に触れない構成は可能 |
| 複雑度 | 条件付き。`plan_rename` の分岐追加は小関数分割で CC<16 を維持すること（実装時計測） |
| Vision OCR 送り | OK。原典「必要時」＋ §11.3「不要な OCR を実行しない」と整合。次回条件（Attachment 到来後の実測需要）の明記あり |

## 指摘（計画書へ反映すること。すべて反映済み）

- A1 `resolve_date` の受理形式を拡張すること。`src/rename/rules.rs:243-258` は `parse_from_rfc3339` のみで、EXIF 正規化形（`src/context/image_metadata.rs:320-348` が作る `YYYY-MM-DDTHH:MM:SS`、zone なし）を読み飛ばす。現状では EXIF 撮影日時が `{date}` に使われることは決してない。zone 無し wall-clock 形（`%Y-%m-%dT%H:%M:%S` と `%Y-%m-%d %H:%M:%S`）を受理する。zone の推測はしない（wall-clock の値だけ使う）。影響は画像の EXIF 値に限られる。→ D3 の (a)・T2b に反映した。
- A2 画像の日付優先順位を EXIF 先頭にすること。Phase 3+4 の順位（作成→更新→EXIF→文書内→現在）のままでは実ファイルの mtime が必ず勝ち、要件 #3（EXIF 撮影日時の `{date}` e2e）が成立しない。写真の FS 日時は転送時刻であり撮影日ではない。画像のみ EXIF→作成→更新→現在とし、文書内日付は画像では使わない（本文スキャンをしないため `document_ymd` は常に None）。文書の順位は不変。→ D3 の (b)・I4・T2 に反映した。
- A3 `classify_image` は `context::image_metadata::is_image_file` を再利用し、拡張子リストを二重に持たないこと（drift 防止）。→ T1 に反映した。
- A4 `rename_file` のゲートは画像拡張子を先に判定し、文書判定の既存メッセージ・文面を変えないこと。`document.rs` の画像向けエラー文面（「image rename arrives with Phase 5」）は到達不能になるが API として残し、文面変更しない（既存テスト保護）。→ T3 に反映した。
- A5 Swift プロンプト追加は `target_type == \"image\"` 分岐内に限り、文書プロンプトの差分ゼロを diff で保証すること。→ T4 に反映した。
- A6 v093 の EXIF e2e は決定的に書くこと。一時 dir は Spotlight 索引外（creation None）・mtime あり。A2 の順位により EXIF 付き TIFF の日付は決定的に EXIF になる。EXIF 無し PNG の日付断言は P01（title のみ）で避け、midnight またぎの flake を作らないこと。→ A6 として計画書 §4（D6・I4 注記）に含めた。実装時に遵守する。
- A7 HEIC は拡張子受理のみとし、実ファイル検証は PNG/TIFF に限ること（生成手段が OS 依存で再現性が低い）。Fake テストでは実体の小さい一時ファイルを `.HEIC` 名で使う（Rust はデコードしないため正当）。→ D6・I8 に反映済み。

## 残る注意（実装・レビュー工程への引き継ぎ）

- R1（reviewer へ）: `service.rs` の画像分岐と `rules.rs` の日時受理拡張は小関数に分割し、新規 CC>=16 を作らないこと。`plan_rename` 本体の CC 上昇に注意する。
- Q1（QA へ）: `cargo test` のほか、実機検証（合成 PNG/TIFF の dry-run・execute・undo、所要時間、画像サイズ、journal 行）を §9 に実測記録すること。視覚理解の未検証は残リスクとして明記する。
