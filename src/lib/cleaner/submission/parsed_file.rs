use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
    fs::read,
    hash::Hash,
    io::{BufWriter, Write},
    path::Path,
    sync::Arc,
};

use crate::{
    cleaner::{config::CleanerConfig, submission::deletion_stats::DeletionStats},
    helper::{SourceFile, exception, is_empty, is_newline},
    parsing::{is_main_tex, parse},
};
use anyhow::{Context, Result, anyhow};
use image::ImageDecoder;
use log::{debug, warn};
use tree_sitter::{Node, Parser, Tree};

/// All common file‑extensions for image formats supported by the image crate.
pub const IMAGE_EXTENSIONS: &[&str] = &[
    "avif", "bmp", "dib", "dds", "exr", "ff", "gif", "hdr", "ico", "jpg", "jpeg", "jpe", "png",
    "pnm", "pbm", "pgm", "ppm", "qoi", "tga", "targa", "tif", "tiff", "webp",
];

/// Grammar names that are preserved as‑is during stripping.
/// These tokens are simply copied to the output without modification.
pub const GRAMMAR_PRESERVE: [&str; 38] = [
    "$",
    "$$",
    "(",
    ")",
    ",",
    "=",
    "[",
    "]",
    "bibstyle_include",
    "bibtex_include",
    "biblatex_include",
    "command_name",
    "curly_group_author_list_repeat1",
    "curly_group_glob_pattern",
    "curly_group_path",
    "delimiter",
    "hyperlink",
    "import_include",
    "latex_include",
    "number",
    "old_command_definition",
    "operator",
    "placeholder",
    "source_code",
    "subscript",
    "superscript",
    "tikz_library_import",
    "value_literal",
    "word",
    "{",
    "|",
    "}",
    "label",
    "verbatim",
    "verbatim_environment",
    "value",
    "todo_command_name",
    "argc",
];

/// Grammar names that may contain nested sub‑nodes that need to be processed recursively.
pub const GRAMMAR_NESTING: [&str; 72] = [
    "acronym_definition",
    "acronym_reference",
    "asy_enviroment",
    "asydef_environment",
    "author_declaration",
    "begin",
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
    "curly_group_command_name",
    "curly_group_impl",
    "curly_group_label",
    "curly_group_label_list",
    "curly_group_text",
    "curly_group_text_list",
    "curly_group_value",
    "curly_group_word",
    "displayed_equation",
    "end",
    "enum_item",
    "environment_definition",
    "glossary_entry_definition",
    "glossary_entry_reference",
    "graphics_include",
    "inkscape_include",
    "inline_formula",
    "key_value_pair",
    "label_definition",
    "label_number",
    "label_reference",
    "label_reference_range",
    "let_command_definition",
    "listing_environment",
    "luacode_environment",
    "math_delimiter",
    "math_environment",
    "minted_environment",
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
            *&self.cleaner_config.im_size,
            *&self.cleaner_config.im_size,
            FilterType::Gaussian,
        );
        let target = target.as_ref();

        // Write the resized image to the target file.
        let mut output_file = File::options()
            .create(true)
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
        let content = read(self.source_file.full())?;
        deletion_stats.bib_deleted.push(content);
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
}

/// Methods for handling LaTeX‑like files.
impl LatexLikeFile {
    /// Copies a cleaned version of the LaTeX file to the target path.
    /// This method strips comments, unused commands and processes custom
    /// conditional blocks before writing the cleaned content.
    pub fn copy_cleaned_to(
        &self,
        target: impl AsRef<Path>,
        grammar_errors: &mut Vec<String>,
        deletion_stats: &mut DeletionStats,
    ) -> Result<()> {
        use std::fs::File;

        let filename = self.source_file.full().to_string_lossy();

        let (stripped, grammar_err, stats) = ContentStripper::clean(
            &self.original_content,
            self.tree.root_node(),
            filename.as_ref(),
            self.cleaner_config.clone(),
        )?;

        grammar_errors.extend(grammar_err);
        deletion_stats.merge(stats);

        if let Some(stripped) = stripped {
            let mut file = BufWriter::new(File::create(target.as_ref())?);

            file.write_all(&stripped)?;
            file.flush()?;
        }

        Ok(())
    }

    /// Copies the raw (unmodified) LaTeX file to the target path.
    pub fn copy_raw_to(&self, target: impl AsRef<Path>) -> Result<()> {
        use std::fs::copy;

        copy(self.source_file.full(), target.as_ref())?;

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
    ) -> Result<Self> {
        debug!("Creating ParsedFile for: {:?}", path.as_ref());

        let mut parser = Parser::new();
        let path = path.as_ref();

        parser
            .set_language(&tree_sitter_latex::language())
            .expect("Error loading LaTeX grammar");

        let source_file = SourceFile::from_path(path, parent)?;
        debug!("Source file created: {:?}", source_file.relative());

        Ok(match path.extension() {
            Some(ext)
                if ext == "tex"
                    || (cleaner_config.clean_classes
                        && (ext == "sty"
                            || ext == "cls"
                            || ext == "dtx"
                            || ext == "bbx"
                            || ext == "cbx"
                            || ext == "ins"
                            || ext == "def"
                            || ext == "clo")) =>
            {
                let content = read(source_file.full())
                    .context(anyhow!("Failed to read LaTeX file {}", path.display()))?;

                if content.is_empty() {
                    debug!("File {} is empty", path.display());
                } else {
                    debug!(
                        "Read {} bytes from LaTeX file {}",
                        content.len(),
                        path.display()
                    );
                }

                let tree = match parser.parse(&content, None) {
                    Some(tree) => {
                        debug!("Successfully parsed LaTeX file: {}", path.display());
                        tree
                    }
                    None => {
                        warn!(
                            "Could not parse .tex/.sty/.cls file {}, no tree returned",
                            path.display()
                        );
                        return Err(anyhow!(
                            "Could not parse .tex/.sty/.cls file {}, no tree returned",
                            path.display()
                        ));
                    }
                };

                debug!("Created LatexLike ParsedFile for {}", path.display());
                Self::LatexLike(LatexLikeFile {
                    cleaner_config,
                    source_file,
                    original_content: content,
                    tree,
                    stats: Default::default(),
                })
            }
            Some(ext)
                if IMAGE_EXTENSIONS
                    .contains(&ext.to_str().context("File path is not valid utf8")?) =>
            {
                debug!(
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
            Some(ext) if ext == "bib" => {
                debug!("Detected BibTeX file: {}", path.display());
                Self::Bib(BibFile {
                    cleaner_config,
                    source_file,
                    stats: Default::default(),
                })
            }
            _ => {
                debug!("Treating as other file type: {}", path.display());
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
    pub fn copy_raw_to(&self, target: impl AsRef<Path>) -> Result<()> {
        match &self {
            ParsedFile::LatexLike(latex_like_file) => latex_like_file.copy_raw_to(target),
            ParsedFile::Bib(source_file) => source_file.copy_raw_to(target),
            ParsedFile::Image(sf) => sf.copy_raw_to(target),
            ParsedFile::Other(source_file) => source_file.copy_raw_to(target),
        }
    }

    /// Copies a cleaned version of the file to the target path.
    /// The behavior depends on the file type and configuration.
    pub fn copy_cleaned_to(
        &self,
        target: impl AsRef<Path>,
        grammar_errors: &mut Vec<String>,
        deletion_stats: &mut DeletionStats,
    ) -> Result<()> {
        match self {
            ParsedFile::LatexLike(latex_like_file) => {
                latex_like_file.copy_cleaned_to(target, grammar_errors, deletion_stats)
            }
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
            ParsedFile::Other(source_file) => source_file.copy_raw_to(target),
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
    /// The node was a comment definition that was deleted.
    CommandDefinitionDeleted(Vec<u8>, bool),
    CommandInvocationDeleted(Vec<u8>, bool),
    LongCommandInvocationDeleted(Vec<u8>),
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
            CommandDefinitionDeleted(ws, _) => Some(ws),
            CommandInvocationDeleted(ws, _) => Some(ws),
            CommentPackageIncludeDeleted(ws, _) => Some(ws),
            BlockCommentDeleted(ws, _) => Some(ws),
            LongCommandInvocationDeleted(ws) => Some(ws),
            _ => None,
        }
    }

    /// Returns a reference to the internal text.
    pub fn text(&self) -> &[u8] {
        match self {
            Kept(text) => text,
            SuffixCommentDeleted(ws) => ws,
            FullLineCommentDeleted(ws) => ws,
            IfEvaluated(ws) => ws,
            IfEvaluatedEmpty(ws, _) => ws,
            CommandDefinitionDeleted(ws, _) => ws,
            CommandInvocationDeleted(ws, _) => ws,
            CommentPackageIncludeDeleted(ws, _) => ws,
            BlockCommentDeleted(ws, _) => ws,
            LongCommandInvocationDeleted(ws) => ws,
            _ => &[],
        }
    }

    /// Appends the current node's text to an existing output string,
    /// taking into account the previous node's handling state.
    ///
    /// The logic here ensures that line endings and comment delimiters
    /// are preserved correctly when nodes are removed or kept.
    pub fn append_to(&self, existing: &mut Vec<u8>, prev: &NodeHandling) {
        match &prev {
            OutOfBounds => (),
            BlockCommentDeleted(_, false) => {
                // Drop the newline if it exists as it is consumed by the comment env
                // This code is complex as Rust does not offer another way to drop a prefix in-place
                let new_existing = strip_leading_newline(existing);
                let mut new_existing = new_existing.to_owned();

                new_existing.extend_from_slice(self.text());
                existing.clear();
                existing.append(&mut new_existing);
            }
            NoNode
            | Kept(_)
            | SuffixCommentDeleted(_)
            | IfEvaluated(_)
            | IfEvaluatedEmpty(_, false)
            | CommandDefinitionDeleted(_, false)
            | CommandInvocationDeleted(_, false)
            | LongCommandInvocationDeleted(_)
            | CommentPackageIncludeDeleted(_, false) => {
                existing.append(&mut self.text().to_owned())
            }
            FullLineCommentDeleted(_)
            | BlockCommentDeleted(_, true)
            | IfEvaluatedEmpty(_, true)
            | CommandDefinitionDeleted(_, true)
            | CommandInvocationDeleted(_, true)
            | CommentPackageIncludeDeleted(_, true) => {
                if matches!(self, Self::FullLineCommentDeleted(_)) {
                    return;
                }

                let mut text = self.text();
                let first = text.iter().next();
                let first_is_newline = first.map_or(false, |&b| is_newline(b));

                if first_is_newline {
                    text = strip_leading_newline(text);
                }

                existing.extend_from_slice(text);
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
    command_definitions: HashSet<Vec<u8>>,
    long_command_definitions: HashSet<Vec<u8>>,
    last_byte_of_document_block: Option<usize>,
    grammar_errors: Vec<String>,
    deletion_stats: DeletionStats,
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
    ) -> Result<(Option<Vec<u8>>, Vec<String>, DeletionStats)> {
        debug!("Starting content stripping for file: {}", filename);
        debug!("Content size: {} bytes", content.len());

        let mut stripper = Self::new(content, filename, cleaner_config);

        let result = stripper.handle_node(root, NoNode).map(|v| {
            (
                v.into_inner(),
                stripper.grammar_errors,
                stripper.deletion_stats,
            )
        });

        if let Ok((ref cleaned, ref errors, _)) = result {
            debug!("Content stripping completed for {}", filename);
            debug!("Grammar errors found: {}", errors.len());
            if let Some(cleaned_content) = cleaned {
                debug!("Cleaned content size: {} bytes", cleaned_content.len());
            }
        }

        result
    }

    fn new(content: &'a [u8], filename: &'a str, cleaner_config: Arc<CleanerConfig>) -> Self {
        Self {
            filename,
            content,
            cleaner_config,
            if_values: Default::default(),
            command_definitions: Default::default(),
            long_command_definitions: Default::default(),
            last_byte_of_document_block: None,
            grammar_errors: Vec::new(),
            deletion_stats: Default::default(),
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

        let return_val = match grammar_name {
            // An error node was hit, this means that there was a parsing error. This can be attributed to a malformed latex document or an issue with the tree-sitter grammar.
            // The content of the node is simply preserved.
            "ERROR" => {
                debug!(
                    "Found ERROR node at line {}, column {} in {}",
                    node.start_position().row + 1,
                    node.start_position().column,
                    self.filename
                );

                let g_err = format!(
                    "Found ERROR node when traversing tree-sitter tree of file '{}:{}:{}' with content: {:?}",
                    self.filename,
                    node.start_position().row + 1,
                    node.start_position().column,
                    String::from_utf8_lossy(node_content)
                );

                self.grammar_errors.push(g_err);

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
                    .map(|v| *v)
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
            "long_command_definition" => {
                let nested_command_handled = match node.child(1) {
                    Some(c) if c.grammar_name() == "new_command_definition" => {
                        self.handle_new_command_definition(c, previous_node, whitespaces, true)?
                    }
                    Some(c) if c.grammar_name() == "old_command_definition" => {
                        self.handle_old_command_definition(c, previous_node, whitespaces, true)?
                    }
                    Some(_) | None => {
                        whitespaces.extend_from_slice(node_content);
                        return Ok(Kept(whitespaces));
                    }
                };

                match nested_command_handled {
                    Kept(mut s) => {
                        let mut prefix = "\\long{}".as_bytes().to_owned();
                        prefix.append(&mut s);
                        Ok(Kept(s))
                    }
                    CommandDefinitionDeleted(s, _) => {
                        // The byte in front of and after the current node is a newline, thus, we are fullline.
                        let fullline = self
                            .content
                            .get(node.start_byte() - 1)
                            .map_or(false, |&b| is_newline(b))
                            && self
                                .content
                                .get(node.end_byte())
                                .map_or(false, |&b| is_newline(b));

                        // We need the fulline argument corresponding to the wrapped \long\...
                        Ok(CommandDefinitionDeleted(s, fullline))
                    }
                    _ => unreachable!("Long command without actual command definition"),
                }
            }
            "new_command_definition" => {
                self.handle_new_command_definition(node, previous_node, whitespaces, false)
            }
            "old_command_definition" => {
                self.handle_old_command_definition(node, previous_node, whitespaces, false)
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

                if &self.content[path.byte_range()] == b"comment" {
                    // The byte in front of and after the current node is a newline, thus, we are fullline.
                    let fullline = self
                        .content
                        .get(node.start_byte() - 1)
                        .map_or(false, |&b| is_newline(b))
                        && self
                            .content
                            .get(node.end_byte())
                            .map_or(false, |&b| is_newline(b))
                        && self
                            .content
                            .get(node.end_byte() + 1)
                            .map_or(false, |&b| is_newline(b));

                    self.deletion_stats
                        .comment_package_include_deleted
                        .push(node_content.to_owned());
                    Ok(CommentPackageIncludeDeleted(whitespaces, fullline))
                } else {
                    whitespaces.extend_from_slice(node_content);
                    return Ok(Kept(whitespaces));
                }
            }
            "generic_command" => {
                // We want to check if this command invocation references a command with an empty body that we previously added to self.command_definitions
                let name = match node.child(0) {
                    Some(c) => c,
                    None => {
                        whitespaces.extend_from_slice(node_content);
                        return Ok(Kept(whitespaces));
                    }
                };

                let name_str = &self.content[name.byte_range()];

                if self.command_definitions.contains(name_str) {
                    // The byte in front of and after the current node is a newline, thus, we are fullline.
                    let fullline = self
                        .content
                        .get(node.start_byte() - 1)
                        .map_or(false, |&b| is_newline(b))
                        && self
                            .content
                            .get(node.end_byte())
                            .map_or(false, |&b| is_newline(b));
                    // We are disabling this requirement for now, as it seems to be the way to go.
                    // && self.content.get(node.end_byte() + 1).map_or(false, |&b| is_newline(b));

                    self.deletion_stats
                        .comment_invocation_deleted
                        .push(node_content.to_owned());
                    return Ok(CommandInvocationDeleted(whitespaces, fullline));
                }

                if self.long_command_definitions.contains(name_str) {
                    self.deletion_stats
                        .comment_invocation_deleted
                        .push(node_content.to_owned());

                    // We want to replace the command invocation with an empty curly brace to preserve formatting.
                    // This is special to commands defined with \long
                    let mut replacement_text = whitespaces;
                    replacement_text.push(b'{');
                    replacement_text.push(b'}');

                    return Ok(LongCommandInvocationDeleted(replacement_text));
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
                let mut is_document_environment = false;
                let mut prev_node = previous_node;

                while let Some(child) = node.child(i) {
                    // The \begin{document} block can be formatted in various ways (and include comments!)
                    // This is an extensive way to check if this "begin" block is actually the document environment
                    if child.grammar_name() == "begin"
                        && let Some(name) = child.child_by_field_name("name")
                        && let Some(text) = name.child_by_field_name("text")
                    {
                        let mut i = 0;
                        let mut complete_group_name = Vec::new();

                        while let Some(child) = text.child(i) {
                            if child.grammar_name() == "word" {
                                complete_group_name
                                    .extend_from_slice(&self.content[child.byte_range()]);
                            }
                            i += 1;
                        }

                        if complete_group_name == b"document" {
                            is_document_environment = true
                        }
                    }

                    let result = self.handle_node(child, prev_node.clone())?;

                    result.append_to(&mut new_content, &prev_node);

                    prev_node = result;

                    i += 1;
                }

                // If this is the document environment, then we want to remove all content after this node, thus we store the last byte
                if is_document_environment {
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
                return Ok(Kept(whitespaces));
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
                return Ok(Kept(whitespaces));
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
                return Ok(Kept(whitespaces));
            }
            // Handle if if else blocks seperatly
            "general_if" => self.handle_if_block(node, node_content, whitespaces),
            r"\iffalse" | r"\fi" | r"\if0" | r"\iftrue" | r"\else" | "comment_environment" => {
                // The byte in front of and after the current node is a newline, thus, we are fullline.
                let fullline = self
                    .content
                    .get(node.start_byte() - 1)
                    .map_or(false, |&b| is_newline(b))
                    && self
                        .content
                        .get(node.end_byte())
                        .map_or(false, |&b| is_newline(b))
                    && self
                        .content
                        .get(node.end_byte() + 1)
                        .map_or(false, |&b| is_newline(b));

                self.deletion_stats
                    .block_comment_deleted
                    .push(node_content.to_owned());
                Ok(BlockCommentDeleted(whitespaces, fullline))
            }
            // Preserve all these nodes as-is.
            x if GRAMMAR_PRESERVE.contains(&x) || x.starts_with(r"\") => {
                whitespaces.extend_from_slice(node_content);
                return Ok(Kept(whitespaces));
            }
            // Keep all other nodes that are not listed in this match statement and log a warning
            other => {
                warn!(
                    "Unhandled grammar '{}' with content {:?}",
                    other,
                    node.utf8_text(&self.content).unwrap()
                );

                whitespaces.extend_from_slice(node_content);
                return Ok(Kept(whitespaces));
            }
        };

        return_val
    }

    /// Extracts the whitespace that appears between the previous sibling node
    /// and the current node. This whitespace is preserved in the output.
    fn whitespace_to_previous_node(&self, node: &Node<'a>) -> Vec<u8> {
        if let Some(prev) = node.prev_sibling() {
            let content_inbetween = &self.content[prev.end_byte()..node.start_byte()];
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
                // The byte in front of and after the current node is a newline, thus, we are fullline.
                let fullline = self
                    .content
                    .get(node.start_byte() - 1)
                    .map_or(false, |&b| is_newline(b))
                    && self
                        .content
                        .get(node.end_byte())
                        .map_or(false, |&b| is_newline(b))
                    && self
                        .content
                        .get(node.end_byte() + 1)
                        .map_or(false, |&b| is_newline(b));

                self.deletion_stats
                    .if_evaluated_empty
                    .push(self.content[node.byte_range()].to_owned());
                return Ok(IfEvaluatedEmpty(whitespaces, fullline));
            }
        };

        let grammar_name = first_child.grammar_name();

        match grammar_name {
            r"\iffalse" | r"\if0" => {
                // First node is content
                // Second node is \else or \fi
                // Third node is content of else block if present (content should be kept)
                // Fourth node is \fi if present
                if let Some(fi_or_else) = node.child(2) {
                    if fi_or_else.grammar_name() == r"\else" {
                        if let Some(else_content) = node.child(3) {
                            if let Some(if_content) = node.child(1) {
                                self.deletion_stats
                                    .if_evaluated
                                    .push(self.content[if_content.byte_range()].to_owned());
                            }

                            whitespaces.extend_from_slice(&self.content[else_content.byte_range()]);
                            return Ok(IfEvaluated(whitespaces));
                        }
                    }
                }

                // The byte in front of and after the current node is a newline, thus, we are fullline.
                let fullline = self
                    .content
                    .get(node.start_byte() - 1)
                    .map_or(false, |&b| is_newline(b))
                    && self
                        .content
                        .get(node.end_byte())
                        .map_or(false, |&b| is_newline(b))
                    && self
                        .content
                        .get(node.end_byte() + 1)
                        .map_or(false, |&b| is_newline(b));

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

                    whitespaces.extend_from_slice(&self.content[if_content.byte_range()]);
                    Ok(IfEvaluated(whitespaces))
                } else {
                    // The byte in front of and after the current node is a newline, thus, we are fullline.
                    let fullline = self
                        .content
                        .get(node.start_byte() - 1)
                        .map_or(false, |&b| is_newline(b))
                        && self
                            .content
                            .get(node.end_byte())
                            .map_or(false, |&b| is_newline(b))
                        && self
                            .content
                            .get(node.end_byte() + 1)
                            .map_or(false, |&b| is_newline(b));

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
                        // The byte in front of and after the current node is a newline, thus, we are fullline.
                        let fullline = self
                            .content
                            .get(node.start_byte() - 1)
                            .map_or(false, |&b| is_newline(b))
                            && self
                                .content
                                .get(node.end_byte())
                                .map_or(false, |&b| is_newline(b))
                            && self
                                .content
                                .get(node.end_byte() + 1)
                                .map_or(false, |&b| is_newline(b));

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
                            // The byte in front of and after the current node is a newline, thus, we are fullline.
                            let fullline = self
                                .content
                                .get(node.start_byte() - 1)
                                .map_or(false, |&b| is_newline(b))
                                && self
                                    .content
                                    .get(node.end_byte())
                                    .map_or(false, |&b| is_newline(b))
                                && self
                                    .content
                                    .get(node.end_byte() + 1)
                                    .map_or(false, |&b| is_newline(b));

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

                    // The byte in front of and after the current node is a newline, thus, we are fullline.
                    let fullline = self
                        .content
                        .get(node.start_byte() - 1)
                        .map_or(false, |&b| is_newline(b))
                        && self
                            .content
                            .get(node.end_byte())
                            .map_or(false, |&b| is_newline(b))
                        && self
                            .content
                            .get(node.end_byte() + 1)
                            .map_or(false, |&b| is_newline(b));

                    self.deletion_stats
                        .if_evaluated_empty
                        .push(self.content[node.byte_range()].to_owned());
                    return Ok(IfEvaluatedEmpty(whitespaces, fullline));
                }

                // The byte in front of and after the current node is a newline, thus, we are fullline.
                let fullline = self
                    .content
                    .get(node.start_byte() - 1)
                    .map_or(false, |&b| is_newline(b))
                    && self
                        .content
                        .get(node.end_byte())
                        .map_or(false, |&b| is_newline(b))
                    && self
                        .content
                        .get(node.end_byte() + 1)
                        .map_or(false, |&b| is_newline(b));

                self.deletion_stats
                    .if_evaluated_empty
                    .push(self.content[node.byte_range()].to_owned());
                return Ok(IfEvaluatedEmpty(whitespaces, fullline));
            }
            "text" => self.handle_node(first_child, previous_node),
            "general_if" => self.handle_if_block(
                first_child,
                &self.content[first_child.byte_range()],
                whitespaces,
            ),
            _ => {
                // The byte in front of and after the current node is a newline, thus, we are fullline.
                let fullline = self
                    .content
                    .get(node.start_byte() - 1)
                    .map_or(false, |&b| is_newline(b))
                    && self
                        .content
                        .get(node.end_byte())
                        .map_or(false, |&b| is_newline(b))
                    && self
                        .content
                        .get(node.end_byte() + 1)
                        .map_or(false, |&b| is_newline(b));

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
        node_content: &'a [u8],
        mut whitespaces: Vec<u8>,
    ) -> Result<NodeHandling> {
        assert!(node.grammar_name() == "general_if");

        let first_byte_of_next_node = match node.child(0) {
            Some(c) => c.start_byte(),
            None => match node.next_sibling() {
                Some(c) => c.start_byte(),
                None => {
                    whitespaces.extend_from_slice(node_content);
                    return exception(
                        anyhow!("Custom if does not have any child or subsequent nodes."),
                        Kept(whitespaces),
                        "tex-cleaner",
                        self.cleaner_config.force,
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
            None => {
                whitespaces.extend_from_slice(node_content);
                return exception(
                    anyhow!(
                        "Custom if with name '{}' not defined",
                        String::from_utf8_lossy(name)
                    ),
                    Kept(whitespaces),
                    "tex-cleaner",
                    self.cleaner_config.force,
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

            whitespaces.extend_from_slice(&self.content[c.byte_range()]);
            return Ok(IfEvaluated(whitespaces));
        }

        if let Some(c) = else_content
            && !*is_true
        {
            if let Some(if_content) = if_content {
                self.deletion_stats
                    .if_evaluated
                    .push(self.content[if_content.byte_range()].to_owned());
            }
            whitespaces.extend_from_slice(&self.content[c.byte_range()]);
            return Ok(IfEvaluated(whitespaces));
        }

        // The byte in front of and after the current node is a newline, thus, we are fullline.
        let fullline = self
            .content
            .get(node.start_byte() - 1)
            .map_or(false, |&b| is_newline(b))
            && self
                .content
                .get(node.end_byte())
                .map_or(false, |&b| is_newline(b))
            && self
                .content
                .get(node.end_byte() + 1)
                .map_or(false, |&b| is_newline(b));

        self.deletion_stats
            .if_evaluated_empty
            .push(node_content.to_owned());
        Ok(IfEvaluatedEmpty(whitespaces, fullline))
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
        long: bool,
    ) -> Result<NodeHandling> {
        let mut new_content = whitespaces.clone();
        let node_content = &self.content[node.byte_range()];
        let mut prev_node = previous_node;

        let new_command = match node.child(0) {
            Some(c) => c,
            None => {
                whitespaces.extend_from_slice(node_content);
                return Ok(Kept(whitespaces));
            }
        };
        let cleaned_command = self.handle_node(new_command, prev_node.clone())?;

        cleaned_command.append_to(&mut new_content, &prev_node);
        prev_node = cleaned_command;

        // Declaration node
        let decl = match node.child(1) {
            Some(c) => c,
            None => {
                whitespaces.extend_from_slice(node_content);
                return Ok(Kept(whitespaces));
            }
        };
        // Name of command
        let cleaned_decl = self.handle_node(decl, prev_node.clone())?;

        cleaned_decl.append_to(&mut new_content, &prev_node);
        prev_node = cleaned_decl.clone();

        // Name inside declaration node
        // If it has a child, then the name is wrapped by curly braces
        let name = match decl.child(1) {
            Some(c) => &self.content[c.byte_range()],
            None => cleaned_decl.text(),
        };

        let args = match node.child(2) {
            Some(c) => c,
            None => {
                whitespaces.extend_from_slice(node_content);
                return Ok(Kept(whitespaces));
            }
        };
        let cleaned_args = self.handle_node(args, prev_node.clone())?;

        cleaned_args.append_to(&mut new_content, &prev_node);
        prev_node = cleaned_args;

        // Extract implementation of new command
        let implementation = match node.child(3) {
            Some(c) => c,
            None => {
                whitespaces.extend_from_slice(node_content);
                return Ok(Kept(whitespaces));
            }
        };
        let cleaned_impl = self.handle_node(implementation, prev_node.clone())?;

        // If the implementation is empty, we treat it as a comment command, thus all invocations should be deleted.
        // We determine empty implementations by checking if they only contain whitespace characters.
        let cleaned_impl_tree = parse(cleaned_impl.text()).context(anyhow!(
            "Could not parse cleaned implementation of oldcommand: {}",
            String::from_utf8_lossy(cleaned_impl.text()),
        ))?;
        if is_empty(&cleaned_impl_tree.root_node()) {
            // If the implementation is empty, we treat it as a comment command, thus all invocations should be deleted.
            if long {
                self.long_command_definitions.insert(name.to_owned());

                let mut deleted_content = Vec::from(b"\\long{}");
                deleted_content.extend_from_slice(node_content);

                self.deletion_stats
                    .comment_definition_deleted
                    .push(deleted_content);
            } else {
                self.command_definitions.insert(name.to_owned());

                self.deletion_stats
                    .comment_definition_deleted
                    .push(node_content.to_owned());
            }

            // The byte in front of and after the current node is a newline, thus, we are fullline.
            let fullline = self
                .content
                .get(node.start_byte() - 1)
                .map_or(false, |&b| is_newline(b))
                && self
                    .content
                    .get(node.end_byte())
                    .map_or(false, |&b| is_newline(b));

            self.deletion_stats
                .comment_definition_deleted
                .push(node_content.to_owned());
            Ok(CommandDefinitionDeleted(whitespaces, fullline))
        } else {
            cleaned_impl.append_to(&mut new_content, &prev_node);
            prev_node = cleaned_impl;

            // Clean remaining children (starting with index 4)
            let mut i = 4;

            // Clean all child nodes inside the command body
            while let Some(child) = node.child(i) {
                let result = self.handle_node(child, prev_node.clone())?;

                result.append_to(&mut new_content, &prev_node);

                prev_node = result;
                i += 1;
            }

            Ok(Kept(new_content))
        }
    }

    fn handle_new_command_definition(
        &mut self,
        node: Node<'a>,
        previous_node: NodeHandling,
        mut whitespaces: Vec<u8>,
        long: bool,
    ) -> Result<NodeHandling> {
        let mut new_content = whitespaces.clone();
        let node_content = &self.content[node.byte_range()];
        let mut prev_node = previous_node;

        let new_command = match node.child(0) {
            Some(c) => c,
            None => {
                whitespaces.extend_from_slice(node_content);
                return Ok(Kept(whitespaces));
            }
        };
        let cleaned_command = self.handle_node(new_command, prev_node.clone())?;

        cleaned_command.append_to(&mut new_content, &prev_node);
        prev_node = cleaned_command;

        // Declaration node
        let decl = match node.child(1) {
            Some(c) => c,
            None => {
                whitespaces.extend_from_slice(node_content);
                return Ok(Kept(whitespaces));
            }
        };
        let cleaned_decl = self.handle_node(decl, prev_node.clone())?;

        cleaned_decl.append_to(&mut new_content, &prev_node);
        prev_node = cleaned_decl.clone();

        // Name inside declaration node
        // If it has a child, then the name is wrapped by curly braces
        let name = match decl.child(1) {
            Some(c) => &self.content[c.byte_range()],
            None => cleaned_decl.text(),
        };

        let argc = match node.child(2) {
            Some(c) => c,
            None => {
                whitespaces.extend_from_slice(node_content);
                return Ok(Kept(whitespaces));
            }
        };
        let cleaned_argc = self.handle_node(argc, prev_node.clone())?;

        cleaned_argc.append_to(&mut new_content, &prev_node);
        prev_node = cleaned_argc;

        // Extract implementation of new command
        let implementation = match node.child(3) {
            Some(c) => c,
            None => {
                whitespaces.extend_from_slice(node_content);
                return Ok(Kept(whitespaces));
            }
        };
        let cleaned_impl = self.handle_node(implementation, prev_node.clone())?;

        // If the implementation is empty, we treat it as a comment command, thus all invocations should be deleted.
        // We determine empty implementations by checking if they only contain whitespace characters.
        let cleaned_impl_tree = parse(cleaned_impl.text()).context(anyhow!(
            "Could not parse cleaned implementation of newcommand: {}",
            String::from_utf8_lossy(cleaned_impl.text()),
        ))?;
        if is_empty(&cleaned_impl_tree.root_node()) {
            // If the implementation is empty, we treat it as a comment command, thus all invocations should be deleted.
            if long {
                self.long_command_definitions.insert(name.to_owned());

                let mut deleted_content = Vec::from(b"\\long{}");
                deleted_content.extend_from_slice(node_content);

                self.deletion_stats
                    .comment_definition_deleted
                    .push(deleted_content);
            } else {
                self.command_definitions.insert(name.to_owned());

                self.deletion_stats
                    .comment_definition_deleted
                    .push(node_content.to_owned());
            }

            // The byte in front of and after the current node is a newline, thus, we are fullline.
            let fullline = self
                .content
                .get(node.start_byte() - 1)
                .map_or(false, |&b| is_newline(b))
                && self
                    .content
                    .get(node.end_byte())
                    .map_or(false, |&b| is_newline(b));

            Ok(CommandDefinitionDeleted(whitespaces, fullline))
        } else {
            cleaned_impl.append_to(&mut new_content, &prev_node);
            prev_node = cleaned_impl;

            // Clean remaining children (starting with index 4)
            let mut i = 4;

            // Clean all child nodes inside the command body
            while let Some(child) = node.child(i) {
                let result = self.handle_node(child, prev_node.clone())?;

                result.append_to(&mut new_content, &prev_node);

                prev_node = result;
                i += 1;
            }

            Ok(Kept(new_content))
        }
    }
}

/// Strip one or more newline characters from the start of a byte slice.
/// Handles both Unix (LF) and Windows (CRLF) line endings.
/// Returns the remaining bytes after stripping newlines.
pub fn strip_leading_newline(content: &[u8]) -> &[u8] {
    if content[0] == b'\r' && content[1] == b'\n' {
        &content[2..]
    } else if content[0] == b'\n' {
        &content[1..]
    } else {
        &content
    }
}
