#![allow(dead_code)]

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::LazyLock,
};

use itertools::Itertools;
use regex::Regex;

static TEX_INPUT_1: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\\input\{([^}]+)}").unwrap());

static TEX_INPUT_2: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\\input\s+([^\x00-\x1F\x7F/\s\r\n]+)").unwrap());

static PACKAGE_NAME_PICKER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\\(?:RequirePackage|usepackage)(?:\[.*?])?\{([^}]+)}").unwrap());

static INCLUDEGRAPHICS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\\includegraphics(?:\[.*?])?\{([^}]+)}").unwrap());

fn find_tex_input(input_line: &str) -> Option<String> {
    if !input_line.contains("\\input") {
        return None;
    }

    for tex_input_re in [&*TEX_INPUT_1, &*TEX_INPUT_2] {
        if let Some(captures) = tex_input_re.captures(input_line) {
            if let Some(matched) = captures.get(1) {
                return Some(matched.as_str().trim().to_string());
            }
        }
    }
    None
}

fn find_tex_thing(tex_line: &str, pattern: &Regex, needles: &[&str]) -> Option<String> {
    let tex_line = tex_line.trim();

    // Check if line starts with comment
    if tex_line.starts_with('%') {
        return None;
    }

    // Check if any needle is present in the line
    if !needles.iter().any(|needle| tex_line.contains(needle)) {
        return None;
    }

    // Search for pattern match
    if let Some(captures) = pattern.captures(tex_line) {
        if let Some(matched) = captures.get(1) {
            return Some(matched.as_str().to_string());
        }
    }

    None
}

fn find_include_graphics_filename(tex_line: &str) -> Option<String> {
    find_tex_thing(tex_line, &INCLUDEGRAPHICS_RE, &["\\includegraphics"])
}

fn find_used_files(tex_files: &[PathBuf]) -> HashSet<PathBuf> {
    use std::fs::read_to_string;
    let scoopers = [find_tex_input, find_include_graphics_filename];

    let file_contents = tex_files.iter().map(read_to_string).filter_map(Result::ok);

    file_contents
        .filter_map(|content| {
            let lines: Vec<_> = content.lines().collect();
            for lineno in 0..lines.len() {
                let multiline = &lines[lineno..lineno + 2].iter().map(|v| v.trim()).join("");

                for scooper in &scoopers {
                    if let Some(used) = scooper(&multiline) {
                        return Some(PathBuf::from(used));
                    }
                }
            }
            None
        })
        .collect()
}

pub(crate) fn find_unused_toplevel_files<P, F>(in_dir: P, tex_files: &[F]) -> HashSet<PathBuf>
where
    P: AsRef<Path>,
    F: AsRef<Path>,
{
    use std::fs::read_dir;

    let in_dir = in_dir.as_ref();

    let in_dir_files: HashSet<_> = read_dir(in_dir)
        .unwrap_or_else(|_| panic!("Failed to read directory: {:#?}", in_dir))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();

    let full_tex_files: Vec<_> = tex_files
        .iter()
        .map(|f| Path::new(in_dir).join(f))
        .collect();

    let used_files = find_used_files(&full_tex_files);
    let unused_files: HashSet<_> = in_dir_files.difference(&used_files).cloned().collect();

    unused_files
}
