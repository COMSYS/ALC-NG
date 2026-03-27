#![allow(dead_code)]

use std::{collections::HashSet, path::PathBuf};

use serde::{Deserialize, Serialize};

struct TopLevelFile {
    pub filename: String,
    pub process: MainProcessSpec,
    pub hyperref_found: Option<bool>,
    pub issues: Vec<TexFileIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueType {
    FileNotFound,
    ConflictingFileType,
    ConflictingOutputType,
    ConflictingEngineType,
    ConflictingPostprocessType,
    UnsupportedCompilerType,
    UnsupportedCompilerTypeUnicode,
    UnsupportedCompilerTypeImageMix,
    UnsupportedCompilerTypeLatex209,
    ConflictingImageTypes,
    IncludeCommandWithMacro,
    ContentsDecodeError,
    IssueInSubfile,
    IndexDefinitionMissing,
    BblVersionMismatch,
    BblFileMissing,
    MultipleBibliographyTypes,
    BblUsageMismatch,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TexFileIssue {
    pub key: IssueType,
    pub info: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
}

impl TexFileIssue {
    /// Create a new issue.
    ///
    /// Parameters are intentionally positional (exactly the same order as the
    /// Python `__init__`) to keep the API familiar to users of the original
    /// library.
    pub fn new(key: IssueType, info: impl Into<String>, filename: Option<String>) -> Self {
        Self {
            key,
            info: info.into(),
            filename,
        }
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct MainProcessSpec {
    pub compiler: Option<CompilerSpec>,
    pub bibiliography: Option<BibProcessSpec>,
    pub index: Option<IndexProcessSpec>,
    pub fontmaps: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BibProcessSpec {
    pub processor: BibCompiler,
    pub pre_generator: bool,
}

impl BibProcessSpec {
    pub fn new(processor: BibCompiler, pre_generator: bool) -> Self {
        Self {
            processor,
            pre_generator,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IndexProcessSpec {
    processor: IndexCompiler,
    pre_generated: bool,
}

impl IndexProcessSpec {
    pub fn new(processor: IndexCompiler, pre_generated: bool) -> Self {
        Self {
            processor,
            pre_generated,
        }
    }
}

#[derive(Debug)]
pub struct CompilerSpec {
    pub engine: EngineType,
    pub lang: LanguageType,
    pub output: OutputType,
    pub postp: PostProcessType,
}

impl<'de> Deserialize<'de> for CompilerSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let compiler_str = String::deserialize(deserializer)?;
        let spec = CompilerSpec::from_compiler_string(&compiler_str);
        Ok(spec)
    }
}

impl CompilerSpec {
    const PDF_SUBMISSION_STRING: &'static str = "pdf_submission";
    const HTML_SUBMISSION_STRING: &'static str = "html_submission";
    const TEX_BBL_VERSIONS: [&'static str; 2] = ["3.2", "3.3"];

    // Compiler selection mapping
    const COMPILER_SELECTION: &'static [(LanguageType, OutputType, EngineType, &'static str)] = &[
        // LanguageType.tex, OutputType.dvi
        (LanguageType::Tex, OutputType::Dvi, EngineType::Tex, "etex"),
        (
            LanguageType::Tex,
            OutputType::Dvi,
            EngineType::Luatex,
            "dviluatex",
        ),
        (LanguageType::Tex, OutputType::Dvi, EngineType::Ptex, "ptex"),
        (
            LanguageType::Tex,
            OutputType::Dvi,
            EngineType::Uptex,
            "uptex",
        ),
        // LanguageType.tex, OutputType.pdf
        (
            LanguageType::Tex,
            OutputType::Pdf,
            EngineType::Tex,
            "pdfetex",
        ),
        (
            LanguageType::Tex,
            OutputType::Pdf,
            EngineType::Luatex,
            "luatex",
        ),
        (
            LanguageType::Tex,
            OutputType::Pdf,
            EngineType::Xetex,
            "xetex",
        ),
        // LanguageType.latex, OutputType.dvi
        (
            LanguageType::Latex,
            OutputType::Dvi,
            EngineType::Tex,
            "latex",
        ),
        (
            LanguageType::Latex,
            OutputType::Dvi,
            EngineType::Luatex,
            "dvilualatex",
        ),
        (
            LanguageType::Latex,
            OutputType::Dvi,
            EngineType::Ptex,
            "platex",
        ),
        (
            LanguageType::Latex,
            OutputType::Dvi,
            EngineType::Uptex,
            "uplatex",
        ),
        // LanguageType.latex, OutputType.pdf
        (
            LanguageType::Latex,
            OutputType::Pdf,
            EngineType::Tex,
            "pdflatex",
        ),
        (
            LanguageType::Latex,
            OutputType::Pdf,
            EngineType::Luatex,
            "lualatex",
        ),
        (
            LanguageType::Latex,
            OutputType::Pdf,
            EngineType::Xetex,
            "xelatex",
        ),
    ];

    pub fn new() -> Self {
        CompilerSpec {
            engine: EngineType::Unknown,
            lang: LanguageType::Unknown,
            output: OutputType::Unknown,
            postp: PostProcessType::None,
        }
    }

    pub fn from_compiler_string(compiler: &str) -> Self {
        let mut spec = CompilerSpec::new();

        match compiler {
            Self::PDF_SUBMISSION_STRING => {
                spec.lang = LanguageType::Pdf;
                spec.engine = EngineType::Unknown;
                spec.output = OutputType::Unknown;
                spec.postp = PostProcessType::None;
                return spec;
            }
            Self::HTML_SUBMISSION_STRING => {
                spec.lang = LanguageType::Html;
                spec.engine = EngineType::Unknown;
                spec.output = OutputType::Unknown;
                spec.postp = PostProcessType::None;
                return spec;
            }
            "tex" => {
                spec.lang = LanguageType::Tex;
                spec.engine = EngineType::Tex;
                spec.output = OutputType::Dvi;
                spec.postp = PostProcessType::DvipsPs2pdf;
                return spec;
            }
            "latex" => {
                spec.lang = LanguageType::Latex;
                spec.engine = EngineType::Tex;
                spec.output = OutputType::Dvi;
                spec.postp = PostProcessType::DvipsPs2pdf;
                return spec;
            }
            "pdftex" => {
                spec.lang = LanguageType::Tex;
                spec.engine = EngineType::Tex;
                spec.output = OutputType::Pdf;
                spec.postp = PostProcessType::None;
                return spec;
            }
            _ => {
                // Handle compiler strings with post-processing
                let mut parts = compiler.split("+");
                let raw_compiler = parts.next();
                let raw_post_processing = parts.next();

                let compiler = if let Some(compiler) = raw_compiler {
                    compiler
                } else {
                    panic!("Invalid compiler name")
                };

                spec.postp = if let Some(post_processing) = raw_post_processing
                    && let Ok(post_processing) =
                        serde_json::from_str::<PostProcessType>(post_processing)
                {
                    post_processing
                } else {
                    PostProcessType::Unknown
                };

                // Look up the compiler in the selection map
                for (lang, output, engine, compiler_name) in Self::COMPILER_SELECTION {
                    if compiler == *compiler_name {
                        spec.lang = lang.clone();
                        spec.output = output.clone();
                        spec.engine = engine.clone();
                        break;
                    }
                }
            }
        }

        spec
    }

    pub fn is_determined(&self) -> bool {
        self.engine != EngineType::Unknown
            && self.lang != LanguageType::Unknown
            && self.output != OutputType::Unknown
    }

    pub fn compiler_string(&self) -> Option<String> {
        // Handle PDF and HTML special cases
        if self.lang == LanguageType::Pdf {
            return Some(Self::PDF_SUBMISSION_STRING.to_string());
        }
        if self.lang == LanguageType::Html {
            return Some(Self::HTML_SUBMISSION_STRING.to_string());
        }

        // Look up in compiler selection
        for (lang, output, engine, compiler_name) in Self::COMPILER_SELECTION {
            if *lang == self.lang && *output == self.output && *engine == self.engine {
                let mut result = compiler_name.to_string();

                if self.postp != PostProcessType::None && self.postp != PostProcessType::Unknown {
                    // TODO: Check whether output == DVI for post-processing
                    result.push('+');
                    // TODO: check fi this works as expected
                    if let Ok(v) = serde_json::to_value(&self.postp)
                        && let Some(s) = v.as_str()
                    {
                        result.push_str(s);
                    }
                }

                return Some(result);
            }
        }

        None
    }

    pub fn tex_compiler(&self) -> Option<String> {
        // Handle PDF and HTML special cases
        if self.lang == LanguageType::Pdf {
            return Some(Self::PDF_SUBMISSION_STRING.to_string());
        }
        if self.lang == LanguageType::Html {
            return Some(Self::HTML_SUBMISSION_STRING.to_string());
        }

        // Look up in compiler selection
        for (lang, output, engine, compiler_name) in Self::COMPILER_SELECTION {
            if *lang == self.lang && *output == self.output && *engine == self.engine {
                return Some(compiler_name.to_string());
            }
        }

        None
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum FileType {
    Tex,
    Bib,
    Idx,
    Bbl,
    Ind,
    Other,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(rename_all = "lowercase")]
pub enum EngineType {
    Unknown,
    Tex,
    Luatex,
    Xetex,
    Ptex,
    Uptex,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(rename_all = "lowercase")]
pub enum LanguageType {
    Unknown,
    Tex,
    Latex,
    Latex209,
    Pdf,
    Html,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(rename_all = "lowercase")]
pub enum OutputType {
    Unknown,
    Dvi,
    Pdf,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub enum PostProcessType {
    Unknown,
    None,
    #[serde(rename = "dvips_ps2pdf")]
    DvipsPs2pdf,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum IndexCompiler {
    Unknown,
    MakeIndex,
    Mendex,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum BibCompiler {
    Unknown,
    BibTex,
    BibTex8,
    BibTexU,
    UpBibTex,
    Biber,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum BblType {
    Unknown,
    Plain,
    Biblatex,
}

struct ParsedTexFile {
    path: PathBuf,
    graphics_path: Vec<Vec<PathBuf>>,
    uses_bibliography: bool,
    uses_bbl_file_type: HashSet<BblType>,
    language: LanguageType,
    contains_documentclass: bool,
    contains_bye: bool,
    contains_pdf_output_true: bool,
    contains_pdf_output_false: bool,
    hyperref_found: bool,
    used_tex_files: HashSet<PathBuf>,
    used_bib_files: HashSet<PathBuf>,
    used_idx_files: HashSet<PathBuf>,
    used_bbl_files: HashSet<PathBuf>,
    used_ind_files: HashSet<PathBuf>,
    used_other_files: HashSet<PathBuf>,
    used_system_files: HashSet<PathBuf>,
    issues: HashSet<TexFileIssue>,
    children: Vec<Self>,
    parent: Vec<Self>,
}

impl ParsedTexFile {
    fn detect_language(&mut self) {
        self.language = LanguageType::Unknown;
    }
}
