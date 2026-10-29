//! Strict unified-diff parsing. The parser never guesses a line number: a
//! finding can only be anchored to a line that was actually present in a hunk.

use crate::domain::{DiffFile, DiffHunk, DiffLine, DiffLineKind, DiffStatus};
use crate::error::{GitPilotError, Result};

pub const MAX_DIFF_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_HUNK_LINES: usize = 30_000;
pub const MAX_HUNK_COUNT: usize = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLayout {
    Unified,
    SideBySide,
}

#[derive(Debug, Clone, Default)]
pub struct DiffStats {
    pub files: usize,
    pub additions: i64,
    pub deletions: i64,
    pub binary_files: usize,
    pub truncated: bool,
}

pub fn parse_patch(patch: &str) -> Result<Vec<DiffFile>> {
    if patch.len() > MAX_DIFF_BYTES {
        return Err(GitPilotError::InvalidInput(format!(
            "diff exceeds the {} MiB safety limit",
            MAX_DIFF_BYTES / 1024 / 1024
        )));
    }
    let mut files = Vec::new();
    let mut current: Option<PendingFile> = None;
    let mut current_hunk: Option<DiffHunk> = None;
    let mut hunk_line_count = 0usize;
    let mut hunk_count = 0usize;

    for line in patch.lines() {
        if line.starts_with("diff --git ") {
            if let Some(mut file) = current.take() {
                if let Some(hunk) = current_hunk.take() {
                    file.hunks.push(hunk);
                }
                files.push(file.finish());
            }
            let paths = parse_diff_header(line);
            let (old_path, path) = paths
                .map(|(old, path)| (Some(old), path))
                .unwrap_or_else(|| (None, "unknown".into()));
            current = Some(PendingFile {
                path,
                old_path,
                status: DiffStatus::Modified,
                binary: false,
                hunks: Vec::new(),
                raw: String::new(),
            });
            hunk_line_count = 0;
            continue;
        }
        let Some(file) = current.as_mut() else {
            continue;
        };
        file.raw.push_str(line);
        file.raw.push('\n');
        if line.starts_with("new file mode") {
            file.status = DiffStatus::Added;
            continue;
        }
        if line.starts_with("deleted file mode") {
            file.status = DiffStatus::Deleted;
            continue;
        }
        if let Some(path) = line.strip_prefix("rename from ") {
            file.old_path = Some(path.into());
            file.status = DiffStatus::Renamed;
            continue;
        }
        if let Some(path) = line.strip_prefix("rename to ") {
            file.path = path.into();
            file.status = DiffStatus::Renamed;
            continue;
        }
        if line.starts_with("Binary files ") || line == "GIT binary patch" {
            file.binary = true;
            continue;
        }
        if line.starts_with("@@ ") {
            hunk_count += 1;
            if hunk_count > MAX_HUNK_COUNT {
                return Err(GitPilotError::InvalidInput(
                    "diff exceeds the hunk-count safety limit".into(),
                ));
            }
            if let Some(hunk) = current_hunk.take() {
                file.hunks.push(hunk);
            }
            current_hunk = Some(parse_hunk_header(line)?);
            hunk_line_count = 0;
            continue;
        }
        if let Some(path) = line.strip_prefix("+++ b/") {
            if file.status != DiffStatus::Deleted {
                file.path = path.into();
            }
            continue;
        }
        if let Some(path) = line.strip_prefix("--- a/") {
            if file.old_path.is_none() || file.status == DiffStatus::Renamed {
                file.old_path = Some(path.into());
            }
            continue;
        }
        let Some(hunk) = current_hunk.as_mut() else {
            continue;
        };
        if hunk_line_count >= MAX_HUNK_LINES {
            continue;
        }
        let (kind, content) = match line.as_bytes().first() {
            Some(b'+') => (DiffLineKind::Added, &line[1..]),
            Some(b'-') => (DiffLineKind::Removed, &line[1..]),
            Some(b' ') => (DiffLineKind::Context, &line[1..]),
            Some(b'\\') => (DiffLineKind::Meta, line),
            None => (DiffLineKind::Meta, ""),
            _ => (DiffLineKind::Meta, line),
        };
        let old_line = match kind {
            DiffLineKind::Context | DiffLineKind::Removed => {
                let n = hunk.old_start + hunk.old_consumed();
                Some(n)
            }
            _ => None,
        };
        let new_line = match kind {
            DiffLineKind::Context | DiffLineKind::Added => {
                let n = hunk.new_start + hunk.new_consumed();
                Some(n)
            }
            _ => None,
        };
        hunk.lines.push(DiffLine {
            kind,
            content: content.into(),
            old_line,
            new_line,
        });
        hunk_line_count += 1;
    }
    if let Some(mut file) = current {
        if let Some(hunk) = current_hunk {
            file.hunks.push(hunk);
        }
        files.push(file.finish());
    }
    Ok(files)
}

pub fn parse_file_patch(
    path: &str,
    status: &str,
    previous_path: Option<String>,
    patch: &str,
) -> Result<DiffFile> {
    let mut file = parse_patch(patch)?
        .into_iter()
        .next()
        .unwrap_or_else(|| DiffFile {
            path: path.into(),
            old_path: previous_path.clone(),
            status: match status {
                "added" => DiffStatus::Added,
                "deleted" => DiffStatus::Deleted,
                "renamed" => DiffStatus::Renamed,
                _ => DiffStatus::Modified,
            },
            additions: 0,
            deletions: 0,
            binary: true,
            generated: false,
            hunks: Vec::new(),
            raw: Some(patch.into()),
        });
    if file.path == "unknown" {
        file.path = path.into();
    }
    if file.old_path.is_none() {
        file.old_path = previous_path;
    }
    file.status = match status {
        "added" => DiffStatus::Added,
        "deleted" => DiffStatus::Deleted,
        "renamed" => DiffStatus::Renamed,
        _ => file.status,
    };
    file.raw = Some(patch.into());
    Ok(file)
}

impl DiffHunk {
    fn old_consumed(&self) -> i64 {
        self.lines
            .iter()
            .filter(|l| matches!(l.kind, DiffLineKind::Context | DiffLineKind::Removed))
            .count() as i64
    }

    fn new_consumed(&self) -> i64 {
        self.lines
            .iter()
            .filter(|l| matches!(l.kind, DiffLineKind::Context | DiffLineKind::Added))
            .count() as i64
    }
}

struct PendingFile {
    path: String,
    old_path: Option<String>,
    status: DiffStatus,
    binary: bool,
    hunks: Vec<DiffHunk>,
    raw: String,
}

impl PendingFile {
    fn finish(self) -> DiffFile {
        let additions = self
            .hunks
            .iter()
            .flat_map(|h| &h.lines)
            .filter(|l| l.kind == DiffLineKind::Added)
            .count() as i64;
        let deletions = self
            .hunks
            .iter()
            .flat_map(|h| &h.lines)
            .filter(|l| l.kind == DiffLineKind::Removed)
            .count() as i64;
        let generated = is_generated(&self.path);
        DiffFile {
            path: self.path,
            old_path: self.old_path,
            status: self.status,
            additions,
            deletions,
            binary: self.binary,
            generated,
            hunks: self.hunks,
            raw: Some(self.raw),
        }
    }
}

fn parse_diff_header(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix("diff --git ")?;
    let parts = split_git_paths(rest);
    (parts.len() == 2).then(|| {
        (
            strip_git_prefix(parts[0]).to_owned(),
            strip_git_prefix(parts[1]).to_owned(),
        )
    })
}

fn split_git_paths(value: &str) -> Vec<&str> {
    if let Some(first_quote) = value.find('"') {
        let mut result = Vec::new();
        let mut start = 0;
        let mut in_quote = false;
        let mut escaped = false;
        for (index, ch) in value.char_indices() {
            if escaped {
                escaped = false;
            } else if ch == '\\' && in_quote {
                escaped = true;
            } else if ch == '"' {
                in_quote = !in_quote;
            } else if ch == ' ' && !in_quote {
                result.push(&value[start..index]);
                start = index + 1;
            }
        }
        result.push(&value[start..]);
        if result.len() > 1 && first_quote > 0 {
            return result;
        }
    }
    value.splitn(2, " b/").collect()
}

fn strip_git_prefix(path: &str) -> &str {
    path.trim_matches('"')
        .strip_prefix("a/")
        .or_else(|| path.trim_matches('"').strip_prefix("b/"))
        .unwrap_or(path.trim_matches('"'))
}

fn parse_hunk_header(line: &str) -> Result<DiffHunk> {
    let header = line
        .strip_prefix("@@ ")
        .ok_or_else(|| GitPilotError::InvalidInput("malformed hunk header".into()))?;
    let range_end = header
        .find("@@")
        .ok_or_else(|| GitPilotError::InvalidInput("malformed hunk header".into()))?;
    let ranges = &header[..range_end];
    let mut parts = ranges.split_whitespace();
    let old = parts
        .next()
        .ok_or_else(|| GitPilotError::InvalidInput("missing old hunk range".into()))?;
    let new = parts
        .next()
        .ok_or_else(|| GitPilotError::InvalidInput("missing new hunk range".into()))?;
    let (old_start, old_lines) = parse_range(old, '-')?;
    let (new_start, new_lines) = parse_range(new, '+')?;
    Ok(DiffHunk {
        old_start,
        old_lines,
        new_start,
        new_lines,
        header: line.into(),
        lines: Vec::new(),
    })
}

fn parse_range(value: &str, marker: char) -> Result<(i64, i64)> {
    let value = value
        .strip_prefix(marker)
        .ok_or_else(|| GitPilotError::InvalidInput(format!("expected {marker} range")))?;
    let (start, lines) = value.split_once(',').unwrap_or((value, "1"));
    let start = start
        .parse()
        .map_err(|_| GitPilotError::InvalidInput("invalid hunk start".into()))?;
    let lines = lines
        .parse()
        .map_err(|_| GitPilotError::InvalidInput("invalid hunk length".into()))?;
    Ok((start, lines))
}

pub fn is_generated(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".lock")
        || lower.ends_with(".min.js")
        || lower.ends_with(".min.css")
        || lower.contains("/vendor/")
        || lower.contains("/generated/")
        || lower.ends_with(".pb.go")
        || lower.ends_with("_generated.rs")
}

pub fn annotate_stats(files: &[DiffFile]) -> DiffStats {
    DiffStats {
        files: files.len(),
        additions: files.iter().map(|f| f.additions).sum(),
        deletions: files.iter().map(|f| f.deletions).sum(),
        binary_files: files.iter().filter(|f| f.binary).count(),
        truncated: files.iter().any(|f| f.hunks.len() > MAX_HUNK_LINES),
    }
}

pub fn describe_hunk(hunk: &DiffHunk) -> String {
    format!(
        "@@ -{},{} +{},{} @@",
        hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines
    )
}

pub fn validate_anchor(files: &[DiffFile], path: &str, line: i64) -> bool {
    files.iter().any(|file| {
        file.path == path
            && file.hunks.iter().any(|hunk| {
                hunk.lines.iter().any(|l| {
                    l.new_line == Some(line)
                        && matches!(l.kind, DiffLineKind::Added | DiffLineKind::Context)
                })
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATCH: &str = r#"diff --git a/src/auth.rs b/src/auth.rs
index 111..222 100644
--- a/src/auth.rs
+++ b/src/auth.rs
@@ -1,3 +1,4 @@
 use std::fmt;
-fn old() {}
+fn new() {}
+fn extra() {}
 // end
diff --git a/logo.png b/logo.png
index 333..444 100644
Binary files a/logo.png and b/logo.png differ
"#;

    #[test]
    fn parses_line_anchors_precisely() {
        let files = parse_patch(PATCH).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "src/auth.rs");
        assert_eq!(files[0].hunks[0].lines[2].new_line, Some(2));
        assert_eq!(files[0].hunks[0].lines[1].old_line, Some(2));
        assert!(validate_anchor(&files, "src/auth.rs", 3));
        assert!(!validate_anchor(&files, "src/auth.rs", 99));
        assert!(files[1].binary);
    }

    #[test]
    fn malformed_headers_are_rejected() {
        assert!(parse_patch("diff --git a/a b/a\n@@ broken").is_err());
    }
}
