use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use crate::helper::SourceFile;

/// Parses a `.fls` file located at `path`, resolving relative paths against `base_path`
/// and returns a tuple of two `HashSet<SourceFile>`:
/// * the first set contains all input files, and
/// * the second set contains all output files.
///
/// The function reads the file line by line, looking for lines that start with
/// `"INPUT "` or `"OUTPUT "`. For each matching line it creates a `SourceFile`
/// from the path, converting relative paths to absolute ones using `base_path`.
/// Lines that do not match these prefixes are ignored.
///
/// # Errors
/// Any I/O error encountered while opening or reading the file is returned.
pub fn parse_fls<P: AsRef<Path>, B: AsRef<Path>>(
    path: P,
    base_path: B,
) -> std::io::Result<(HashSet<SourceFile>, HashSet<SourceFile>)> {
    use itertools::{Either, Itertools};
    use std::fs::File;
    use std::io::{BufRead, BufReader};

    #[derive(Debug)]
    enum FileType {
        Input(SourceFile),
        Output(SourceFile),
    }

    let path = path.as_ref();
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let base_folder = base_path.as_ref();

    Ok(reader
        .lines()
        .filter_map(|v| {
            if let Ok(line) = v {
                if let Some(input_content) = line.strip_prefix("INPUT ") {
                    let mut pb = PathBuf::from(input_content);
                    if pb.is_relative() {
                        pb = base_folder.to_path_buf().join(pb);
                    }
                    return Some(FileType::Input(
                        SourceFile::from_path(pb, base_folder).ok()?,
                    ));
                } else if let Some(output_content) = line.strip_prefix("OUTPUT ") {
                    let mut pb = PathBuf::from(output_content);
                    if pb.is_relative() {
                        pb = base_folder.to_path_buf().join(pb);
                    }

                    return Some(FileType::Output(
                        SourceFile::from_path(pb, base_folder).ok()?,
                    ));
                }

                return None;
            }

            None
        })
        .partition_map(|file_type| match file_type {
            FileType::Input(content) => Either::Left(content),
            FileType::Output(content) => Either::Right(content),
        }))
}
