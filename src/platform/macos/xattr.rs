use anyhow::Result;
use serde_json::Value;
use std::path::Path;

use crate::models::file_context::{Evidence, EvidenceSource};

/// Binary な extended attribute であり、Value::String にしてはいけない
///
/// macOS の Xattr の一部は binary 形式で符号化されているため、文字列として出力しない。
///
/// 除外対象:
/// - com.apple.macl (MAC 拡張情報 - binary)
/// - com.apple.quarantine (quarantine info - binary)
pub fn is_binary(name: &str) -> bool {
    matches!(name, "com.apple.quarantine" | "com.apple.macl")
}

/// Collect evidence from xattr, filtering out binary attributes
///
/// - com.apple.macl, com.apple.quarantine のような binary な Xattr は skip する
/// - NULL 終端文字列 という形式で UTF-8 エンコード可能な文字列のみ Text::String として処理
/// - Binary attribute は Evidence に含めない（Jev 向けの非推奨なデータを取り除く）
pub fn collect(path: &Path) -> Result<Vec<Evidence>> {
    let mut evidence = Vec::new();

    let all_attrs = xattr::list(path)?;

    for name in all_attrs {
        let key = name.to_string_lossy().into_owned();

        // Binary attribute かどうかチェック
        if is_binary(&key) {
            // Binary attributes を skip
            continue;
        }

        // テキストを取得
        let value = xattr::get(path, &name)?;

        if let Some(value_bytes) = value {
            // NULL 終端文字列として読み込めるかチェック
            let null_end = value_bytes.iter().position(|&b| b == 0);

            if let Some(end_pos) = null_end {
                // NULL 終端文字列としてパース可能かつ、UTF-8 としてパース可能ならば、Value::String を使用
                let text_bytes = &value_bytes[..end_pos];
                let text: String = String::from_utf8(text_bytes.to_vec())?;
                evidence.push(Evidence {
                    source: EvidenceSource::Xattr,
                    key,
                    value: Value::String(text),
                    attribute: None,
                });
            }
            // NULL 終端文字列として読み込めない（binary）→ Evidence に含めない
        }
    }

    Ok(evidence)
}
