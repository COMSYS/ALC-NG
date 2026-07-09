use std::{collections::HashSet, path::Path, sync::LazyLock};

use regex::Regex;

use crate::helper::{ResultOkWithWarning as _, SourceFile};

static PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?mi)Latexmk: Found bibliography file\(s\):\n(\s\s(\S*)\n)*^Latexmk:"#).unwrap()
});

static LINE_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?im)^\s\s(.*)$"#).unwrap());
static BST_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?im)The style file:\s*([\w./\\-]+\.(?:bst|bbx|cbx|dbx))"#).unwrap()
});

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
        .and_then(|c| c.get(0).map(|v| v.as_str()));

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

/// Finds referenced bibliography style files in the given content.
///
/// This function searches the `content` string for occurrences that match the
/// `BST_PATTERN` regular expression, extracts each BST file path, and resolves
/// them relative to the provided `parent` directory. The result is a set of
/// `SourceFile`s representing each referenced style file.
///
/// # Arguments
///
/// * `content` - The text to search for style file references.
/// * `parent` - The base directory used to resolve relative file paths.
///
/// # Returns
///
/// A `HashSet<SourceFile>` containing all referenced bibliography style files
/// found in the content. If none are found, an empty set is returned.
pub fn find_referenced_bsts(content: &str, parent: impl AsRef<Path>) -> HashSet<SourceFile> {
    BST_PATTERN
        .captures_iter(content)
        .filter_map(|c| c.get(1).map(|v| v.as_str()))
        .filter_map(|s| {
            SourceFile::from_path(parent.as_ref().join(s), parent.as_ref()).ok_with_warning()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const BTEX_LOG: &str = r#"-----------
Running 'bibtex  "paper.aux"'
------------
This is BibTeX, Version 0.99e (TeX Live 2026)
The top-level auxiliary file: paper.aux
The style file: splncs04-etal.bst
Database file #1: paper.bib
Warning--empty booktitle in Albrechtetal2021Homomorphic
Warning--empty booktitle in Anantharamanetal2026Evaluating
Warning--empty booktitle in Rahmanietal2026Collaborative
Warning--empty booktitle in Tempini2022PatientsLikeMe
(There were 4 warnings)
Latexmk: applying rule 'pdflatex'...
Rule 'pdflatex':  Reasons for rerun
Changed files or newly in use/created:
  paper.aux
  paper.out

------------"#;

    #[test]
    fn test_bst_pattern_matches_style_file() {
        let captures: Vec<_> = BST_PATTERN
            .captures_iter(BTEX_LOG)
            .filter_map(|c| c.get(1).map(|m| m.as_str()))
            .collect();

        assert_eq!(captures, vec!["splncs04-etal.bst"]);
    }

    #[test]
    fn test_bst_pattern_no_match() {
        let captures: Vec<_> = BST_PATTERN
            .captures_iter("no style files here")
            .filter_map(|c| c.get(1).map(|m| m.as_str()))
            .collect();

        assert!(captures.is_empty());
    }
}
