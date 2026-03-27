use std::{collections::HashSet, path::Path};

use crate::helper::{ResultOkWithWarning as _, SourceFile};

/// Parses a dependency file content and returns a set of `SourceFile` objects.
///
/// The content is expected to contain entries like `*{file}{path}` or `*{class}{path}`.
/// For each entry, it constructs an absolute path relative to `base_folder` and
/// converts it to a `SourceFile` using `SourceFile::from_path`. Entries that
/// do not match the expected categories are ignored.
///
/// Returns a `HashSet<SourceFile>` containing the successfully parsed source files.
pub fn parse_dep_file<P: AsRef<Path>>(content: &str, base_folder: P) -> HashSet<SourceFile> {
    use regex::Regex;
    use std::sync::LazyLock;

    static RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"\*\{(file|class)\}\s*\{([^\}]+)\}"#).unwrap());

    RE.captures_iter(content)
        .filter_map(|cap| Some((cap.get(1)?.as_str(), cap.get(2)?.as_str())))
        .filter_map(|(category, path)| {
            let path = match category {
                "file" => base_folder.as_ref().join(path),
                "class" => base_folder.as_ref().join(path).with_extension("cls"),
                _ => return None,
            };

            let res = SourceFile::from_path(path, &base_folder);
            res.ok_with_warning()
        })
        .collect()
}
