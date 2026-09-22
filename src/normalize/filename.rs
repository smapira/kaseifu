pub fn normalize_filename(input: &str) -> Vec<String> {
    let base = input
        .rsplit('.')
        .next()
        .map(|s| s.to_string())
        .unwrap_or_else(|| input.to_string());

    let cleans = remove_noise(&base);

    let cleans = cleans
        .split_whitespace()
        .filter(|term| !term.is_empty())
        .filter(|term| {
            let lower = term.to_lowercase();
            !lower.contains("recents")
                && !lower.contains("library")
                && !lower.contains("public")
                && !lower.contains("desktop")
                && !lower.contains("documents")
        })
        .map(|s| s.to_string())
        .collect();

    cleans
}

fn remove_noise(filename: &str) -> &str {
    // Handle special files BEFORE extension stripping (e.g., .DS_Store)
    if filename == ".DS_Store" || filename == ".localized" {
        return "";
    }

    if filename.starts_with("Screenshot ") {
        // Remove Screenshot prefix AND date/time portion for cleaner semantic terms
        // "Screenshot 2026-09-21 at 14-51-02 FILENAME.ext" -> "FILENAME"
        let after_screenshot = &filename[10..]; // after "Screenshot "
        if let Some(pos) = after_screenshot.find(" at ") {
            let after_date = &after_screenshot[10 + pos..]; // at 14-51-02 FILENAME.ext
            // Strip leading whitespace and return up to extension
            let trimmed = after_date.trim_start();
            return trimmed;
        }
        return &filename[10..];
    }
    filename
}
