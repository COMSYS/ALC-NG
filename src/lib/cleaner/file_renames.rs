use std::{
    collections::{HashMap, HashSet},
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result};
use log::{info, warn};
use tree_sitter::Node;

use crate::parsing::parse;

/// Renames of project files whose path contains spaces.
///
/// arXiv replaces spaces in file names with underscores, which breaks `\input`,
/// `\includegraphics` and similar references to those files. Renaming the files
/// up front and rewriting the references keeps the submission compilable.
#[derive(Debug, Default, Clone)]
pub struct FileRenames {
    /// Maps the original relative path to the new relative path.
    renames: HashMap<PathBuf, PathBuf>,
}

impl FileRenames {
    /// Computes the renames for the given relative paths of all project files.
    ///
    /// A file is not renamed if its new name would collide with another file.
    pub fn new<'a>(paths: impl IntoIterator<Item = &'a Path>) -> Self {
        let paths: HashSet<&Path> = paths.into_iter().collect();
        let mut renames = HashMap::new();
        let mut taken: HashSet<PathBuf> = paths.iter().map(|p| p.to_path_buf()).collect();

        let mut candidates: Vec<&Path> = paths
            .iter()
            .copied()
            .filter(|p| contains_space(p))
            .collect();
        // Sort for deterministic collision handling
        candidates.sort();

        for path in candidates {
            let Some(new_path) = replace_spaces(path) else {
                warn!("Not renaming {}: path is not valid UTF-8", path.display());
                continue;
            };

            if taken.contains(&new_path) {
                warn!(
                    "Not renaming {} to {}: a file with that name already exists",
                    path.display(),
                    new_path.display()
                );
                continue;
            }

            taken.insert(new_path.clone());
            renames.insert(path.to_path_buf(), new_path);
        }

        Self { renames }
    }

    /// Returns `true` if no file is renamed.
    pub fn is_empty(&self) -> bool {
        self.renames.is_empty()
    }

    /// Iterates over all renames as `(original, new)` relative paths.
    pub fn iter(&self) -> impl Iterator<Item = (&PathBuf, &PathBuf)> {
        self.renames.iter()
    }

    /// Returns the relative path a file is written to in the cleaned project.
    pub fn target(&self, relative: &Path) -> PathBuf {
        self.renames
            .get(relative)
            .cloned()
            .unwrap_or_else(|| relative.to_path_buf())
    }

    /// Rewrites a file reference as written in a LaTeX source, e.g. the argument of
    /// `\input` or `\includegraphics`, if it refers to a renamed file or directory.
    ///
    /// References may omit the file extension and may be relative to a directory
    /// set elsewhere (e.g. `\graphicspath` or `\import`), so a reference matches if
    /// it is a trailing part of a renamed path, with or without extension.
    /// Surrounding whitespace of the reference is preserved.
    pub fn rewrite_reference(&self, reference: &[u8]) -> Option<Vec<u8>> {
        let reference = std::str::from_utf8(reference).ok()?;
        let trimmed = reference.trim();
        let normalized = trimmed.trim_start_matches("./").trim_end_matches('/');

        if !normalized.contains(' ') || !self.references_renamed_path(normalized) {
            return None;
        }

        let start = reference.len() - reference.trim_start().len();
        let end = start + trimmed.len();

        let mut rewritten = String::with_capacity(reference.len());
        rewritten.push_str(&reference[..start]);
        rewritten.push_str(&trimmed.replace(' ', "_"));
        rewritten.push_str(&reference[end..]);

        Some(rewritten.into_bytes())
    }

    /// Rewrites all references to renamed files in a LaTeX source and leaves
    /// everything else untouched. Returns `None` if nothing was rewritten.
    ///
    /// This is used for files that are not cleaned (e.g. due to `% !ALC-NG keep-all`),
    /// as they would otherwise reference files that no longer exist.
    pub fn rewrite_references(
        &self,
        content: &[u8],
        root: Node,
        filename: &str,
    ) -> Option<Vec<u8>> {
        if self.is_empty() {
            return None;
        }

        let mut rewritten = Vec::with_capacity(content.len());
        let mut last_byte = 0;
        let mut changed = false;
        let mut stack = vec![root];

        // Visit nodes in document order
        while let Some(node) = stack.pop() {
            if node.kind() == "path" {
                let reference = &content[node.byte_range()];
                if let Some(new_reference) = self.rewrite_reference(reference) {
                    info!(
                        "Rewriting reference {:?} to {:?} in uncleaned file {}",
                        String::from_utf8_lossy(reference),
                        String::from_utf8_lossy(&new_reference),
                        filename
                    );
                    rewritten.extend_from_slice(&content[last_byte..node.start_byte()]);
                    rewritten.extend_from_slice(&new_reference);
                    last_byte = node.end_byte();
                    changed = true;
                }
                continue;
            }

            let mut cursor = node.walk();
            let children: Vec<_> = node.children(&mut cursor).collect();
            stack.extend(children.into_iter().rev());
        }

        if !changed {
            return None;
        }

        rewritten.extend_from_slice(&content[last_byte..]);
        Some(rewritten)
    }

    /// Copies a LaTeX source to `target`, rewriting references to renamed files.
    pub fn copy_with_rewritten_references(
        &self,
        source: &Path,
        target: &Path,
        filename: &str,
    ) -> Result<()> {
        if !self.is_empty() {
            let content = std::fs::read(source)
                .with_context(|| format!("Failed to read {}", source.display()))?;
            let tree =
                parse(&content).with_context(|| format!("Failed to parse {}", source.display()))?;

            if let Some(rewritten) = self.rewrite_references(&content, tree.root_node(), filename) {
                std::fs::write(target, rewritten)
                    .with_context(|| format!("Failed to write {}", target.display()))?;
                return Ok(());
            }
        }

        std::fs::copy(source, target)
            .with_context(|| format!("Failed to copy {}", source.display()))?;

        Ok(())
    }

    fn references_renamed_path(&self, reference: &str) -> bool {
        self.renames.keys().any(|original| {
            // The file itself, the file without extension and all parent directories
            let without_extension = original.with_extension("");
            std::iter::once(original.as_path())
                .chain(std::iter::once(without_extension.as_path()))
                .chain(original.ancestors().skip(1))
                .filter_map(to_slash_string)
                .any(|candidate| {
                    candidate == reference || candidate.ends_with(&format!("/{reference}"))
                })
        })
    }
}

fn contains_space(path: &Path) -> bool {
    path.as_os_str().as_encoded_bytes().contains(&b' ')
}

fn replace_spaces(path: &Path) -> Option<PathBuf> {
    path.components()
        .map(|component| match component {
            Component::Normal(name) => name.to_str().map(|n| PathBuf::from(n.replace(' ', "_"))),
            other => Some(PathBuf::from(other.as_os_str())),
        })
        .collect()
}

/// Joins the components with `/`, the separator used in LaTeX sources.
fn to_slash_string(path: &Path) -> Option<String> {
    let parts: Option<Vec<&str>> = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(name) => Some(name.to_str()),
            _ => None,
        })
        .collect();
    let joined = parts?.join("/");

    (!joined.is_empty()).then_some(joined)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn renames(paths: &[&str]) -> FileRenames {
        FileRenames::new(paths.iter().map(Path::new))
    }

    #[test]
    fn renames_files_with_spaces() {
        let r = renames(&["main.tex", "my figure.png", "sub dir/a b.tex"]);

        assert_eq!(r.target(Path::new("main.tex")), Path::new("main.tex"));
        assert_eq!(
            r.target(Path::new("my figure.png")),
            Path::new("my_figure.png")
        );
        assert_eq!(
            r.target(Path::new("sub dir/a b.tex")),
            Path::new("sub_dir/a_b.tex")
        );
    }

    #[test]
    fn skips_collisions() {
        let r = renames(&["a b.tex", "a_b.tex"]);

        assert!(r.is_empty());
        assert_eq!(r.target(Path::new("a b.tex")), Path::new("a b.tex"));
    }

    #[test]
    fn rewrites_references() {
        let r = renames(&["main.tex", "figures/my figure.png", "my dir/sec 1.tex"]);

        let rewrite = |s: &str| {
            r.rewrite_reference(s.as_bytes())
                .map(|v| String::from_utf8(v).unwrap())
        };

        assert_eq!(
            rewrite("figures/my figure.png").as_deref(),
            Some("figures/my_figure.png")
        );
        assert_eq!(
            rewrite("figures/my figure").as_deref(),
            Some("figures/my_figure")
        );
        assert_eq!(
            rewrite("./figures/my figure").as_deref(),
            Some("./figures/my_figure")
        );
        // Relative to \graphicspath
        assert_eq!(rewrite("my figure").as_deref(), Some("my_figure"));
        // Directory, e.g. in \import
        assert_eq!(rewrite("my dir/").as_deref(), Some("my_dir/"));
        assert_eq!(rewrite("my dir/sec 1").as_deref(), Some("my_dir/sec_1"));
        // Surrounding whitespace is kept
        assert_eq!(rewrite(" my figure ").as_deref(), Some(" my_figure "));

        assert_eq!(rewrite("main"), None);
        assert_eq!(rewrite("other file"), None);
        assert_eq!(rewrite("figure"), None);
    }

    #[test]
    fn rewrites_references_in_source() {
        let r = renames(&["main.tex", "part one.tex"]);
        let content = b"% !ALC-NG keep-all\n\\input{part one} % keep me\n\\input{other file}\n";
        let tree = parse(content).unwrap();

        let rewritten = r.rewrite_references(content, tree.root_node(), "test.tex");

        assert_eq!(
            rewritten.as_deref(),
            Some(&b"% !ALC-NG keep-all\n\\input{part_one} % keep me\n\\input{other file}\n"[..])
        );
        assert_eq!(
            renames(&["a b.tex"]).rewrite_references(
                b"no refs",
                parse(b"no refs").unwrap().root_node(),
                "t"
            ),
            None
        );
    }
}
