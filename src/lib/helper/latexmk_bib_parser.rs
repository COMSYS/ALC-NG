use std::{collections::HashSet, path::Path, sync::LazyLock};

use regex::Regex;

use crate::helper::{ResultOkWithWarning as _, SourceFile};

static PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?mi)Latexmk: Found bibliography file\(s\):\n(\s\s(\S*)\n)*^Latexmk:"#).unwrap()
});

static LINE_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?im)^\s\s(.*)$"#).unwrap());

/// Finds referenced bibliography files in the given content.
///
/// This function searches the `content` string for sections that match the
/// `PATTERN` regular expression, extracts each bibliography file path from
/// the matched block, and resolves them relative to the provided `parent`
/// directory. The result is a set of `SourceFile`s representing each
/// referenced bibliography file.
///
/// # Arguments
///
/// * `content` - The text to search for bibliography references.
/// * `parent` - The base directory used to resolve relative file paths.
///
/// # Returns
///
/// A `HashSet<SourceFile>` containing all referenced bibliography files
/// found in the content. If none are found, an empty set is returned.
pub fn find_referenced_bibs(content: &str, parent: impl AsRef<Path>) -> HashSet<SourceFile> {
    let large_match = PATTERN
        .captures(content)
        .map(|c| c.get(0).map(|v| v.as_str()))
        .flatten();

    match large_match {
        Some(m) => LINE_PATTERN
            .captures_iter(m)
            .filter_map(|c| c.get(1).map(|v| v.as_str()))
            .filter_map(|s| {
                SourceFile::from_path(parent.as_ref().join(s), parent.as_ref()).ok_with_warning()
            })
            .collect(),
        None => HashSet::new(),
    }
}
