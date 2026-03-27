#![allow(dead_code)]
use std::{
    collections::{BTreeMap, HashSet},
    fmt::Display,
    path::{Path, PathBuf},
    process::Command,
};

use serde::{Deserialize, Deserializer, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};

use crate::{
    arxiv::preflight::{BibCompiler, CompilerSpec, MainProcessSpec},
    helper::{ResultOkWithWarning as _, SourceFile},
};

#[derive(Debug)]
pub enum ZZRMException {
    FileNotFound(String),
    UnsupportedFile(String),
    UnsupportedFiletypeVersion(String),
    MultipleFiles(String),
    Key(String),
    Parse(String),
    InvalidFormat(String),
    Io(std::io::Error),
}

impl Display for ZZRMException {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ZZRMException::FileNotFound(path) => write!(f, "File not found: {}", path),
            ZZRMException::UnsupportedFile(path) => write!(f, "Unsupported file: {}", path),
            ZZRMException::UnsupportedFiletypeVersion(path) => {
                write!(f, "Unsupported filetype version: {}", path)
            }
            ZZRMException::MultipleFiles(path) => write!(f, "Multiple files found: {}", path),
            ZZRMException::Key(key) => write!(f, "Invalid key: {}", key),
            ZZRMException::Parse(err) => write!(f, "Parse error: {}", err),
            ZZRMException::InvalidFormat(format) => write!(f, "Invalid format: {}", format),
            ZZRMException::Io(err) => write!(f, "IO error: {}", err),
        }
    }
}

impl From<std::io::Error> for ZZRMException {
    fn from(err: std::io::Error) -> Self {
        ZZRMException::Io(err)
    }
}

#[derive(Deserialize, Debug)]
pub struct Process {
    pub compiler: CompilerSpec,
    pub bibliography: Bibliography,
    pub index: Index,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Bibliography {
    pub processor: BibCompiler,
    pub pre_generated: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Index {
    pub processor: String,
    pub pre_generated: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Source {
    pub filename: String,
    pub usage: FileUsage,
    pub orientation: Orientation,
    pub keep_comments: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "lowercase")]
pub enum FileUsage {
    Ignore,
    TopLevel,
    Include,
    Append,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "lowercase")]
pub enum Orientation {
    Portrait,
    Landscape,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct UserFile {
    pub filename: Option<String>,
    pub usage: Option<FileUsage>,
    pub orientation: Option<Orientation>,
    pub keep_comments: Option<bool>,
    pub fontmaps: Option<Vec<String>>,
}

#[derive(Serialize_repr, Deserialize_repr, PartialEq, Debug, Default)]
#[repr(u8)]
pub enum Version {
    /// Legacy, text based version
    #[default]
    V1 = 1,
    /// JSON/Yaml based
    V2 = 2,
}

impl Version {
    pub fn get_v2() -> Version {
        Version::V2
    }
}

#[derive(Deserialize, Debug, Default)]
pub struct ZeroZeroReadMe {
    #[serde(default = "Version::get_v2")]
    version: Version,
    pub process: MainProcessSpec,
    #[serde(deserialize_with = "ZeroZeroReadMe::sources_array_to_map")]
    pub sources: BTreeMap<String, UserFile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stamp: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    nohyperref: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    texlive_version: Option<u32>,
}

impl ZeroZeroReadMe {
    pub fn new_from_folder<P: AsRef<Path>>(folder: P) -> Result<Self, ZZRMException> {
        use glob::glob_with;
        let full_folder_path = PathBuf::from(folder.as_ref()).canonicalize()?;

        let mut candidates: Vec<_> = glob_with(
            &format!("{}/00README.*", full_folder_path.to_string_lossy()),
            glob::MatchOptions {
                case_sensitive: false,
                require_literal_separator: false,
                require_literal_leading_dot: false,
            },
        )
        .map_err(|err| ZZRMException::UnsupportedFile(err.to_string()))?
        .filter_map(Result::ok)
        .filter_map(|p| ZeroZeroReadMe::new_from_file(p).ok_with_warning())
        .collect();

        if candidates.len() > 1 {
            return Err(ZZRMException::MultipleFiles(
                "Multiple 00readme files found".into(),
            ));
        }

        candidates
            .pop()
            .ok_or(ZZRMException::FileNotFound("No 00readme file found".into()))
    }

    pub fn new_from_file<P>(file: P) -> Result<Self, ZZRMException>
    where
        P: AsRef<Path>,
    {
        let file = file.as_ref();
        let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let ext = file
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();

        if !stem.eq_ignore_ascii_case("00README") {
            return Err(ZZRMException::UnsupportedFile(
                "Filename must start with 00README".into(),
            ));
        }

        Ok(match ext.as_str() {
            // v1 extensions
            "txt" | "rst" | "readme" => Self::fetch_00readme_v1(file)?,
            // v2 extensions
            "json" | "jsn" | "ndjson" | "toml" | "yml" | "yaml" => Self::fetch_00readme_v2(file)?,
            _ => {
                return Err(ZZRMException::UnsupportedFile(format!(
                    "Unsupported extension: .{}",
                    ext
                )));
            }
        })
    }

    fn fetch_00readme_v1<P>(path: P) -> Result<Self, ZZRMException>
    where
        P: AsRef<Path>,
    {
        use std::fs::read_to_string;
        let data =
            read_to_string(path).map_err(|err| ZZRMException::FileNotFound(err.to_string()))?;

        if data.is_empty() {
            return Ok(Default::default());
        }

        let mut zzrm = Self::default();
        zzrm.version = Version::V1;

        for line in data.lines() {
            let idioms: Vec<&str> = line.trim().split_whitespace().collect();
            match idioms.len() {
                2 => {
                    let filename = idioms[0];
                    let keyword = idioms[1];
                    let mut userfile =
                        zzrm.sources
                            .get(filename)
                            .cloned()
                            .unwrap_or_else(|| UserFile {
                                filename: Some(filename.to_string()),
                                ..Default::default()
                            });

                    match keyword {
                        "ignore" => userfile.usage = Some(FileUsage::Ignore),
                        "include" => userfile.usage = Some(FileUsage::Include),
                        "keepcomments" => userfile.keep_comments = Some(true),
                        "landscape" => userfile.orientation = Some(Orientation::Landscape),
                        "toplevelfile" => userfile.usage = Some(FileUsage::TopLevel),
                        "append" => userfile.usage = Some(FileUsage::Append),
                        "fontmap" => {
                            // global option – add to process.fontmaps
                            zzrm.process
                                .fontmaps
                                .get_or_insert_with(|| vec![filename.to_string()]);
                        }
                        _ => {
                            // unknown keyword
                            return Err(ZZRMException::Key(format!("Unknown keyword {}", keyword)));
                        }
                    }

                    // default to toplevel if nothing else was set
                    if userfile.usage.is_none()
                        && userfile.keep_comments.is_none()
                        && userfile.orientation.is_none()
                        && userfile.fontmaps.is_none()
                    {
                        userfile.usage = Some(FileUsage::TopLevel);
                    }

                    if keyword != "fontmap" {
                        zzrm.sources.insert(filename.to_string(), userfile);
                    }
                }
                1 => match idioms[0] {
                    "nostamp" => zzrm.stamp = Some(false),
                    "nohyperref" => zzrm.nohyperref = Some(true),
                    _ => {}
                },
                _ => {}
            };
        }

        Ok(zzrm)
    }

    fn fetch_00readme_v2<P>(path: P) -> Result<Self, ZZRMException>
    where
        P: AsRef<Path>,
    {
        use std::fs::read_to_string;

        let path = path.as_ref();
        let ext = path
            .extension()
            .map(|v| v.to_str())
            .flatten()
            .ok_or(ZZRMException::Parse(
                "Could not determine extension for file".into(),
            ))?;

        let data =
            read_to_string(path).map_err(|err| ZZRMException::FileNotFound(err.to_string()))?;

        if data.is_empty() {
            return Ok(Default::default());
        }

        let mut zzrm: Self = match ext {
            "json" | "jsn" | "ndjson" => {
                serde_json::from_str(&data).map_err(|e| ZZRMException::Parse(e.to_string()))?
            }
            "toml" => toml::from_str(&data).map_err(|e| ZZRMException::Parse(e.to_string()))?,
            "yml" | "yaml" => {
                serde_yaml_ng::from_str(&data).map_err(|e| ZZRMException::Parse(e.to_string()))?
            }
            _ => {
                return Err(ZZRMException::UnsupportedFile(format!(
                    "Unsupported ext: {}",
                    ext
                )));
            }
        };

        zzrm.version = Version::V2;

        Ok(zzrm)
    }

    pub fn sources_array_to_map<'de, D>(
        deserializer: D,
    ) -> Result<BTreeMap<String, UserFile>, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Deserialize the array of `SourceEntry`
        let entries: Vec<UserFile> = Deserialize::deserialize(deserializer)?;
        let filenames = entries.clone();

        Ok(filenames
            .iter()
            .filter_map(|v| v.filename.to_owned())
            .zip(entries.into_iter().map(|v| v))
            .collect())
    }

    pub fn latex_commands(
        &self,
        parent_dir: impl AsRef<Path>,
    ) -> Result<Vec<(SourceFile, Command)>, ZZRMException> {
        let compiler = self
            .process
            .compiler
            .as_ref()
            .ok_or(ZZRMException::Parse("Missing compiler spec".into()))?
            .tex_compiler()
            .ok_or(ZZRMException::Parse("Missing compiler".into()))?;

        let tl_files = self
            .sources
            .iter()
            .filter(|(_, properties)| matches!(properties.usage, Some(FileUsage::TopLevel)))
            .filter_map(|(path, _)| {
                // TODO: make this more secure?
                let mut cmd = Command::new(&compiler);

                cmd.arg("-interaction=nonstop");
                cmd.arg("-recorder");
                cmd.arg("-f");
                cmd.arg(path);
                cmd.current_dir(&parent_dir);

                Some((
                    SourceFile::from_path(parent_dir.as_ref().join(path), &parent_dir)
                        .ok_with_warning()?,
                    cmd,
                ))
            })
            .collect();

        Ok(tl_files)
    }

    pub fn top_level_files(&self, parent_dir: impl AsRef<Path>) -> HashSet<SourceFile> {
        self.sources
            .iter()
            .filter(|(_, properties)| matches!(properties.usage, Some(FileUsage::TopLevel)))
            .filter_map(|(p, _)| {
                SourceFile::from_path(parent_dir.as_ref().join(p), &parent_dir).ok_with_warning()
            })
            .collect()
    }
}
