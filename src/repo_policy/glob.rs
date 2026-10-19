//! Minimal glob vocabulary for repository policy rules.
//!
//! A pattern is matched against the path of the target file relative to the
//! directory that holds `.kaseifu.toml` (`content/project/brief.md`), and the
//! whole path must match. The vocabulary is deliberately small because the
//! pattern is written by hand in a committed file:
//!
//! | Token | Matches |
//! |---|---|
//! | `?` | exactly one character, never `/` |
//! | `*` | zero or more characters, never `/` |
//! | `**` | zero or more characters, including `/` |
//! | `**/` | the same, and may also match nothing (`**/x` matches `x`) |
//! | `{name}` | one or more characters, never `/`, captured as `name` |
//!
//! Everything else is literal. There is no escape character, so a pattern that
//! needs a literal `{`, `}`, `*` or `?` cannot be written; no rule in the
//! reviewed policies needed one, and an escape syntax would be one more thing
//! to get wrong in a file nobody tests before committing.
//!
//! Wildcards are greedy: each one takes the longest run it can and backtracks
//! only when the rest of the pattern cannot match. A match gives up after
//! [`MAX_MATCH_STEPS`] steps so a pathological pattern cannot burn CPU.

/// Token/path steps one match may take before it is reported as no match.
///
/// Real patterns are well under 100 characters and real paths under 500, so a
/// successful match needs a few thousand steps at most; the budget only ever
/// stops a pattern built to backtrack exponentially.
const MAX_MATCH_STEPS: usize = 200_000;

/// Longest accepted capture name (a placeholder is a path segment, not a
/// sentence).
const MAX_CAPTURE_NAME: usize = 32;

/// A compiled glob pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Glob {
    source: String,
    tokens: Vec<Token>,
    names: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Token {
    /// One literal character.
    Literal(char),
    /// `?`
    One,
    /// `*`
    Segment,
    /// `**`
    Deep,
    /// `**/`
    DeepDirs,
    /// `{name}`, addressed by its slot in [`Glob::names`].
    Capture(usize),
}

/// The captured texts of one successful match.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Captures {
    names: Vec<String>,
    values: Vec<Option<String>>,
}

impl Captures {
    /// The text captured for `name`, if the pattern declares it and matched.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.names
            .iter()
            .position(|candidate| candidate == name)
            .and_then(|index| self.values[index].as_deref())
    }
}

impl Glob {
    /// Compile `pattern` or say what is wrong with it (ASCII, one sentence).
    pub fn compile(pattern: &str) -> Result<Self, String> {
        if pattern.is_empty() {
            return Err("match_glob must not be empty".to_string());
        }
        if pattern.starts_with('/') {
            return Err(format!(
                "match_glob `{pattern}` must be relative to the repository root"
            ));
        }
        let chars: Vec<char> = pattern.chars().collect();
        let mut tokens = Vec::new();
        let mut names: Vec<String> = Vec::new();
        let mut index = 0;
        while index < chars.len() {
            match chars[index] {
                '?' => {
                    tokens.push(Token::One);
                    index += 1;
                }
                '*' if chars.get(index + 1) == Some(&'*') => {
                    if chars.get(index + 2) == Some(&'/') {
                        tokens.push(Token::DeepDirs);
                        index += 3;
                    } else {
                        tokens.push(Token::Deep);
                        index += 2;
                    }
                }
                '*' => {
                    tokens.push(Token::Segment);
                    index += 1;
                }
                '{' => {
                    let close = chars[index + 1..]
                        .iter()
                        .position(|candidate| *candidate == '}')
                        .map(|offset| index + 1 + offset)
                        .ok_or_else(|| {
                            format!(
                                "match_glob `{pattern}` has an unclosed capture (expected `}}`)"
                            )
                        })?;
                    let name: String = chars[index + 1..close].iter().collect();
                    validate_capture_name(&name)?;
                    if names.contains(&name) {
                        return Err(format!(
                            "match_glob `{pattern}` captures `{name}` more than once"
                        ));
                    }
                    tokens.push(Token::Capture(names.len()));
                    names.push(name);
                    index = close + 1;
                }
                '}' => {
                    return Err(format!("match_glob `{pattern}` has a `}}` without `{{`"));
                }
                other => {
                    tokens.push(Token::Literal(other));
                    index += 1;
                }
            }
        }
        Ok(Self {
            source: pattern.to_string(),
            tokens,
            names,
        })
    }

    /// The pattern text as written (shown in the proposal).
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Every capture name the pattern declares, in order of appearance.
    pub fn capture_names(&self) -> &[String] {
        &self.names
    }

    /// Match a whole relative path; `None` when it does not match.
    pub fn matches(&self, path: &str) -> Option<Captures> {
        let chars: Vec<char> = path.chars().collect();
        let mut matcher = Matcher {
            glob: self,
            path: &chars,
            values: vec![None; self.names.len()],
            steps: MAX_MATCH_STEPS,
        };
        matcher.walk(0, 0).then(|| Captures {
            names: self.names.clone(),
            values: matcher.values,
        })
    }
}

/// One match in progress: the pattern, the path, the capture slots and the
/// remaining step budget.
///
/// The state lives in a struct (instead of flowing through every signature)
/// so each token family is one small function: a matcher that folds wildcard
/// backtracking, capture bookkeeping and the budget into one body is the
/// classic way a glob implementation grows into an unreadable one.
struct Matcher<'a> {
    glob: &'a Glob,
    path: &'a [char],
    values: Vec<Option<String>>,
    steps: usize,
}

impl Matcher<'_> {
    /// Match the tokens from `token_index` against the path from `char_index`.
    fn walk(&mut self, token_index: usize, char_index: usize) -> bool {
        if self.steps == 0 {
            return false;
        }
        self.steps -= 1;
        let Some(token) = self.glob.tokens.get(token_index).copied() else {
            return char_index == self.path.len();
        };
        match token {
            Token::Literal(expected) => self.literal(token_index, char_index, expected),
            Token::One => self.one(token_index, char_index),
            Token::Segment => self.spread(token_index, char_index, false),
            Token::Deep => self.spread(token_index, char_index, true),
            Token::DeepDirs => self.deep_dirs(token_index, char_index),
            Token::Capture(slot) => self.capture(token_index, char_index, slot),
        }
    }

    /// One literal character, then the rest of the pattern.
    fn literal(&mut self, token_index: usize, char_index: usize, expected: char) -> bool {
        self.path.get(char_index) == Some(&expected) && self.walk(token_index + 1, char_index + 1)
    }

    /// `?`: one character that is not a separator.
    fn one(&mut self, token_index: usize, char_index: usize) -> bool {
        self.path
            .get(char_index)
            .is_some_and(|candidate| *candidate != '/')
            && self.walk(token_index + 1, char_index + 1)
    }

    /// `*` (`allow_slash` false) and `**` (true): try every run length,
    /// longest first.
    fn spread(&mut self, token_index: usize, char_index: usize, allow_slash: bool) -> bool {
        for end in (char_index..=self.path.len()).rev() {
            if !allow_slash && self.path[char_index..end].contains(&'/') {
                continue;
            }
            if self.walk(token_index + 1, end) {
                return true;
            }
        }
        false
    }

    /// `**/`: the run may also be empty, so `**/x` matches `x`.
    fn deep_dirs(&mut self, token_index: usize, char_index: usize) -> bool {
        for end in (char_index..=self.path.len()).rev() {
            let boundary = end == char_index || self.path[end - 1] == '/';
            if boundary && self.walk(token_index + 1, end) {
                return true;
            }
        }
        false
    }

    /// `{name}`: one non-empty run without a separator, captured.
    fn capture(&mut self, token_index: usize, char_index: usize, slot: usize) -> bool {
        for end in (char_index + 1..=self.path.len()).rev() {
            if self.path[char_index..end].contains(&'/') {
                continue;
            }
            let previous = self.values[slot].take();
            self.values[slot] = Some(self.path[char_index..end].iter().collect());
            if self.walk(token_index + 1, end) {
                return true;
            }
            self.values[slot] = previous;
        }
        false
    }
}

/// One rule destination: literal text with `<name>` placeholders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    source: String,
    parts: Vec<Part>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    Literal(String),
    Capture(String),
}

impl Template {
    /// Parse `text` or say what is wrong with it (ASCII, one sentence).
    pub fn parse(text: &str) -> Result<Self, String> {
        let chars: Vec<char> = text.chars().collect();
        let mut parts = Vec::new();
        let mut literal = String::new();
        let mut index = 0;
        while index < chars.len() {
            match chars[index] {
                '<' => {
                    let close = chars[index + 1..]
                        .iter()
                        .position(|candidate| *candidate == '>')
                        .map(|offset| index + 1 + offset)
                        .ok_or_else(|| {
                            format!("destination `{text}` has an unclosed `<` (expected `>`)")
                        })?;
                    let name: String = chars[index + 1..close].iter().collect();
                    if name.is_empty() {
                        return Err(format!("destination `{text}` has an empty `<>`"));
                    }
                    validate_capture_name(&name)
                        .map_err(|message| format!("destination `{text}`: {message}"))?;
                    if !literal.is_empty() {
                        parts.push(Part::Literal(std::mem::take(&mut literal)));
                    }
                    parts.push(Part::Capture(name));
                    index = close + 1;
                }
                '>' => {
                    return Err(format!("destination `{text}` has a `>` without `<`"));
                }
                other => {
                    literal.push(other);
                    index += 1;
                }
            }
        }
        if !literal.is_empty() {
            parts.push(Part::Literal(literal));
        }
        Ok(Self {
            source: text.to_string(),
            parts,
        })
    }

    /// The destination text as written (used by the load-time path checks).
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Every `<name>` placeholder this template reads, in order.
    pub fn placeholders(&self) -> Vec<&str> {
        self.parts
            .iter()
            .filter_map(|part| match part {
                Part::Capture(name) => Some(name.as_str()),
                Part::Literal(_) => None,
            })
            .collect()
    }

    /// Substitute every placeholder with its captured text.
    ///
    /// A placeholder without a capture is kept literally: loading rejects that
    /// case, so an unresolved `<name>` can only appear if validation and
    /// expansion disagree, and showing the text is safer than silently
    /// dropping a path segment.
    pub fn expand(&self, captures: &Captures) -> String {
        let mut out = String::new();
        for part in &self.parts {
            match part {
                Part::Literal(text) => out.push_str(text),
                Part::Capture(name) => match captures.get(name) {
                    Some(value) => out.push_str(value),
                    None => {
                        out.push('<');
                        out.push_str(name);
                        out.push('>');
                    }
                },
            }
        }
        out
    }
}

/// Shared name rule for glob captures and destination placeholders.
fn validate_capture_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("a capture name must not be empty".to_string());
    }
    if name.chars().count() > MAX_CAPTURE_NAME {
        return Err(format!(
            "capture name `{name}` is longer than {MAX_CAPTURE_NAME} characters"
        ));
    }
    let mut chars = name.chars();
    let first = chars.next().unwrap_or('_');
    if !(first.is_ascii_alphabetic() || first == '_') {
        return Err(format!(
            "capture name `{name}` must start with a letter or `_`"
        ));
    }
    if !name
        .chars()
        .all(|candidate| candidate.is_ascii_alphanumeric() || candidate == '_' || candidate == '-')
    {
        return Err(format!(
            "capture name `{name}` may use letters, digits, `_` and `-` only"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(pattern: &str, path: &str) -> Option<Captures> {
        Glob::compile(pattern)
            .expect("pattern compiles")
            .matches(path)
    }

    #[test]
    fn segment_wildcard_never_crosses_a_slash() {
        assert!(matches("content/*", "content/brief.md").is_some());
        assert!(matches("content/*", "content/project/brief.md").is_none());
        assert!(matches("content/**", "content/project/brief.md").is_some());
    }

    #[test]
    fn deep_directory_prefix_matches_zero_or_more_directories() {
        assert!(matches("**/brief.md", "brief.md").is_some());
        assert!(matches("**/brief.md", "a/brief.md").is_some());
        assert!(matches("**/brief.md", "a/b/brief.md").is_some());
    }

    #[test]
    fn named_capture_takes_one_segment() {
        let captures =
            matches("content/{project}/*.md", "content/hearing/brief.md").expect("pattern matches");
        assert_eq!(captures.get("project"), Some("hearing"));
    }

    #[test]
    fn question_mark_matches_exactly_one_character() {
        assert!(matches("a?c", "abc").is_some());
        assert!(matches("a?c", "ac").is_none());
        assert!(matches("a?c", "a/c").is_none());
    }

    #[test]
    fn anonymous_wildcards_are_required_to_match_the_whole_path() {
        assert!(matches("content/*", "content/brief.md").is_some());
        assert!(matches("content/*", "content/brief.md.bak").is_some());
        assert!(matches("content/*.md", "content/brief.md.bak").is_none());
        assert!(matches("content/*", "docs/brief.md").is_none());
        assert!(matches("content/**", "content/a/brief.md").is_some());
        assert!(matches("content/**/brief.md", "content/brief.md").is_some());
    }

    #[test]
    fn compile_reports_the_first_authoring_mistake() {
        assert!(Glob::compile("").is_err());
        assert!(Glob::compile("/abs").is_err());
        assert!(Glob::compile("content/{project").is_err());
        assert!(Glob::compile("content/{a}/{a}").is_err());
        assert!(Glob::compile("content/{1bad}").is_err());
        assert!(Glob::compile("content/}").is_err());
    }

    #[test]
    fn template_expands_every_placeholder() {
        let captures = matches("content/{project}/**", "content/hearing/notes/brief.md")
            .expect("pattern matches");
        let template = Template::parse("sites/<project>/content").expect("template parses");
        assert_eq!(template.placeholders(), vec!["project"]);
        assert_eq!(template.expand(&captures), "sites/hearing/content");
    }

    #[test]
    fn template_reports_broken_placeholders() {
        assert!(Template::parse("sites/<project").is_err());
        assert!(Template::parse("sites/<>/x").is_err());
        assert!(Template::parse("sites/>/x").is_err());
    }
}
