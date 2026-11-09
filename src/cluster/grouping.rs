//! Deterministic first-pass clustering for `kaseifu cluster` (issue #26).
//!
//! The groups are decided here, with no model involved: a file's type and the
//! tokens of its name are enough to make the buckets reproducible, and that is
//! what lets a dry run be deterministic and testable. The model's role is the
//! *name* of a group (see `naming`), never the membership.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// What a member is, from its extension (a folder is its own kind).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Category {
    Image,
    Document,
    Archive,
    Data,
    Code,
    Audio,
    Video,
    Folder,
    Other,
}

impl Category {
    /// The category of one extension (lowercased by the caller).
    pub fn from_extension(extension: Option<&str>) -> Self {
        let Some(extension) = extension else {
            return Self::Other;
        };
        match extension.to_ascii_lowercase().as_str() {
            "png" | "jpg" | "jpeg" | "heic" | "gif" | "webp" | "tiff" | "tif" | "bmp" | "svg" => {
                Self::Image
            }
            "md" | "pdf" | "doc" | "docx" | "txt" | "rtf" | "pages" | "key" | "ppt" | "pptx"
            | "odt" | "numbers" | "xls" | "xlsx" => Self::Document,
            "zip" | "tar" | "gz" | "bz2" | "7z" | "rar" | "dmg" => Self::Archive,
            "csv" | "tsv" | "json" | "yaml" | "yml" | "toml" | "xml" | "sql" | "db" | "sqlite" => {
                Self::Data
            }
            "rs" | "py" | "js" | "ts" | "tsx" | "jsx" | "swift" | "c" | "h" | "cpp" | "go"
            | "rb" | "php" | "sh" | "html" | "htm" | "css" | "scss" => Self::Code,
            "mp3" | "wav" | "flac" | "m4a" | "aac" | "ogg" => Self::Audio,
            "mp4" | "mov" | "m4v" | "avi" | "mkv" | "webm" => Self::Video,
            _ => Self::Other,
        }
    }

    /// The journal/report word (and the fallback directory name).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Document => "document",
            Self::Archive => "archive",
            Self::Data => "data",
            Self::Code => "code",
            Self::Audio => "audio",
            Self::Video => "video",
            Self::Folder => "folder",
            Self::Other => "other",
        }
    }

    /// The classification of a file of this kind: what it *is* (`images`,
    /// `code`, ...), and `None` when nothing names it.
    ///
    /// This is not the folder a file goes to - see [`Category::class_folder`].
    /// Keeping the two apart means a report still says "this is code" for an
    /// html file while the file is filed under `documents` (issue #28).
    pub fn class_name(self) -> Option<&'static str> {
        Some(match self {
            Self::Image => "images",
            Self::Document => "documents",
            Self::Archive => "archives",
            Self::Data => "data",
            Self::Code => "code",
            Self::Audio => "audio",
            Self::Video => "videos",
            Self::Folder => "folders",
            // `Other` is the catch-all for a file nothing knows. Inventing a
            // word for it would put a bucket nobody asked for in the report.
            Self::Other => return None,
        })
    }

    /// The folder under the kind root that holds this kind (`images`,
    /// `documents`, ...), and `None` for a kind whose type nothing decides.
    ///
    /// A file the table does not name has no type folder: it is not turned into
    /// a `misc` bucket or filed under a `documents` level nobody asked for, and
    /// the plan puts it flat in the kind root (issue #28).
    pub fn class_folder(self) -> Option<&'static str> {
        Some(match self {
            Self::Image => "images",
            Self::Archive => "archives",
            Self::Audio => "audio",
            Self::Video => "videos",
            Self::Document | Self::Data | Self::Code | Self::Folder => "documents",
            Self::Other => return None,
        })
    }
}

/// One file (or folder) that can be grouped.
#[derive(Debug, Clone)]
pub struct Member {
    pub source: PathBuf,
    /// The entry name (file name, or folder name).
    pub name: String,
    pub category: Category,
    /// The extension that types this entry (lowercased), or `None` when the name
    /// has none. The extension is the type a group is filed under, so `html` and
    /// `sh` files are separate types instead of one `code` bucket (issue #28).
    pub extension: Option<String>,
    pub bytes: u64,
    pub tokens: BTreeSet<String>,
}

impl Member {
    /// Read one entry; `None` for anything that is not a regular file or a
    /// folder (a symlink, a socket, an unreadable entry).
    pub fn read(path: &Path) -> Option<Self> {
        let metadata = std::fs::symlink_metadata(path).ok()?;
        if metadata.file_type().is_symlink() {
            return None;
        }
        if !metadata.is_dir() && !metadata.is_file() {
            return None;
        }
        // A folder's dotted name is not an extension, so folders carry none.
        let extension = metadata.is_file().then(|| {
            path.extension()
                .map(|extension| extension.to_string_lossy().to_lowercase())
                .filter(|extension| !extension.is_empty())
        });
        let extension = extension.flatten();
        let category = if metadata.is_dir() {
            Category::Folder
        } else {
            Category::from_extension(extension.as_deref())
        };
        let name = path.file_name()?.to_string_lossy().to_string();
        Some(Self {
            source: path.to_path_buf(),
            tokens: tokens_of(path),
            name,
            category,
            extension,
            bytes: if metadata.is_file() {
                metadata.len()
            } else {
                0
            },
        })
    }
}

/// One group of members that will become one directory.
#[derive(Debug, Clone)]
pub struct Group {
    pub category: Category,
    /// The extension this group is typed by, or `None` when its members have
    /// none.
    pub extension: Option<String>,
    pub members: Vec<Member>,
    /// Tokens shared by at least two members, most common first.
    pub shared_tokens: Vec<String>,
}

impl Group {
    /// A group of members of one kind; the shared tokens are recomputed here so
    /// a caller that removed members (a destination filter) never carries a
    /// token no surviving member has.
    pub fn from_members(
        category: Category,
        extension: Option<String>,
        members: Vec<Member>,
    ) -> Self {
        let shared_tokens = shared_tokens(&members);
        Self {
            category,
            extension,
            members,
            shared_tokens,
        }
    }

    /// The classification of this group (`images`, `code`, ...), and `None`
    /// when nothing names it.
    pub fn class_name(&self) -> Option<&'static str> {
        self.category.class_name()
    }

    /// The folder this group's files are filed under, inside the kind root
    /// (`images`, `documents`, ...), and `None` when nothing types the group.
    pub fn class_folder(&self) -> Option<&'static str> {
        self.category.class_folder()
    }

    /// The word this group's type is named by: its extension, or the class
    /// folder when its files have none. `None` when neither says what the group
    /// is, which is the one case with no name of its own (issue #28).
    pub fn type_name(&self) -> Option<&str> {
        match &self.extension {
            Some(extension) => Some(extension),
            None => self.class_folder(),
        }
    }

    /// The deterministic candidate names, best first (the first is also the
    /// fallback when no model is available).
    ///
    /// A group with no type and no shared token has nothing to offer and gets an
    /// empty list: it is filed flat in its kind root rather than under an
    /// invented name.
    pub fn candidate_names(&self) -> Vec<String> {
        let mut candidates = Vec::new();
        if let Some(type_name) = self.type_name() {
            push_name(&mut candidates, type_name);
        }
        if let Some(token) = self.shared_tokens.first() {
            push_name(&mut candidates, token);
        }
        if self.shared_tokens.len() >= 2 {
            push_name(
                &mut candidates,
                &format!("{}-{}", self.shared_tokens[0], self.shared_tokens[1]),
            );
        }
        if let (Some(token), Some(type_name)) = (self.shared_tokens.first(), self.type_name())
            && token != type_name
        {
            push_name(&mut candidates, &format!("{token}-{type_name}"));
        }
        candidates
    }

    /// Bytes over the members that are files.
    pub fn bytes(&self) -> u64 {
        self.members.iter().map(|member| member.bytes).sum()
    }
}

/// Add a candidate unless it is empty or already there.
fn push_name(candidates: &mut Vec<String>, name: &str) {
    let name = sanitize_name(name);
    if !name.is_empty() && !candidates.contains(&name) {
        candidates.push(name);
    }
}

/// Lowercase, `[a-z0-9]` and unicode alphanumerics, single dashes, bounded.
///
/// A cluster name becomes a directory name, so it is sanitized here once and
/// never trusted again: separators, colons, control characters and leading
/// dashes cannot survive.
pub fn sanitize_name(name: &str) -> String {
    let mut out = String::new();
    for character in name.chars() {
        if character.is_alphanumeric() {
            out.extend(character.to_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-');
    let bounded: String = trimmed.chars().take(48).collect();
    bounded.trim_matches('-').to_string()
}

/// The tokens of an entry name (stem only), used for similarity and naming.
fn tokens_of(path: &Path) -> BTreeSet<String> {
    let Some(stem) = path.file_stem() else {
        return BTreeSet::new();
    };
    stem.to_string_lossy()
        .split(|character: char| !character.is_alphanumeric())
        .map(|token| token.to_lowercase())
        .filter(|token| token.len() >= 2 && !token.chars().all(|character| character.is_numeric()))
        .filter(|token| !STOP_WORDS.contains(&token.as_str()))
        .collect()
}

/// Words that carry no grouping signal (names are already short).
const STOP_WORDS: [&str; 14] = [
    "the", "and", "for", "with", "from", "this", "that", "copy", "final", "new", "old", "img",
    "dsc", "untitled",
];

/// Tokens present in at least two members, most common first.
fn shared_tokens(members: &[Member]) -> Vec<String> {
    let mut counts: BTreeMap<&String, usize> = BTreeMap::new();
    for member in members {
        for token in &member.tokens {
            *counts.entry(token).or_default() += 1;
        }
    }
    let mut shared: Vec<(&String, usize)> = counts
        .into_iter()
        .filter(|(_, count)| *count >= 2)
        .collect();
    shared.sort_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(right.0)));
    shared
        .into_iter()
        .take(4)
        .map(|(token, _)| token.clone())
        .collect()
}

/// Split members into groups: type first, then name-token similarity.
///
/// A bucket below `min` members is returned as ungrouped; a bucket above `max`
/// is split into evenly sized groups so no directory grows without bound.
pub fn group(
    members: Vec<Member>,
    min: usize,
    max: usize,
    by_type: bool,
) -> (Vec<Group>, Vec<Member>) {
    // One type is one class *and* one extension: `~/Documents/documents/html`
    // and `.../sh` are different types (issue #28).
    let mut typed: BTreeMap<(Category, Option<String>), Vec<Member>> = BTreeMap::new();
    for member in members {
        typed
            .entry((member.category, member.extension.clone()))
            .or_default()
            .push(member);
    }

    let mut groups = Vec::new();
    let mut ungrouped = Vec::new();
    for ((category, extension), mut members) in typed {
        members.sort_by(|left, right| left.name.cmp(&right.name));
        // `--by-type` skips the token pass: every file of one type is one group.
        let buckets = if by_type {
            vec![members]
        } else {
            buckets(members)
        };
        for bucket in buckets {
            if bucket.len() < min.max(1) {
                ungrouped.extend(bucket);
                continue;
            }
            groups.extend(split_into_chunks(
                category,
                &extension,
                bucket,
                max.max(min.max(1)),
            ));
        }
    }
    (groups, ungrouped)
}

/// Greedy buckets: a member joins the first bucket it is similar to.
fn buckets(members: Vec<Member>) -> Vec<Vec<Member>> {
    let mut buckets: Vec<Vec<Member>> = Vec::new();
    for member in members {
        let index = buckets
            .iter()
            .position(|bucket| bucket.iter().any(|existing| similar(existing, &member)));
        match index {
            Some(index) => buckets[index].push(member),
            None => buckets.push(vec![member]),
        }
    }
    buckets
}

/// True when two entries share enough name tokens to belong together.
///
/// An entry with no usable token (a camera name, a date) is grouped by type
/// alone instead of being isolated: the type is still real evidence.
fn similar(left: &Member, right: &Member) -> bool {
    if left.tokens.is_empty() || right.tokens.is_empty() {
        return true;
    }
    let intersection = left.tokens.intersection(&right.tokens).count();
    let union = left.tokens.union(&right.tokens).count();
    union > 0 && intersection as f64 / union as f64 >= SIMILARITY_THRESHOLD
}

/// Jaccard threshold for the first pass: one shared token in a three-token
/// name is enough, two unrelated names are not.
const SIMILARITY_THRESHOLD: f64 = 0.33;

/// Split a bucket into chunks of at most `max`, balanced over the chunks.
fn split_into_chunks(
    category: Category,
    extension: &Option<String>,
    members: Vec<Member>,
    max: usize,
) -> Vec<Group> {
    if members.len() <= max {
        return vec![Group::from_members(category, extension.clone(), members)];
    }
    let chunks = members.len().div_ceil(max);
    let size = members.len().div_ceil(chunks);
    members
        .chunks(size)
        .map(|chunk| Group::from_members(category, extension.clone(), chunk.to_vec()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(name: &str) -> Member {
        let extension = name
            .rsplit_once('.')
            .map(|(_, extension)| extension.to_lowercase());
        Member {
            source: PathBuf::from(name),
            name: name.to_string(),
            category: Category::from_extension(extension.as_deref()),
            extension,
            bytes: 1,
            tokens: tokens_of(Path::new(name)),
        }
    }

    #[test]
    fn the_type_decides_the_category() {
        assert_eq!(Category::from_extension(Some("PNG")), Category::Image);
        assert_eq!(Category::from_extension(Some("md")), Category::Document);
        // A spreadsheet is a document, not an unknown type (owner decision,
        // 2026-11-09): the type table answers for it and `cluster` files it
        // with the rest instead of dropping it flat as `kind: null`.
        assert_eq!(Category::from_extension(Some("xlsx")), Category::Document);
        assert_eq!(Category::from_extension(Some("xls")), Category::Document);
        assert_eq!(Category::from_extension(Some("zip")), Category::Archive);
        assert_eq!(Category::from_extension(None), Category::Other);
    }

    #[test]
    fn tokens_skip_numbers_and_stop_words() {
        let tokens = tokens_of(Path::new("the-CBD-2026-report final.md"));
        assert!(tokens.contains("cbd"));
        assert!(tokens.contains("report"));
        assert!(!tokens.contains("2026"));
        assert!(!tokens.contains("the"));
    }

    #[test]
    fn a_shared_token_groups_and_a_missing_one_does_not() {
        let left = member("cbd-report.md");
        let right = member("cbd-charts.md");
        let other = member("invoice.md");
        assert!(similar(&left, &right));
        assert!(!similar(&left, &other));
    }

    #[test]
    fn small_buckets_are_ungrouped_and_big_ones_are_split() {
        let members = vec![
            member("cbd-a.md"),
            member("cbd-b.md"),
            member("cbd-c.md"),
            member("cbd-d.md"),
            member("cbd-e.md"),
            member("alone.md"),
        ];
        let (groups, ungrouped) = group(members, 2, 2, false);
        assert_eq!(ungrouped.len(), 1);
        assert!(groups.iter().all(|group| group.members.len() <= 2));
        assert_eq!(
            groups
                .iter()
                .map(|group| group.members.len())
                .sum::<usize>()
                + 1,
            6
        );
    }

    #[test]
    fn by_type_keeps_dissimilar_files_of_one_kind_together() {
        let members = vec![member("alpha.png"), member("beta.png"), member("gamma.png")];
        let (by_token, token_ungrouped) = group(members.clone(), 2, 20, false);
        assert!(by_token.is_empty(), "unrelated names stay ungrouped");
        assert_eq!(token_ungrouped.len(), 3);

        let (by_type, ungrouped) = group(members, 2, 20, true);
        assert!(ungrouped.is_empty());
        assert_eq!(by_type.len(), 1);
        assert_eq!(by_type[0].members.len(), 3);
    }

    #[test]
    fn names_are_sanitized_and_candidates_are_stable() {
        assert_eq!(sanitize_name("CBD/Charts: 2026"), "cbd-charts-2026");
        assert_eq!(sanitize_name("..."), "");
        let members = vec![member("cbd-chart-a.png"), member("cbd-charts-b.png")];
        let group = Group::from_members(Category::Image, Some("png".to_string()), members);
        let names = group.candidate_names();
        // The extension is the type, so the type name is `png`, not `images`.
        assert_eq!(names[0], "png");
        assert!(names.contains(&"cbd".to_string()));
    }

    #[test]
    fn an_extension_and_a_class_are_separate_types() {
        let members = vec![member("cbd-a.html"), member("cbd-b.html")];
        let (groups, _) = group(members, 2, 20, false);
        // The classification says what the files are; the folder says where they
        // go. They are different questions (issue #28).
        assert_eq!(groups[0].class_name(), Some("code"));
        assert_eq!(groups[0].class_folder(), Some("documents"));
        assert_eq!(groups[0].type_name(), Some("html"));
    }

    #[test]
    fn files_the_table_does_not_name_have_no_type_folder() {
        let members = vec![member("cbd-a"), member("cbd-b")];
        let (groups, _) = group(members, 2, 20, false);
        // Nothing classifies them and nothing types them: no `misc` bucket, no
        // `documents` level - the plan files them flat in the kind root.
        assert_eq!(groups[0].class_name(), None);
        assert_eq!(groups[0].extension, None);
        assert_eq!(groups[0].class_folder(), None);
        assert_eq!(groups[0].type_name(), None);
        assert_eq!(groups[0].candidate_names(), vec!["cbd".to_string()]);
    }

    #[test]
    fn an_extension_nothing_classifies_is_still_the_group_s_name() {
        let members = vec![member("cbd-a.ttf"), member("cbd-b.ttf")];
        let (groups, _) = group(members, 2, 20, false);
        assert_eq!(groups[0].class_name(), None);
        assert_eq!(groups[0].class_folder(), None);
        assert_eq!(groups[0].type_name(), Some("ttf"));
    }
}
