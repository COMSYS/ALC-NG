use std::path::Path;
use std::{collections::HashSet, fs::read};

use crate::helper::{ResultOkWithWarning, SourceFile};

/// Searches the given `path` for LaTeX source files that represent the main entry point of a document.
///
/// This function iterates over all entries in the directory specified by `path`, filters for files with a
/// `.tex` extension, reads their contents, parses them, and checks whether they are considered main
/// TeX files according to the logic in `crate::parsing::is_main_tex`. The paths of the matching files
/// are then converted into `SourceFile` instances and collected into a `HashSet` that is returned.
///
/// # Errors
///
/// Any I/O errors encountered while reading the directory or the files are returned as an
/// `std::io::Result`. If a file cannot be parsed or does not meet the main TeX criteria, it is simply
/// omitted from the resulting set.
pub fn find_mains<P>(path: P) -> std::io::Result<HashSet<SourceFile>>
where
    P: AsRef<Path>,
{
    use crate::parsing::{is_main_tex, parse};
    use std::fs::read_dir;

    Ok(read_dir(&path)?
        .filter(|v| {
            if let Ok(entry) = v
                && let path = entry.path()
                && let Some(ext) = path.extension()
            {
                return ext == "tex";
            }

            false
        })
        .filter_map(Result::ok_with_warning)
        .filter_map(|entry| {
            let content = read(entry.path()).ok_with_warning()?;

            let parsed = parse(&content)?;
            if is_main_tex(&parsed, &content) {
                return SourceFile::from_path(entry.path(), &path).ok_with_warning();
            }

            None
        })
        .collect())
}
