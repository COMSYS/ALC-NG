use std::{
    collections::HashMap,
    ffi::OsStr,
    fmt::Debug,
    fs::read,
    hash::Hash,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{
    cleaner::{
        config::CleanerConfig,
        file_renames::FileRenames,
        submission::{
            comment_commands::{AnalyzedFile, CommentCommand, CommentCommands, EmptyDefinition},
            deletion_stats::DeletionStats,
            directives::{FileDirectives, directive_keywords, parse_directives},
        },
    },
    helper::{SourceFile, exception, is_empty, is_newline},
    parsing::{is_main_tex, parse},
};
use anyhow::{Context, Result, anyhow};
use image::ImageDecoder;
use log::{debug, info, trace, warn};
use tree_sitter::{Node, Parser, Tree};

/// Extensions of LaTeX class and package files, which are only cleaned with `clean_classes`.
const CLASS_EXTENSIONS: [&str; 8] = ["sty", "cls", "dtx", "bbx", "cbx", "ins", "def", "clo"];

/// All common file‑extensions for image formats supported by the image crate.
pub const IMAGE_EXTENSIONS: &[&str] = &[
    "avif", "bmp", "dib", "dds", "exr", "ff", "gif", "hdr", "ico", "jpg", "jpeg", "jpe", "png",
    "pnm", "pbm", "pgm", "ppm", "qoi", "tga", "targa", "tif", "tiff", "webp",
];

/// Conditionals defined by the iftex package
/// List derived from https://github.com/latex3/iftex/blob/main/iftex.tex
pub const IF_TEX_CONDITIONALS: [&[u8]; 16] = [
    b"etex",
    b"pdftex",
    b"xetex",
    b"luatex",
    b"luahbtex",
    b"luametatex",
    b"ptex",
    b"uptex",
    b"ptexng",
    b"vtex",
    b"alephtex",
    b"tutex",
    b"texpadtex",
    b"hint",
    b"prote",
    b"pdf",
];

/// Conditionals of TeX, e-TeX and pdfTeX (without the `\if` prefix). Their condition is
/// evaluated by the engine, thus both branches are kept.
/// `\iftrue`, `\iffalse` and `\if0` are evaluated by the cleaner and missing.
pub const PRIMITIVE_CONDITIONALS: [&[u8]; 22] = [
    b"cat",
    b"num",
    b"dim",
    b"odd",
    b"vmode",
    b"hmode",
    b"mmode",
    b"inner",
    b"void",
    b"hbox",
    b"vbox",
    b"x",
    b"eof",
    b"case",
    b"defined",
    b"csname",
    b"fontchar",
    b"incsname",
    b"pdfprimitive",
    b"pdfabsnum",
    b"pdfabsdim",
    b"pdfstrcmp",
];

/// Grammar names that are preserved as‑is during stripping.
/// These tokens are simply copied to the output without modification.
pub const GRAMMAR_PRESERVE: [&str; 33] = [
    "$",
    "$$",
    "(",
    ")",
    ",",
    "=",
    "[",
    "]",
    "active_character",
    "argc",
    "command_name",
    "delimiter",
    "hyperlink",
    "label",
    "minted_environment",
    "number",
    "old_command_definition",
    "operator",
    "parameter_delimiter",
    "placeholder",
    "source_code",
    "subscript",
    "superscript",
    "tikz_library_import",
    "todo_command_name",
    "value",
    "value_literal",
    "verbatim",
    "verbatim_environment",
    "word",
    "{",
    "|",
    "}",
];

/// Grammar names that may contain nested sub‑nodes that need to be processed recursively.
pub const GRAMMAR_NESTING: [&str; 85] = [
    "acronym_definition",
    "acronym_reference",
    "asy_enviroment",
    "asydef_environment",
    "author_declaration",
    "begin",
    "biblatex_include",
    "bibstyle_include",
    "bibtex_include",
    "brack_group",
    "brack_group_argc",
    "brack_group_key_value",
    "brack_group_text",
    "caption",
    "changes_replaced",
    "chapter",
    "citation",
    "color_definition",
    "color_reference",
    "color_set_definition",
    "counter_addition",
    "counter_declaration",
    "counter_definition",
    "counter_increment",
    "counter_typesetting",
    "counter_value",
    "counter_within_declaration",
    "counter_without_declaration",
    "curly_group",
    "curly_group_author_list",
    "curly_group_author_list_repeat1",
    "curly_group_command_name",
    "curly_group_glob_pattern",
    "curly_group_impl",
    "curly_group_label",
    "curly_group_label_list",
    "curly_group_path",
    "curly_group_path_list",
    "curly_group_text",
    "curly_group_text_list",
    "curly_group_value",
    "curly_group_word",
    "displayed_equation",
    "else_content",
    "end",
    "enum_item",
    "environment_definition",
    "expand_after",
    "file_include",
    "glossary_entry_definition",
    "glossary_entry_reference",
    "graphics_include",
    "graphics_path_declaration",
    "if_content",
    "import_include",
    "inkscape_include",
    "inline_formula",
    "key_value_pair",
    "label_definition",
    "label_number",
    "label_reference",
    "label_reference_range",
    "latex_include",
    "let_command_definition",
    "listing_environment",
    "luacode_environment",
    "math_delimiter",
    "math_environment",
    "paired_delimiter_definition",
    "paragraph",
    "part",
    "pycode_environment",
    "sageblock_environment",
    "sagesilent_environment",
    "section",
    "subparagraph",
    "subsection",
    "subsubsection",
    "svg_include",
    "text",
    "text_mode",
    "theorem_definition",
    "title_declaration",
    "todo",
    "verbatim_include",
];

/// Represents a parsed file. The enum distinguishes between LaTeX‑like files,
/// bibliography files, and all other binary files.
#[derive(Debug)]
pub enum ParsedFile {
    /// LaTeX, LaTeX style or class file.
    LatexLike(LatexLikeFile),
    /// BibTeX bibliography file.
    Bib(BibFile),
    /// Image file
    Image(ImageFile),
    /// Any other file (e.g. PDFs, binaries).
    Other(OtherFile),
}

#[derive(Debug)]
pub struct ImageFile {
    /// Shared configuration used for cleaning.
    cleaner_config: Arc<CleanerConfig>,
    /// Metadata and path information of the source file.
    pub source_file: SourceFile,
    /// Store stats about deleted bytes
    pub stats: DeletionStats,
}

impl ImageFile {
    /// Copies the raw (unmodified) image file to the target path.
    fn copy_raw_to(&self, target: impl AsRef<Path>) -> Result<()> {
        use std::fs::copy;

        copy(self.source_file.full(), target.as_ref())?;

        Ok(())
    }

    /// Copies a cleaned (resized) version of the image to the target path.
    fn copy_cleaned_to(&self, target: impl AsRef<Path>) -> Result<()> {
        use image::{DynamicImage, ImageReader, imageops::FilterType};
        use std::fs::File;

        // Read the image and detect its format.
        let original = ImageReader::open(self.source_file.full())?.with_guessed_format()?;
        let format = original.format().context(format!(
            "Could not infer image format of file {}",
            self.source_file.full().display(),
        ))?;

        // Decode the image and apply EXIF orientation if present.
        let mut decoder = original.into_decoder()?;
        let orientation = decoder.orientation()?;
        let mut original = DynamicImage::from_decoder(decoder)?;
        original.apply_orientation(orientation);

        // Resize the image according to the configuration.
        let img = original.resize(
            self.cleaner_config.im_size,
            self.cleaner_config.im_size,
            FilterType::Gaussian,
        );
        let target = target.as_ref();

        // Write the resized image to the target file.
        let mut output_file = File::options()
            .create(true)
            .truncate(true)
            .write(true)
            .open(target)
            .context(anyhow!(
                "Unable to create target file at path {}",
                target.display()
            ))?;
        img.write_to(&mut output_file, format)
            .context(anyhow!("Failed to write file {}", target.display()))?;
        output_file.flush()?;

        Ok(())
    }
}

/// File that contains LaTeX‑like source code (``.tex``, ``.sty`` or ``.cls``).
/// The struct stores the source file metadata, the parsed syntax tree
/// and the original text content.
pub struct LatexLikeFile {
    /// Shared configuration used for cleaning.
    cleaner_config: Arc<CleanerConfig>,
    /// Original input path, needed for accurate logging
    input_path: PathBuf,
    /// Metadata and path information of the source file.
    pub source_file: SourceFile,
    /// The syntax tree produced by tree-sitter.
    pub tree: Tree,
    /// The original textual content of the file.
    pub original_content: Vec<u8>,
    /// Store stats about deleted bytes
    pub stats: DeletionStats,
}

impl Debug for LatexLikeFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LatexLikeFile")
            .field("source_file", &self.source_file)
            .field("stats", &self.stats)
            .finish()
    }
}

/// File that contains a BibTeX bibliography.
/// It keeps the source file metadata and the raw text content.
pub struct BibFile {
    /// Shared configuration used for cleaning.
    cleaner_config: Arc<CleanerConfig>,
    /// Metadata and path information of the source file.
    pub source_file: SourceFile,
    /// Store stats about deleted bytes
    pub stats: DeletionStats,
}

impl Debug for BibFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BibFile")
            .field("source_file", &self.source_file)
            .field("stats", &self.stats)
            .finish()
    }
}

impl BibFile {
    /// Copies the raw (unmodified) bibliography file to the target path.
    pub fn copy_raw_to(&self, target: impl AsRef<Path>) -> Result<()> {
        use std::fs::copy;

        copy(self.source_file.full(), target.as_ref())?;

        Ok(())
    }

    /// Do not copy file, as cleaning = removing
    pub fn copy_cleaned_to(
        &self,
        _: impl AsRef<Path>,
        deletion_stats: &mut DeletionStats,
    ) -> Result<()> {
        // Track the deletion of the bib file
        deletion_stats
            .bib_deleted
            .push(self.source_file.relative().to_string_lossy().to_string());
        Ok(())
    }
}

/// Files that are not LaTeX‑like or BibTeX. The content is stored as raw bytes.
pub struct OtherFile {
    /// Shared configuration used for cleaning.
    _cleaner_config: Arc<CleanerConfig>,
    /// Metadata and path information of the source file.
    pub source_file: SourceFile,
    /// Store stats about deleted bytes
    pub stats: DeletionStats,
}

impl Debug for OtherFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OtherFile")
            .field("source_file", &self.source_file)
            .finish()
    }
}

impl OtherFile {
    /// Copies the raw (unmodified) file to the target path.
    pub fn copy_raw_to(&self, target: impl AsRef<Path>) -> Result<()> {
        use std::fs::copy;

        copy(self.source_file.full(), target.as_ref())?;

        Ok(())
    }

    /// Copies the file to the target path. Uncleaned class and package files
    /// only get their references to renamed files rewritten.
    pub fn copy_to(&self, target: impl AsRef<Path>, file_renames: &FileRenames) -> Result<()> {
        let is_class_file = self
            .source_file
            .extension()
            .is_some_and(|ext| CLASS_EXTENSIONS.contains(&ext.to_string_lossy().as_ref()));

        if is_class_file {
            file_renames.copy_with_rewritten_references(
                self.source_file.full(),
                target.as_ref(),
                &self.source_file.relative().to_string_lossy(),
            )
        } else {
            self.copy_raw_to(target)
        }
    }
}

/// Methods for handling LaTeX‑like files.
impl LatexLikeFile {
    /// Copies a cleaned version of the LaTeX file to the target path.
    /// This method strips comments, unused commands and processes custom
    /// conditional blocks before writing the cleaned content.
    pub fn copy_cleaned_to(
        &self,
        target: impl AsRef<Path>,
        deletion_stats: &mut DeletionStats,
        file_renames: &FileRenames,
        comment_commands: &Arc<CommentCommands>,
    ) -> Result<()> {
        use std::fs::File;

        let filename = self.cleaning_name();

        let (stripped, stats) = ContentStripper::clean_with_comment_commands(
            &self.original_content,
            self.tree.root_node(),
            &filename,
            self.cleaner_config.clone(),
            Some(file_renames),
            comment_commands.clone(),
        )?;

        deletion_stats.merge(stats);

        if let Some(stripped) = stripped {
            let mut file = BufWriter::new(File::create(target.as_ref())?);

            file.write_all(&stripped)?;
            file.flush()?;
        }

        Ok(())
    }

    /// The name of the file used for cleaning and logging.
    pub fn cleaning_name(&self) -> String {
        self.input_path
            .join(self.source_file.relative())
            .to_string_lossy()
            .into_owned()
    }

    /// The file as input of the comment command analysis.
    pub fn analyzed<'a>(&'a self, filename: &'a str) -> AnalyzedFile<'a> {
        AnalyzedFile {
            content: &self.original_content,
            root: self.tree.root_node(),
            filename,
        }
    }

    /// Copies the raw (unmodified) LaTeX file to the target path.
    /// Only references to renamed files are rewritten.
    pub fn copy_raw_to(&self, target: impl AsRef<Path>, file_renames: &FileRenames) -> Result<()> {
        use std::fs::write;

        let filename = self.source_file.relative().to_string_lossy();
        let content = file_renames
            .rewrite_references(&self.original_content, self.tree.root_node(), &filename)
            .unwrap_or_else(|| self.original_content.clone());

        write(target.as_ref(), content)?;

        Ok(())
    }
}

/// Factory and utility methods for `ParsedFile`.
impl ParsedFile {
    /// Construct a new `ParsedFile` from a filesystem path.
    ///
    /// The function reads the file, parses it if it is a LaTeX‑like file,
    /// and stores the original content. For non‑parsing file types it
    /// simply reads the raw bytes.
    pub fn new(
        cleaner_config: Arc<CleanerConfig>,
        path: impl AsRef<Path>,
        parent: impl AsRef<Path>,
        original_input_path: impl AsRef<Path>,
    ) -> Result<Self> {
        trace!("Creating ParsedFile for: {:?}", path.as_ref());

        let mut parser = Parser::new();
        let path = path.as_ref();

        parser
            .set_language(&tree_sitter_latex::language())
            .expect("Error loading LaTeX grammar");

        let source_file = SourceFile::from_path(path, parent)?;
        trace!("Source file created: {:?}", source_file.relative());

        Ok(match path.extension() {
            Some(ext)
                if ext == "tex"
                    || (cleaner_config.clean_classes
                        && CLASS_EXTENSIONS.contains(&ext.to_string_lossy().as_ref())) =>
            {
                let content = read(source_file.full())
                    .context(anyhow!("Failed to read LaTeX file {}", path.display()))?;

                if content.is_empty() {
                    trace!("File {} is empty", path.display());
                } else {
                    trace!(
                        "Read {} bytes from LaTeX file {}",
                        content.len(),
                        path.display()
                    );
                }

                let tree = match parser.parse(&content, None) {
                    Some(tree) => {
                        trace!("Successfully parsed LaTeX file: {}", path.display());
                        tree
                    }
                    None => {
                        warn!(
                            "Could not parse .tex/.sty/.cls file {}, no tree returned",
                            path.display()
                        );
                        anyhow::bail!(
                            "Could not parse .tex/.sty/.cls file {}, no tree returned",
                            path.display()
                        );
                    }
                };

                trace!("Created LatexLike ParsedFile for {}", path.display());
                Self::LatexLike(LatexLikeFile {
                    cleaner_config,
                    source_file,
                    original_content: content,
                    tree,
                    stats: Default::default(),
                    input_path: original_input_path.as_ref().to_path_buf(),
                })
            }
            Some(ext)
                if IMAGE_EXTENSIONS
                    .contains(&ext.to_str().context("File path is not valid utf8")?) =>
            {
                trace!(
                    "Detected image file (extension: {}): {}",
                    ext.display(),
                    path.display()
                );
                Self::Image(ImageFile {
                    cleaner_config,
                    source_file,
                    stats: Default::default(),
                })
            }
            Some(ext)
                if [
                    OsStr::new("bib"),
                    OsStr::new("bst"),
                    OsStr::new("bbx"),
                    OsStr::new("dbx"),
                    OsStr::new("cbx"),
                ]
                .contains(&ext) =>
            {
                trace!("Detected BibTeX file: {}", path.display());
                Self::Bib(BibFile {
                    cleaner_config,
                    source_file,
                    stats: Default::default(),
                })
            }
            _ => {
                trace!("Treating as other file type: {}", path.display());
                Self::Other(OtherFile {
                    _cleaner_config: cleaner_config,
                    source_file,
                    stats: Default::default(),
                })
            }
        })
    }

    /// Retrieve the underlying `SourceFile` reference.
    pub fn source_file(&self) -> &SourceFile {
        match &self {
            ParsedFile::LatexLike(latex_like_file) => &latex_like_file.source_file,
            ParsedFile::Bib(bib_file) => &bib_file.source_file,
            ParsedFile::Image(img_file) => &img_file.source_file,
            ParsedFile::Other(other_file) => &other_file.source_file,
        }
    }

    /// Determine if this file is the main document.
    ///
    /// For LaTeX‑like files this delegates to `is_main_tex`. BibTeX and other
    /// files are never considered main.
    pub fn is_main(&self) -> bool {
        match &self {
            ParsedFile::LatexLike(latex_like_file) => {
                is_main_tex(&latex_like_file.tree, &latex_like_file.original_content)
            }
            _ => false,
        }
    }

    /// Copies the raw (unmodified) file to the target path.
    /// Only references to renamed files are rewritten in LaTeX sources.
    pub fn copy_raw_to(&self, target: impl AsRef<Path>, file_renames: &FileRenames) -> Result<()> {
        match &self {
            ParsedFile::LatexLike(latex_like_file) => {
                latex_like_file.copy_raw_to(target, file_renames)
            }
            ParsedFile::Bib(source_file) => source_file.copy_raw_to(target),
            ParsedFile::Image(sf) => sf.copy_raw_to(target),
            ParsedFile::Other(other_file) => other_file.copy_to(target, file_renames),
        }
    }

    /// Copies a cleaned version of the file to the target path.
    /// The behavior depends on the file type and configuration.
    pub fn copy_cleaned_to(
        &self,
        target: impl AsRef<Path>,
        deletion_stats: &mut DeletionStats,
        file_renames: &FileRenames,
        comment_commands: &Arc<CommentCommands>,
    ) -> Result<()> {
        match self {
            ParsedFile::LatexLike(latex_like_file) => latex_like_file.copy_cleaned_to(
                target,
                deletion_stats,
                file_renames,
                comment_commands,
            ),
            ParsedFile::Bib(bib) => {
                if bib.cleaner_config.keep_bib {
                    bib.copy_raw_to(target)
                } else {
                    bib.copy_cleaned_to(target, deletion_stats)
                }
            }
            ParsedFile::Image(sf) => {
                if sf.cleaner_config.resize_images {
                    sf.copy_cleaned_to(target)
                } else {
                    sf.copy_raw_to(target)
                }
            }
            ParsedFile::Other(other_file) => other_file.copy_to(target, file_renames),
        }
    }

    pub fn deletion_stats(&self) -> &DeletionStats {
        match self {
            ParsedFile::LatexLike(latex_like_file) => &latex_like_file.stats,
            ParsedFile::Bib(bib_file) => &bib_file.stats,
            ParsedFile::Image(image_file) => &image_file.stats,
            ParsedFile::Other(other_file) => &other_file.stats,
        }
    }
}

impl From<ParsedFile> for SourceFile {
    fn from(parsed_file: ParsedFile) -> Self {
        match parsed_file {
            ParsedFile::LatexLike(latex_like_file) => latex_like_file.source_file,
            ParsedFile::Bib(bib_file) => bib_file.source_file,
            ParsedFile::Image(img_file) => img_file.source_file,
            ParsedFile::Other(other_file) => other_file.source_file,
        }
    }
}

impl PartialEq for ParsedFile {
    fn eq(&self, other: &Self) -> bool {
        self.source_file() == other.source_file()
    }
}

impl Eq for ParsedFile {}

impl Hash for ParsedFile {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.source_file().hash(state);
    }
}

/// Represents the handling result of a node during content stripping.
/// Each variant carries the text that should be added to the output
/// (or `None` if the node is discarded).
#[derive(Clone)]
enum NodeHandling {
    /// There is no previous node, because we are at the beginning of the document.
    NoNode,
    /// The node was kept and not changed.
    Kept(Vec<u8>),
    /// The node was a comment that had some kind of latex code in front of it and was deleted.
    SuffixCommentDeleted(Vec<u8>),
    /// The node was a comment that spanned the completed line and was deleted.
    FullLineCommentDeleted(Vec<u8>),
    /// The node was an if block that was evaluated to either the if or else case.
    IfEvaluated(Vec<u8>),
    /// The node was an if block that was evaluated but the appropriate case was empty.
    /// The bool denotes if it was originally fullline.
    IfEvaluatedEmpty(Vec<u8>, bool),
    /// The node was after \end{document} and was deleted.
    OutOfBounds,
    /// The node was an invocation of a comment command in math mode that was deleted.
    CommandInvocationDeleted(Vec<u8>, bool),
    /// The node was an invocation of a comment command that was replaced with `{}`.
    CommandInvocationReplaced(Vec<u8>),
    CommentPackageIncludeDeleted(Vec<u8>, bool),
    BlockCommentDeleted(Vec<u8>, bool),
}

impl NodeHandling {
    /// Extracts the text from the handling result, if any.
    pub fn into_inner(self) -> Option<Vec<u8>> {
        match self {
            Kept(text) => Some(text),
            SuffixCommentDeleted(text) => Some(text),
            FullLineCommentDeleted(text) => Some(text),
            IfEvaluated(ws) => Some(ws),
            IfEvaluatedEmpty(ws, _) => Some(ws),
            CommandInvocationDeleted(ws, _) => Some(ws),
            CommentPackageIncludeDeleted(ws, _) => Some(ws),
            BlockCommentDeleted(ws, _) => Some(ws),
            CommandInvocationReplaced(ws) => Some(ws),
            _ => None,
        }
    }

    /// Returns `true` if the node was deleted and spanned complete lines.
    fn is_fullline_deletion(&self) -> bool {
        matches!(
            self,
            BlockCommentDeleted(_, true)
                | IfEvaluatedEmpty(_, true)
                | CommandInvocationDeleted(_, true)
                | CommentPackageIncludeDeleted(_, true)
        )
    }

    /// Returns a reference to the internal text.
    pub fn text(&self) -> &[u8] {
        match self {
            Kept(text) => text,
            SuffixCommentDeleted(ws) => ws,
            FullLineCommentDeleted(ws) => ws,
            IfEvaluated(ws) => ws,
            IfEvaluatedEmpty(ws, _) => ws,
            CommandInvocationDeleted(ws, _) => ws,
            CommentPackageIncludeDeleted(ws, _) => ws,
            BlockCommentDeleted(ws, _) => ws,
            CommandInvocationReplaced(ws) => ws,
            _ => &[],
        }
    }

    /// Appends the current node's text to an existing output string,
    /// taking into account the previous node's handling state.
    ///
    /// The logic here ensures that line endings and comment delimiters
    /// are preserved correctly when nodes are removed or kept.
    pub fn append_to(&self, existing: &mut Vec<u8>, prev: &NodeHandling) {
        // A node that spanned complete lines is removed together with its indentation
        let own_text = if self.is_fullline_deletion() {
            strip_trailing_indentation(self.text())
        } else {
            self.text()
        };

        match &prev {
            OutOfBounds => (),
            BlockCommentDeleted(_, false) => {
                // Drop the newline if it exists as it is consumed by the comment env
                // This code is complex as Rust does not offer another way to drop a prefix in-place
                let new_existing = strip_leading_newline(existing);
                let mut new_existing = new_existing.to_owned();

                new_existing.extend_from_slice(own_text);
                existing.clear();
                existing.append(&mut new_existing);
            }
            NoNode
            | Kept(_)
            | SuffixCommentDeleted(_)
            | IfEvaluated(_)
            | IfEvaluatedEmpty(_, false)
            | CommandInvocationDeleted(_, false)
            | CommandInvocationReplaced(_)
            | CommentPackageIncludeDeleted(_, false) => existing.extend_from_slice(own_text),
            FullLineCommentDeleted(_)
            | BlockCommentDeleted(_, true)
            | IfEvaluatedEmpty(_, true)
            | CommandInvocationDeleted(_, true)
            | CommentPackageIncludeDeleted(_, true) => {
                if let Self::FullLineCommentDeleted(ws) = self {
                    // If we have more than one newline, we a are creating new vertical space. Only the first newline terminates the previous comment.
                    if ws.iter().filter(|v| **v == b'\n').count() > 1 {
                        existing.extend_from_slice(&ws[1..]);
                    }

                    return;
                }

                // Drop the remainder of the deleted line, i.e. trailing spaces and its newline
                existing.extend_from_slice(strip_rest_of_blank_line(own_text));
            }
        }
    }
}

/// The main content stripper that traverses a tree‑sitter parse tree
/// and produces a cleaned output string.
/// It keeps track of custom commands, if‑blocks and other stateful information.
pub struct ContentStripper<'a> {
    cleaner_config: Arc<CleanerConfig>,
    filename: &'a str,
    content: &'a [u8],
    if_values: HashMap<Vec<u8>, bool>,
    /// Commands whose invocations are removed, see [`CommentCommands`].
    comment_commands: Arc<CommentCommands>,
    /// Only collect the definitions of comment commands instead of cleaning.
    /// Warnings are suppressed as the file is cleaned (and reported) afterwards.
    collecting: bool,
    /// Eligible definitions with an empty body, found while collecting.
    empty_definitions: Vec<EmptyDefinition>,
    last_byte_of_document_block: Option<usize>,
    deletion_stats: DeletionStats,
    directives: FileDirectives,
    /// Renamed project files whose references should be rewritten.
    file_renames: Option<&'a FileRenames>,
}

use NodeHandling::*;

impl<'a> ContentStripper<'a> {
    /// Public entry point for stripping content.
    ///
    /// Returns an optional cleaned string (if a document is found)
    /// and a vector of grammar errors encountered during traversal.
    pub fn clean(
        content: &'a [u8],
        root: Node<'a>,
        filename: &'a str,
        cleaner_config: Arc<CleanerConfig>,
    ) -> Result<(Option<Vec<u8>>, DeletionStats)> {
        Self::clean_with_renames(content, root, filename, cleaner_config, None)
    }

    /// Like [`ContentStripper::clean`], but additionally rewrites references
    /// (e.g. `\input`, `\includegraphics`) to files renamed in `file_renames`.
    ///
    /// Comment commands are determined from this file only.
    pub fn clean_with_renames(
        content: &'a [u8],
        root: Node<'a>,
        filename: &'a str,
        cleaner_config: Arc<CleanerConfig>,
        file_renames: Option<&'a FileRenames>,
    ) -> Result<(Option<Vec<u8>>, DeletionStats)> {
        let comment_commands = CommentCommands::analyze(
            &[AnalyzedFile {
                content,
                root,
                filename,
            }],
            cleaner_config.clone(),
        );

        Self::clean_with_comment_commands(
            content,
            root,
            filename,
            cleaner_config,
            file_renames,
            comment_commands,
        )
    }

    /// Like [`ContentStripper::clean_with_renames`], but with the comment commands
    /// determined for the whole submission.
    pub fn clean_with_comment_commands(
        content: &'a [u8],
        root: Node<'a>,
        filename: &'a str,
        cleaner_config: Arc<CleanerConfig>,
        file_renames: Option<&'a FileRenames>,
        comment_commands: Arc<CommentCommands>,
    ) -> Result<(Option<Vec<u8>>, DeletionStats)> {
        trace!("Starting content stripping for file: {}", filename);
        trace!("Content size: {} bytes", content.len());

        let mut stripper = Self::new(content, filename, cleaner_config);
        stripper.file_renames = file_renames;
        stripper.comment_commands = comment_commands;
        stripper.directives = parse_directives(root, content);

        if !stripper.directives.is_empty() {
            info!(
                "Honoring ALC-NG directives in {}: {}",
                filename,
                stripper.directives.describe()
            );
        }

        // The file opts out of cleaning entirely; keep it byte-for-byte,
        // except for references to renamed files that would break otherwise.
        if stripper.directives.keep_all {
            let content = file_renames
                .and_then(|renames| renames.rewrite_references(content, root, filename))
                .unwrap_or_else(|| content.to_vec());
            return Ok((Some(content), stripper.deletion_stats));
        }

        let result = stripper
            .handle_node(root, NoNode)
            .map(|v| (v.into_inner(), stripper.deletion_stats));

        if let Ok((ref cleaned, ref stats)) = result {
            trace!("Content stripping completed for {}", filename);
            trace!("Grammar errors found: {}", stats.grammar_errors.len());
            if let Some(cleaned_content) = cleaned {
                trace!("Cleaned content size: {} bytes", cleaned_content.len());
            }
        }

        result
    }

    /// Strips the file without producing output and returns the definitions with an
    /// empty body that are eligible as comment commands.
    /// Bodies are cleaned with the given (previously found) comment commands.
    pub(crate) fn empty_definitions(
        content: &'a [u8],
        root: Node<'a>,
        filename: &'a str,
        cleaner_config: Arc<CleanerConfig>,
        comment_commands: Arc<CommentCommands>,
    ) -> Result<Vec<EmptyDefinition>> {
        let mut stripper = Self::new(content, filename, cleaner_config);
        stripper.comment_commands = comment_commands;
        stripper.collecting = true;
        stripper.directives = parse_directives(root, content);

        if stripper.directives.keep_all {
            return Ok(Vec::new());
        }

        stripper.handle_node(root, NoNode)?;

        Ok(stripper.empty_definitions)
    }

    fn new(content: &'a [u8], filename: &'a str, cleaner_config: Arc<CleanerConfig>) -> Self {
        Self {
            filename,
            content,
            cleaner_config,
            if_values: Default::default(),
            comment_commands: Default::default(),
            collecting: false,
            empty_definitions: Vec::new(),
            last_byte_of_document_block: None,
            deletion_stats: Default::default(),
            directives: FileDirectives::default(),
            file_renames: None,
        }
    }

    /// Recursively processes a node and its children.
    /// The return value indicates how the node should be handled
    /// (kept, removed, etc.) and is used by the parent to build the output.
    fn handle_node(&mut self, node: Node<'a>, previous_node: NodeHandling) -> Result<NodeHandling> {
        let node_content = &self.content[node.byte_range()];
        let grammar_name = node.grammar_name();

        // Remove text that is present after a \end{document} block
        if let Some(last_byte_of_document_block) = self.last_byte_of_document_block
            && last_byte_of_document_block < node.start_byte()
        {
            self.deletion_stats
                .out_of_bounds
                .push(node_content.to_owned());
            return Ok(OutOfBounds);
        }

        let mut whitespaces = self.whitespace_to_previous_node(&node);

        match grammar_name {
            // An error node was hit, this means that there was a parsing error. This can be attributed to a malformed latex document or an issue with the tree-sitter grammar.
            // The content of the node is simply preserved.
            "ERROR" => {
                let g_err = format!(
                    "Found ERROR node when traversing tree-sitter tree of file '{}:{}:{}' with content: {:?}",
                    self.filename,
                    node.start_position().row + 1,
                    node.start_position().column,
                    String::from_utf8_lossy(node_content)
                );

                self.deletion_stats.grammar_errors.push(g_err);

                // Whole document has a large parsing error
                // If first node is ERROR, treat it as source file
                // We will usually find another (more specific) ERROR node down in the tree
                // This allows cleaning even if a nested error exists
                if node.start_byte() == 0 {
                    return self.handle_source_file(node, previous_node, whitespaces);
                }

                whitespaces.extend_from_slice(node_content);

                Ok(Kept(whitespaces))
            }
            // Keep class include statements as is.
            "class_include" => {
                whitespaces.extend_from_slice(node_content);
                Ok(Kept(whitespaces))
            }
            "line_comment" | "comment" => {
                // ALC-NG directive lines are always kept, regardless of other settings.
                if node.grammar_name() == "line_comment"
                    && directive_keywords(node_content).is_some()
                {
                    let mut kept = whitespaces;
                    kept.extend_from_slice(node_content);
                    return Ok(Kept(kept));
                }

                // If the file opts out of comment cleaning, keep all comments.
                if self.directives.keep_comments {
                    let mut kept = whitespaces;
                    kept.extend_from_slice(node_content);
                    return Ok(Kept(kept));
                }

                // If we do not have a previous node, we are the first node and thus (if deleted) remove a whole line
                let prev = match node.prev_sibling() {
                    Some(p) => p,
                    None => {
                        self.deletion_stats
                            .line_comment
                            .push(node_content.to_owned());
                        return Ok(FullLineCommentDeleted(whitespaces));
                    }
                };

                // Line comment with some latex code before. We need to keep a % before the newline to keep the newline escaped.
                if prev.end_position().row == node.start_position().row {
                    let mut new_content = whitespaces;
                    new_content.push(b'%');

                    self.deletion_stats
                        .suffix_comment
                        .push(node_content.to_owned());
                    return Ok(SuffixCommentDeleted(new_content));
                }

                // Whitespace in the same line in front of the current node
                let whitespace_in_same_line: Vec<u8> = whitespaces
                    .iter()
                    .rev()
                    .take_while(|c| c.is_ascii_whitespace() && !is_newline(**c))
                    .copied()
                    .collect();

                // The previous node is in the previous line (row). And we are a line comment that with some content (probably whitespace) in front of us
                // We thus need to keep the whitespace and a % at the end.
                if prev.end_position().row < node.start_position().row
                    && !whitespace_in_same_line.is_empty()
                {
                    let mut whitespaces = whitespaces;
                    whitespaces.push(b'%');

                    self.deletion_stats
                        .suffix_comment
                        .push(node_content.to_owned());
                    return Ok(SuffixCommentDeleted(whitespaces));
                }

                self.deletion_stats
                    .line_comment
                    .push(node_content.to_owned());
                Ok(FullLineCommentDeleted(whitespaces))
            }
            "long_command_definition" => match node.child(1) {
                // The wrapped definition is cleaned with `\long` (and anything up to it) as prefix
                Some(c) if c.grammar_name() == "new_command_definition" => {
                    whitespaces.extend_from_slice(&self.content[node.start_byte()..c.start_byte()]);
                    self.handle_new_command_definition(c, previous_node, whitespaces)
                }
                Some(c) if c.grammar_name() == "old_command_definition" => {
                    whitespaces.extend_from_slice(&self.content[node.start_byte()..c.start_byte()]);
                    self.handle_old_command_definition(c, previous_node, whitespaces)
                }
                Some(_) | None => {
                    whitespaces.extend_from_slice(node_content);
                    Ok(Kept(whitespaces))
                }
            },
            "new_command_definition" => {
                self.handle_new_command_definition(node, previous_node, whitespaces)
            }
            "old_command_definition" => {
                self.handle_old_command_definition(node, previous_node, whitespaces)
            }
            "path" | "glob_pattern" => {
                // Point references to renamed files to their new name
                match self
                    .file_renames
                    .and_then(|renames| renames.rewrite_reference(node_content))
                {
                    Some(rewritten) => {
                        trace!(
                            "Rewriting file reference {:?} to {:?}",
                            String::from_utf8_lossy(node_content),
                            String::from_utf8_lossy(&rewritten)
                        );
                        whitespaces.extend_from_slice(&rewritten);
                    }
                    None => whitespaces.extend_from_slice(node_content),
                }
                Ok(Kept(whitespaces))
            }
            "package_include" => {
                // We want to remove the import of the comment package if present
                let paths = match node.child_by_field_name("paths") {
                    Some(c) => c,
                    None => {
                        whitespaces.extend_from_slice(node_content);
                        return Ok(Kept(whitespaces));
                    }
                };
                let path = match paths.child_by_field_name("path") {
                    Some(c) => c,
                    None => {
                        whitespaces.extend_from_slice(node_content);
                        return Ok(Kept(whitespaces));
                    }
                };

                if &self.content[path.byte_range()] == b"comment" && !self.directives.keep_comments
                {
                    // Only spaces or tabs separate the node from the surrounding newlines, thus, we are fullline.
                    let fullline = self.is_fullline(&node);

                    self.deletion_stats
                        .comment_package_include_deleted
                        .push(node_content.to_owned());
                    Ok(CommentPackageIncludeDeleted(whitespaces, fullline))
                } else {
                    whitespaces.extend_from_slice(node_content);
                    Ok(Kept(whitespaces))
                }
            }
            "generic_command" | "todo" => {
                // Remove invocations of comment commands, see `CommentCommands`
                if let Some(last_argument) = self.comment_invocation(node) {
                    return self.handle_comment_invocation(node, last_argument, whitespaces);
                }

                if node.child(0).is_none() {
                    whitespaces.extend_from_slice(node_content);
                    return Ok(Kept(whitespaces));
                }

                // Handle child nodes inside the command body, if present
                let mut new_content = whitespaces;
                let mut i = 0;
                let mut prev_node = previous_node;

                while let Some(child) = node.child(i) {
                    let result = self.handle_node(child, prev_node.clone())?;

                    result.append_to(&mut new_content, &prev_node);

                    prev_node = result;

                    i += 1;
                }

                Ok(Kept(new_content))
            }
            "generic_environment" => {
                let mut new_content = whitespaces;
                let mut i = 0;
                let is_document_environment = node
                    .child_by_field_name("begin")
                    .is_some_and(|begin| self.is_document_environment_name(begin));
                let ends_document_environment = node
                    .child_by_field_name("end")
                    .is_some_and(|end| self.is_document_environment_name(end));
                let mut prev_node = previous_node;

                while let Some(child) = node.child(i) {
                    let result = self.handle_node(child, prev_node.clone())?;

                    result.append_to(&mut new_content, &prev_node);

                    prev_node = result;

                    i += 1;
                }

                // If this is the document environment, then we want to remove all content after this node, thus we store the last byte.
                // Unless the file opts to keep content after \end{document} (keep-tail).
                // The parser can pair \begin{document} with an unrelated \end{...}.
                // Only remove the tail if the parsed end really is \end{document}.
                if is_document_environment
                    && ends_document_environment
                    && !self.directives.keep_tail
                {
                    self.last_byte_of_document_block = Some(node.end_byte());
                }

                Ok(Kept(new_content))
            }
            "source_file" => self.handle_source_file(node, previous_node, whitespaces),
            // All these nodes have other nested nodes that each should be processed (cleaned)
            x if GRAMMAR_NESTING.contains(&x) => {
                let mut new_content = whitespaces;
                let mut i = 0;
                let mut prev_node = previous_node;

                while let Some(child) = node.child(i) {
                    let result = self.handle_node(child, prev_node.clone())?;

                    result.append_to(&mut new_content, &prev_node);

                    prev_node = result;
                    i += 1;
                }

                Ok(Kept(new_content))
            }
            // Handle block comments seperatly
            "block_comment" => self.handle_block_comment(node, whitespaces, previous_node),
            "new_if" => {
                // New if definition with default value of false
                let name = node_content
                    .strip_prefix(b"\\newif\\if")
                    .unwrap()
                    .trim_ascii();
                self.if_values.insert(name.to_owned(), false);

                // We keep the definition even though it will be evaluated for every instance
                // If the if is used as part of an error node, then the document will break

                whitespaces.extend_from_slice(node_content);
                Ok(Kept(whitespaces))
            }
            "new_if_false" | "new_if_assign_false" => {
                // New if definition with value of false
                let name = node_content
                    .strip_prefix(b"\\")
                    .and_then(|s| s.strip_suffix(b"false"))
                    .unwrap_or_default()
                    .trim_ascii();
                self.if_values.insert(name.to_owned(), false);

                // We keep the definition even though it will be evaluated for every instance
                // If the if is used as part of an error node, then the document will break

                whitespaces.extend_from_slice(node_content);
                Ok(Kept(whitespaces))
            }
            "if_content" | "if_then" | "if_else" => unreachable!(),
            "new_if_true" | "new_if_assign_true" => {
                // New if definition with value of true
                let name = node_content
                    .strip_prefix(b"\\")
                    .and_then(|s| s.strip_suffix(b"true"))
                    .unwrap_or_default()
                    .trim_ascii();
                self.if_values.insert(name.to_owned(), true);

                // We keep the definition even though it will be evaluated for every instance
                // If the if is used as part of an error node, then the document will break

                whitespaces.extend_from_slice(node_content);
                Ok(Kept(whitespaces))
            }
            // Handle if if else blocks seperatly
            "general_if" => self.handle_if_block(node, previous_node, node_content, whitespaces),
            "comment_environment" => {
                // Keep the whole `comment` environment if the file opts out of comment cleaning.
                if self.directives.keep_comments {
                    whitespaces.extend_from_slice(node_content);
                    return Ok(Kept(whitespaces));
                }

                // Only spaces or tabs separate the node from the surrounding newlines, thus, we are fullline.
                let fullline = self.is_fullline(&node);

                self.deletion_stats
                    .block_comment_deleted
                    .push(node_content.to_owned());
                Ok(BlockCommentDeleted(whitespaces, fullline))
            }
            r"\iffalse" | r"\fi" | r"\if0" | r"\iftrue" | r"\else" => {
                // Outside of a block comment, the token is a leftover of a parsing error,
                // e.g. the `\else` of an `\ifdefined` whose branch opens an environment.
                if node.parent().is_none_or(|p| p.kind() != "block_comment") {
                    whitespaces.extend_from_slice(node_content);
                    return Ok(Kept(whitespaces));
                }
                // Only spaces or tabs separate the node from the surrounding newlines, thus, we are fullline.
                let fullline = self.is_fullline(&node);

                self.deletion_stats
                    .block_comment_deleted
                    .push(node_content.to_owned());
                Ok(BlockCommentDeleted(whitespaces, fullline))
            }
            // Preserve all these nodes as-is.
            x if GRAMMAR_PRESERVE.contains(&x) || x.starts_with(r"\") => {
                whitespaces.extend_from_slice(node_content);
                Ok(Kept(whitespaces))
            }
            // Keep all other nodes that are not listed in this match statement and log a warning
            other => {
                if !self.collecting {
                    warn!(
                        "Unhandled grammar '{}' with content {:?}\nPlease open an issue at https://github.com/COMSYS/ALC-NG/issues",
                        other,
                        node.utf8_text(self.content).unwrap()
                    );
                }

                whitespaces.extend_from_slice(node_content);
                Ok(Kept(whitespaces))
            }
        }
    }

    /// Returns `true` if the `begin` or `end` node names the document environment.
    /// The name can be formatted in various ways (and include comments!), thus only its words are compared.
    fn is_document_environment_name(&self, node: Node<'a>) -> bool {
        let Some(text) = node
            .child_by_field_name("name")
            .and_then(|name| name.child_by_field_name("text"))
        else {
            return false;
        };

        let mut i = 0;
        let mut complete_group_name = Vec::new();

        while let Some(child) = text.child(i) {
            if child.grammar_name() == "word" {
                complete_group_name.extend_from_slice(&self.content[child.byte_range()]);
            }
            i += 1;
        }

        complete_group_name == b"document"
    }

    /// Reports an error that is ignored if the cleaning is forced, see [`exception`].
    /// While collecting, the error is not logged, as cleaning the file reports it.
    fn exception<T>(&self, err: anyhow::Error, otherwise: T) -> Result<T> {
        match (self.collecting, self.cleaner_config.force) {
            (false, force) => exception(err, otherwise, "tex-cleaner", force),
            (true, true) => Ok(otherwise),
            (true, false) => Err(err),
        }
    }

    /// Returns `true` if the node spans complete lines, i.e. only spaces or tabs separate it
    /// from the newline (or start of file) in front of it and the newline after it.
    fn is_fullline(&self, node: &Node<'a>) -> bool {
        let newline_before = self.content[..node.start_byte()]
            .iter()
            .rev()
            .find(|&&b| !is_horizontal_whitespace(b))
            .is_none_or(|&b| is_newline(b));

        let newline_after = self.content[node.end_byte()..]
            .iter()
            .find(|&&b| !is_horizontal_whitespace(b))
            .is_none_or(|&b| is_newline(b));

        newline_before && newline_after
    }

    /// Extracts the whitespace that appears between the previous sibling node
    /// and the current node. This whitespace is preserved in the output.
    fn whitespace_to_previous_node(&self, node: &Node<'a>) -> Vec<u8> {
        if let Some(prev) = node.prev_sibling() {
            let content_inbetween = &self.content[prev.end_byte()..node.start_byte()];
            // Text that is not part of any node, e.g. the `\ifdefined` of a conditional
            // inside an ERROR node, would be lost otherwise.
            if !content_inbetween.iter().all(|c| c.is_ascii_whitespace()) {
                return content_inbetween.to_vec();
            }
            let mut ws_rev: Vec<u8> = content_inbetween
                .iter()
                .rev()
                .take_while(|c| c.is_ascii_whitespace())
                .copied()
                .collect();
            ws_rev.reverse();

            ws_rev
        } else {
            Vec::new()
        }
    }

    /// Handles a block comment node, which may contain nested conditional
    /// expressions and other sub‑nodes.
    fn handle_block_comment(
        &mut self,
        node: Node<'a>,
        mut whitespaces: Vec<u8>,
        previous_node: NodeHandling,
    ) -> Result<NodeHandling> {
        assert!(node.grammar_name() == "block_comment");

        let first_child = match node.child(0) {
            Some(c) => c,
            None => {
                // Only spaces or tabs separate the node from the surrounding newlines, thus, we are fullline.
                let fullline = self.is_fullline(&node);

                self.deletion_stats
                    .if_evaluated_empty
                    .push(self.content[node.byte_range()].to_owned());
                return Ok(IfEvaluatedEmpty(whitespaces, fullline));
            }
        };

        let grammar_name = first_child.grammar_name();

        // If the file opts to keep conditionals, preserve the whole block unevaluated.
        if self.directives.keep_ifs
            && matches!(
                grammar_name,
                r"\iffalse" | r"\if0" | r"\iftrue" | "general_if"
            )
        {
            let mut kept = whitespaces;
            kept.extend_from_slice(&self.content[node.byte_range()]);
            return Ok(Kept(kept));
        }

        // If the file opts to keep comments, preserve comment-like blocks as-is.
        if self.directives.keep_comments && grammar_name == "comment" {
            let mut kept = whitespaces;
            kept.extend_from_slice(&self.content[node.byte_range()]);
            return Ok(Kept(kept));
        }

        match grammar_name {
            r"\iffalse" | r"\if0" => {
                // First node is content
                // Second node is \else or \fi
                // Third node is content of else block if present (content should be kept)
                // Fourth node is \fi if present
                if let Some(fi_or_else) = node.child(2)
                    && fi_or_else.grammar_name() == r"\else"
                    && let Some(else_content) = node.child(3)
                {
                    if let Some(if_content) = node.child(1) {
                        self.deletion_stats
                            .if_evaluated
                            .push(self.content[if_content.byte_range()].to_owned());
                    }

                    // The preserved content dictates the behavior of the following contents.
                    return self.handle_node(else_content, previous_node);
                }

                // Only spaces or tabs separate the node from the surrounding newlines, thus, we are fullline.
                let fullline = self.is_fullline(&node);

                self.deletion_stats
                    .if_evaluated_empty
                    .push(self.content[node.byte_range()].to_owned());
                Ok(IfEvaluatedEmpty(whitespaces, fullline))
            }
            r"\iftrue" => {
                // First node is content (content should be kept)
                // Second node is \else or \fi
                // Third node is content of else block if present (should be deleted)
                // Fourth node is \fi if present
                if let Some(if_content) = node.child(1) {
                    if let Some(else_content) = node.child(3) {
                        self.deletion_stats
                            .if_evaluated
                            .push(self.content[else_content.byte_range()].to_owned());
                    }

                    // The preserved content dictates the behavior of the following contents.
                    self.handle_node(if_content, previous_node)
                } else {
                    // Only spaces or tabs separate the node from the surrounding newlines, thus, we are fullline.
                    let fullline = self.is_fullline(&node);

                    self.deletion_stats
                        .if_evaluated_empty
                        .push(self.content[node.byte_range()].to_owned());
                    Ok(IfEvaluatedEmpty(whitespaces, fullline))
                }
            }
            "comment" => {
                let second_child = match node.child(1) {
                    Some(c) => c,
                    None => {
                        // Only spaces or tabs separate the node from the surrounding newlines, thus, we are fullline.
                        let fullline = self.is_fullline(&node);

                        self.deletion_stats
                            .if_evaluated_empty
                            .push(self.content[node.byte_range()].to_owned());
                        return Ok(IfEvaluatedEmpty(whitespaces, fullline));
                    }
                };

                if second_child.grammar_name() == "else_content" {
                    let text_in_second_child = match second_child.child(0) {
                        Some(c) => c,
                        None => {
                            // Only spaces or tabs separate the node from the surrounding newlines, thus, we are fullline.
                            let fullline = self.is_fullline(&node);

                            self.deletion_stats
                                .if_evaluated_empty
                                .push(self.content[node.byte_range()].to_owned());
                            return Ok(IfEvaluatedEmpty(whitespaces, fullline));
                        }
                    };

                    if text_in_second_child.grammar_name() == "text" {
                        // TODO: What gets deleted here?
                        // self.deletion_stats.if_evaluated.push();
                        whitespaces
                            .extend_from_slice(&self.content[text_in_second_child.byte_range()]);
                        return Ok(IfEvaluated(whitespaces));
                    }

                    // Only spaces or tabs separate the node from the surrounding newlines, thus, we are fullline.
                    let fullline = self.is_fullline(&node);

                    self.deletion_stats
                        .if_evaluated_empty
                        .push(self.content[node.byte_range()].to_owned());
                    return Ok(IfEvaluatedEmpty(whitespaces, fullline));
                }

                // Only spaces or tabs separate the node from the surrounding newlines, thus, we are fullline.
                let fullline = self.is_fullline(&node);

                self.deletion_stats
                    .if_evaluated_empty
                    .push(self.content[node.byte_range()].to_owned());
                Ok(IfEvaluatedEmpty(whitespaces, fullline))
            }
            "text" => self.handle_node(first_child, previous_node),
            "general_if" => self.handle_if_block(
                first_child,
                previous_node,
                &self.content[first_child.byte_range()],
                whitespaces,
            ),
            _ => {
                // Only spaces or tabs separate the node from the surrounding newlines, thus, we are fullline.
                let fullline = self.is_fullline(&node);

                self.deletion_stats
                    .if_evaluated_empty
                    .push(self.content[node.byte_range()].to_owned());
                Ok(IfEvaluatedEmpty(whitespaces, fullline))
            }
        }
    }

    /// Handles a custom `\if` block, evaluating its condition
    /// based on previously defined `\newif` statements.
    fn handle_if_block(
        &mut self,
        node: Node<'a>,
        mut previous_node: NodeHandling,
        node_content: &'a [u8],
        mut new_content: Vec<u8>,
    ) -> Result<NodeHandling> {
        assert!(node.grammar_name() == "general_if");

        // If the file opts to keep conditionals, preserve the whole if block unevaluated.
        if self.directives.keep_ifs {
            new_content.extend_from_slice(node_content);
            return Ok(Kept(new_content));
        }

        let first_byte_of_next_node = match node.child(0) {
            Some(c) => c.start_byte(),
            None => match node.next_sibling() {
                Some(c) => c.start_byte(),
                None => {
                    new_content.extend_from_slice(node_content);
                    return self.exception(
                        anyhow!("Custom if does not have any child or subsequent nodes."),
                        Kept(new_content),
                    );
                }
            },
        };

        let name_with_prefix = &self.content[node.start_byte()..first_byte_of_next_node];
        let name = name_with_prefix
            .strip_prefix(b"\\if")
            .unwrap_or(name_with_prefix)
            .trim_ascii();

        let is_true = match self.if_values.get(name) {
            Some(b) => b,
            // Conditionals of TeX and its engines, or the iftex package: preserve the if but clean both branches.
            None if PRIMITIVE_CONDITIONALS.contains(&name)
                || IF_TEX_CONDITIONALS.contains(&name) =>
            {
                if self.collecting {
                    // Cleaning the file reports it
                } else if PRIMITIVE_CONDITIONALS.contains(&name) {
                    debug!(
                        "Preserving primitive conditional '\\if{}' at {}:{}:{} but cleaning both branches.",
                        String::from_utf8_lossy(name),
                        self.filename,
                        node.start_position().row + 1,
                        node.start_position().column
                    );
                } else {
                    warn!(
                        "If '{}' not defined at {}:{}:{}. Assuming it is from the package 'iftex'. Preserving if but cleaning both branches.",
                        String::from_utf8_lossy(name),
                        self.filename,
                        node.start_position().row + 1,
                        node.start_position().column
                    );
                }
                new_content.extend_from_slice(name_with_prefix);

                let mut i = 0;

                while let Some(child) = node.child(i) {
                    let result = match child.grammar_name() {
                        r"\else" | r"\fi" => {
                            let mut c = self.whitespace_to_previous_node(&child);
                            c.extend_from_slice(
                                &self.content[child.start_byte()..child.end_byte()],
                            );
                            NodeHandling::Kept(c)
                        }
                        _ => self.handle_node(child, previous_node.clone())?,
                    };

                    result.append_to(&mut new_content, &previous_node);

                    previous_node = result;
                    i += 1;
                }

                return Ok(Kept(new_content));
            }
            None => {
                new_content.extend_from_slice(node_content);
                return self.exception(
                    anyhow!(
                        "Custom if with name '{}' not defined at {}:{}:{}",
                        String::from_utf8_lossy(name),
                        self.filename,
                        node.start_position().row + 1,
                        node.start_position().column
                    ),
                    Kept(new_content),
                );
            }
        };

        let if_content = node.child_by_field_name("then");
        let else_content = node.child_by_field_name("else");

        if let Some(c) = if_content
            && *is_true
        {
            if let Some(else_content) = else_content {
                self.deletion_stats
                    .if_evaluated
                    .push(self.content[else_content.byte_range()].to_owned());
            }

            // The preserved content dictates the behavior of the following contents.
            return self.handle_node(c, previous_node);
        }

        if let Some(c) = else_content
            && !*is_true
        {
            if let Some(if_content) = if_content {
                self.deletion_stats
                    .if_evaluated
                    .push(self.content[if_content.byte_range()].to_owned());
            }
            // The preserved content dictates the behavior of the following contents.
            return self.handle_node(c, previous_node);
        }

        // Only spaces or tabs separate the node from the surrounding newlines, thus, we are fullline.
        let fullline = self.is_fullline(&node);

        self.deletion_stats
            .if_evaluated_empty
            .push(node_content.to_owned());
        Ok(IfEvaluatedEmpty(new_content, fullline))
    }

    fn handle_source_file(
        &mut self,
        node: Node<'a>,
        previous_node: NodeHandling,
        whitespaces: Vec<u8>,
    ) -> Result<NodeHandling> {
        let mut new_content = whitespaces;
        let mut i = 0;
        let mut last_byte_of_source_file = 0;
        let mut prev_node = previous_node;

        while let Some(child) = node.child(i) {
            // The `source_file` contains all whitespaces. We want to keep whitespace before the first non-sourcefile node. Thus, we check it on the first child.
            if i == 0 {
                new_content.extend_from_slice(&self.content[0..child.start_byte()]);
            }
            let result = self.handle_node(child, prev_node.clone())?;

            result.append_to(&mut new_content, &prev_node);

            prev_node = result;

            last_byte_of_source_file = child.end_byte();
            i += 1;
        }

        // We are not a main document but have text (probably whitespace) after the last node. We need to keep that text.
        if self.last_byte_of_document_block.is_none()
            && last_byte_of_source_file < self.content.len()
        {
            // The last node is a fullline comment, we need to remove the newline at the end of the comment.
            if matches!(prev_node, FullLineCommentDeleted(_)) {
                last_byte_of_source_file += 1;
            }

            new_content.extend_from_slice(&self.content[last_byte_of_source_file..]);
        }

        Ok(Kept(new_content))
    }

    fn handle_old_command_definition(
        &mut self,
        node: Node<'a>,
        previous_node: NodeHandling,
        mut whitespaces: Vec<u8>,
    ) -> Result<NodeHandling> {
        // The parameter text between declaration and implementation may consist of any number
        // of nodes (e.g. `\def\foo(#1,#2){...}`), so locate the nodes by field instead of index.
        let (Some(command), Some(decl), Some(implementation)) = (
            node.child_by_field_name("command"),
            node.child_by_field_name("declaration"),
            node.child_by_field_name("implementation"),
        ) else {
            whitespaces.extend_from_slice(&self.content[node.byte_range()]);
            return Ok(Kept(whitespaces));
        };

        // Parameters with delimiters (e.g. `\def\a#1\par{}`) swallow text that is not a curly group,
        // so invocations cannot be removed safely.
        let mut arity = 0;
        let mut delimited = false;
        let mut i = 0;
        while let Some(child) = node.child(i) {
            match child.kind() {
                _ if child == implementation => break,
                "placeholder" => arity += 1,
                "parameter_delimiter" | "command_name" if child != decl => delimited = true,
                _ => (),
            }
            i += 1;
        }

        let signature = (!delimited
            && OLD_COMMENT_COMMAND_DEFINITIONS.contains(&&self.content[command.byte_range()]))
        .then(|| Signature {
            name: &self.content[decl.byte_range()],
            arity,
            optional: false,
        });

        self.handle_command_definition(node, implementation, previous_node, whitespaces, signature)
    }

    fn handle_new_command_definition(
        &mut self,
        node: Node<'a>,
        previous_node: NodeHandling,
        mut whitespaces: Vec<u8>,
    ) -> Result<NodeHandling> {
        // Optional argument count and default value sit between declaration and implementation,
        // so locate the nodes by field instead of index.
        let (Some(command), Some(decl), Some(implementation)) = (
            node.child_by_field_name("command"),
            node.child_by_field_name("declaration"),
            node.child_by_field_name("implementation"),
        ) else {
            whitespaces.extend_from_slice(&self.content[node.byte_range()]);
            return Ok(Kept(whitespaces));
        };

        // The name is either given directly or wrapped by curly braces
        let name = match decl.kind() {
            "command_name" => Some(decl),
            _ => decl
                .child_by_field_name("command")
                .filter(|c| c.kind() == "command_name"),
        };
        let argc = match node.child_by_field_name("argc") {
            Some(argc) => argc
                .child_by_field_name("value")
                .and_then(|v| std::str::from_utf8(&self.content[v.byte_range()]).ok())
                .and_then(|v| v.parse::<usize>().ok()),
            None => Some(0),
        };
        // The first argument is optional if a default value is given
        let optional = node.child_by_field_name("default").is_some();

        let signature = match (name, argc) {
            (Some(name), Some(argc))
                if NEW_COMMENT_COMMAND_DEFINITIONS
                    .contains(&&self.content[command.byte_range()]) =>
            {
                Some(Signature {
                    name: &self.content[name.byte_range()],
                    arity: argc.saturating_sub(optional as usize),
                    optional,
                })
            }
            _ => None,
        };

        self.handle_command_definition(node, implementation, previous_node, whitespaces, signature)
    }

    /// Cleans a command definition.
    ///
    /// Definitions are always kept, even with an empty body: the command may be used implicitly
    /// (e.g. hooks like `\thefootnote`), tested for (`\@ifundefined`) or extended later
    /// (`\g@addto@macro`). Only invocations of comment commands are removed, see [`CommentCommands`].
    /// A definition with a `signature` is a comment command candidate if its cleaned body is empty.
    fn handle_command_definition(
        &mut self,
        node: Node<'a>,
        implementation: Node<'a>,
        previous_node: NodeHandling,
        whitespaces: Vec<u8>,
        signature: Option<Signature<'a>>,
    ) -> Result<NodeHandling> {
        let mut new_content = whitespaces;
        let mut prev_node = previous_node;
        let mut i = 0;

        while let Some(child) = node.child(i) {
            let result = self.handle_node(child, prev_node.clone())?;

            if child == implementation
                && let Some(signature) = &signature
            {
                self.record_definition(node, signature, result.text())?;
            }

            result.append_to(&mut new_content, &prev_node);
            prev_node = result;
            i += 1;
        }

        Ok(Kept(new_content))
    }

    /// Records a definition as comment command candidate while collecting,
    /// and counts the definitions of comment commands while cleaning.
    fn record_definition(
        &mut self,
        node: Node<'a>,
        signature: &Signature<'a>,
        cleaned_impl: &[u8],
    ) -> Result<()> {
        if !self.collecting {
            if self
                .comment_commands
                .get(signature.name)
                .is_some_and(|c| c.filename == self.filename && c.end_byte == node.end_byte())
            {
                self.deletion_stats
                    .comment_definitions
                    .push(self.content[node.byte_range()].to_owned());
            }
            return Ok(());
        }

        // Without a mandatory argument there is no payload to remove; such commands are
        // hooks, flags or switches. Internal (`@`) commands are never custom comment commands.
        // If the file opts out of comment cleaning, treat it as a normal command instead.
        if signature.arity == 0 || signature.name.contains(&b'@') || self.directives.keep_comments {
            return Ok(());
        }

        // The definition must hold for the rest of the document
        let Some(in_document) = global_scope(node, self.content) else {
            return Ok(());
        };

        // We determine empty implementations by checking if they only contain whitespace characters (and comments).
        let cleaned_impl_tree = parse(cleaned_impl).context(anyhow!(
            "Could not parse cleaned implementation of command definition: {}",
            String::from_utf8_lossy(cleaned_impl),
        ))?;
        if is_empty(&cleaned_impl_tree.root_node()) {
            self.empty_definitions.push(EmptyDefinition {
                name: signature.name.to_owned(),
                command: CommentCommand {
                    arity: signature.arity,
                    optional: signature.optional,
                    filename: self.filename.to_owned(),
                    end_byte: node.end_byte(),
                    in_document,
                },
            });
        }

        Ok(())
    }

    /// Returns the last argument to remove if the node is an invocation of a comment command.
    fn comment_invocation(&self, node: Node<'a>) -> Option<Node<'a>> {
        if self.directives.keep_comments {
            return None;
        }

        let name = node.child_by_field_name("command")?;
        let command = self
            .comment_commands
            .get(&self.content[name.byte_range()])?;

        // Invocations in front of the definition are not affected by it
        if command.filename == self.filename {
            if node.start_byte() < command.end_byte {
                return None;
            }
        } else if command.in_document {
            return None;
        }

        // Only `todo` nodes contain an optional argument
        if node.child_by_field_name("options").is_some() && !command.optional {
            return None;
        }

        // With fewer braced arguments than the command takes, it consumes other text.
        let mut cursor = node.walk();
        node.children_by_field_name("arg", &mut cursor)
            .nth(command.arity - 1)
    }

    /// Removes the invocation of a comment command up to and including its last argument.
    /// Further arguments are not consumed by the command and thus kept.
    fn handle_comment_invocation(
        &mut self,
        node: Node<'a>,
        last_argument: Node<'a>,
        whitespaces: Vec<u8>,
    ) -> Result<NodeHandling> {
        self.deletion_stats
            .comment_invocation_deleted
            .push(self.content[node.start_byte()..last_argument.end_byte()].to_owned());

        // In math mode, spaces are ignored but an empty group is an atom that changes
        // the spacing (e.g. turns a unary minus into a binary one), so delete the invocation.
        // In an alignment, an empty group starts the cell, which breaks a following `\hline` or `\multicolumn`.
        // Otherwise, we replace the command invocation with an empty curly brace to preserve formatting.
        // Simply deleting it would also delete the space it separates, e.g. the line break of an invocation
        // on its own line, which changes the spacing in the PDF.
        let delete = is_in_math_mode(&node, self.content) || is_in_alignment(&node, self.content);

        let mut new_content = whitespaces;
        if !delete {
            new_content.extend_from_slice(b"{}");
        }

        let mut prev_node = Kept(Vec::new());
        let mut child = last_argument.next_sibling();

        if child.is_none() {
            if delete {
                // Only spaces or tabs separate the node from the surrounding newlines, thus, we are fullline.
                let fullline = self.is_fullline(&node);

                return Ok(CommandInvocationDeleted(new_content, fullline));
            }

            return Ok(CommandInvocationReplaced(new_content));
        }

        while let Some(c) = child {
            let result = self.handle_node(c, prev_node.clone())?;

            result.append_to(&mut new_content, &prev_node);

            prev_node = result;
            child = c.next_sibling();
        }

        Ok(Kept(new_content))
    }
}

/// The name and arguments of a defined command.
struct Signature<'a> {
    name: &'a [u8],
    /// Number of mandatory arguments.
    arity: usize,
    /// The first argument is optional.
    optional: bool,
}

/// `\newcommand`-like definitions that can define comment commands.
/// `\providecommand` is missing as it keeps an existing definition.
const NEW_COMMENT_COMMAND_DEFINITIONS: [&[u8]; 6] = [
    b"\\newcommand",
    b"\\newcommand*",
    b"\\renewcommand",
    b"\\renewcommand*",
    b"\\DeclareRobustCommand",
    b"\\DeclareRobustCommand*",
];

/// `\def`-like definitions that can define comment commands.
const OLD_COMMENT_COMMAND_DEFINITIONS: [&[u8]; 4] = [b"\\def", b"\\gdef", b"\\edef", b"\\xdef"];

/// Environments whose content is an alignment, i.e. split into cells.
const ALIGNMENT_ENVIRONMENTS: [&[u8]; 19] = [
    b"tabular",
    b"tabular*",
    b"tabularx",
    b"tabulary",
    b"xltabular",
    b"longtable",
    b"longtable*",
    b"supertabular",
    b"supertabular*",
    b"mpsupertabular",
    b"xtabular",
    b"tabu",
    b"longtabu",
    b"tblr",
    b"longtblr",
    b"talltblr",
    b"NiceTabular",
    b"NiceTabular*",
    b"array",
];

/// Returns the name of an environment, e.g. `document`.
fn environment_name<'c>(environment: &Node, content: &'c [u8]) -> Option<&'c [u8]> {
    let name = environment
        .child_by_field_name("begin")?
        .child_by_field_name("name")?;
    let name = &content[name.byte_range()];

    Some(
        name.strip_prefix(b"{")
            .and_then(|n| n.strip_suffix(b"}"))
            .unwrap_or(name)
            .trim_ascii(),
    )
}

/// Returns `Some(in_document)` if the definition holds for the rest of the document, i.e. it is
/// not nested in a group, environment, conditional or other definition.
/// `in_document` is `true` if the definition is part of the document environment.
fn global_scope(node: Node, content: &[u8]) -> Option<bool> {
    let mut in_document = false;
    let mut current = node.parent();

    while let Some(parent) = current {
        match parent.kind() {
            // `text` is a sequence of words and commands, not a group
            "source_file"
            | "text"
            | "long_command_definition"
            | "part"
            | "chapter"
            | "section"
            | "subsection"
            | "subsubsection"
            | "paragraph"
            | "subparagraph" => (),
            "generic_environment"
                if environment_name(&parent, content) == Some(b"document".as_slice()) =>
            {
                in_document = true
            }
            _ => return None,
        }
        current = parent.parent();
    }

    Some(in_document)
}

/// Returns `true` if the node is a cell of an alignment, e.g. in a `tabular` environment.
fn is_in_alignment(node: &Node, content: &[u8]) -> bool {
    let mut current = node.parent();

    while let Some(parent) = current {
        match parent.kind() {
            "generic_environment" => {
                return environment_name(&parent, content)
                    .is_some_and(|name| ALIGNMENT_ENVIRONMENTS.contains(&name));
            }
            "curly_group" => return false,
            _ => current = parent.parent(),
        }
    }

    false
}

/// Commands that typeset their argument in text mode, also inside a formula.
const TEXT_MODE_COMMANDS: [&[u8]; 14] = [
    b"\\mbox",
    b"\\hbox",
    b"\\fbox",
    b"\\framebox",
    b"\\makebox",
    b"\\parbox",
    b"\\textrm",
    b"\\textsf",
    b"\\texttt",
    b"\\textnormal",
    b"\\textbf",
    b"\\textit",
    b"\\textup",
    b"\\intertext",
];

/// Returns `true` if the node is typeset in math mode, i.e. inside a formula
/// or math environment but not inside a nested text command (e.g. `\text` or `\mbox`).
fn is_in_math_mode(node: &Node, content: &[u8]) -> bool {
    let mut current = node.parent();

    while let Some(parent) = current {
        match parent.kind() {
            "inline_formula" | "displayed_equation" | "math_environment" => return true,
            "text_mode" => return false,
            "generic_command" => {
                if let Some(name) = parent.child(0)
                    && TEXT_MODE_COMMANDS.contains(&&content[name.byte_range()])
                {
                    return false;
                }
                current = parent.parent();
            }
            _ => current = parent.parent(),
        }
    }

    false
}

/// Returns `true` for spaces and tabs.
fn is_horizontal_whitespace(byte: u8) -> bool {
    byte == b' ' || byte == b'\t'
}

/// Strips the rest of a line that only consists of spaces or tabs, including its newline.
fn strip_rest_of_blank_line(content: &[u8]) -> &[u8] {
    let indentation = content
        .iter()
        .take_while(|&&b| is_horizontal_whitespace(b))
        .count();

    match content.get(indentation) {
        Some(&b) if is_newline(b) => strip_leading_newline(&content[indentation..]),
        _ => content,
    }
}

/// Strips trailing spaces or tabs, i.e. the indentation of a deleted line.
fn strip_trailing_indentation(content: &[u8]) -> &[u8] {
    let indentation = content
        .iter()
        .rev()
        .take_while(|&&b| is_horizontal_whitespace(b))
        .count();

    &content[..content.len() - indentation]
}

/// Strip one or more newline characters from the start of a byte slice.
/// Handles both Unix (LF) and Windows (CRLF) line endings.
/// Returns the remaining bytes after stripping newlines.
pub fn strip_leading_newline(content: &[u8]) -> &[u8] {
    match (content.first(), content.get(1)) {
        (Some(&b'\r'), Some(&b'\n')) => &content[2..],
        (Some(&b'\n'), _) => &content[1..],
        _ => content,
    }
}
