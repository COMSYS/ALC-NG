#![allow(dead_code)]

use std::{
    collections::{HashMap, HashSet},
    ffi::OsStr,
    hash::Hash,
    io::Error,
    path::{Path, PathBuf},
};

use serde::Serialize;

/// Represents a source file with its absolute (`inner_full`) and relative (`inner_relative`) paths.
#[derive(Debug, Clone, Serialize)]
pub struct SourceFile {
    inner_full: PathBuf,
    inner_relative: PathBuf,
}

impl SourceFile {
    /// Creates a new `SourceFile` from a given path `value` relative to a `parent` directory.
    /// The paths are canonicalized and the relative path is computed.
    pub fn from_path<P: AsRef<Path>, V: AsRef<Path>>(value: V, parent: P) -> std::io::Result<Self> {
        let buf = value.as_ref().to_path_buf().canonicalize()?;
        let full_parent = parent.as_ref().canonicalize()?;
        let relative = buf.strip_prefix(&full_parent).map_err(|_| {
            Error::new(
                std::io::ErrorKind::InvalidInput,
                "Parent not a prefix of value",
            )
        })?;

        Ok(Self {
            inner_relative: relative.to_path_buf(),
            inner_full: buf,
        })
    }

    /// Returns a reference to the relative path of the source file.
    pub fn relative(&self) -> &PathBuf {
        &self.inner_relative
    }

    /// Returns a reference to the absolute path of the source file.
    pub fn full(&self) -> &PathBuf {
        &self.inner_full
    }

    /// Returns the file extension of the source file, if any.
    pub fn extension(&self) -> Option<&OsStr> {
        self.inner_full.extension()
    }

    /// Returns the file name (with extension) of the source file, if any.
    pub fn file_name(&self) -> Option<&OsStr> {
        self.inner_full.file_name()
    }

    /// Returns the file stem (name without extension) of the source file, if any.
    pub fn file_stem(&self) -> Option<&OsStr> {
        self.inner_full.file_stem()
    }

    /// Consumes the `SourceFile` and returns the relative path.
    pub fn into_relative(self) -> PathBuf {
        self.inner_relative
    }

    /// Consumes the `SourceFile` and returns the absolute path.
    pub fn into_full(self) -> PathBuf {
        self.inner_full
    }
}

impl PartialEq for SourceFile {
    /// Equality is based on the relative path of the source file.
    fn eq(&self, other: &Self) -> bool {
        self.inner_relative == other.inner_relative
    }
}

impl Eq for SourceFile {}

impl Hash for SourceFile {
    /// Hash is computed from the relative path of the source file.
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.inner_relative.hash(state);
    }
}

/// Trait for grouping source files by a string key.
pub trait GroupSourceFiles {
    fn group_source_files(&self) -> HashMap<String, usize>;
}

impl GroupSourceFiles for HashSet<SourceFile> {
    /// Groups source files by their extension or file name,
    /// excluding certain readme files, and counts occurrences.
    fn group_source_files(&self) -> HashMap<String, usize> {
        use itertools::Itertools;

        self.iter()
            .filter(|v| {
                v.file_name()
                    .map(|name| {
                        name != "00README.json"
                            && name != "00readme.json"
                            && name != "00readme.yaml"
                            && name != "00readme.yml"
                    })
                    .unwrap_or(false)
            })
            .filter_map(|v| v.extension().or(v.file_name()))
            .filter_map(|v| v.to_str())
            .map(|v| v.to_string())
            .chunk_by(|v| v.clone())
            .into_iter()
            .map(|(k, g)| (k, g.count()))
            .collect()
    }
}

impl AsRef<Path> for SourceFile {
    /// Provides a `&Path` reference to the absolute path.
    fn as_ref(&self) -> &Path {
        self.inner_full.as_path()
    }
}

impl AsRef<SourceFile> for SourceFile {
    /// Provides a reference to self.
    fn as_ref(&self) -> &SourceFile {
        self
    }
}
