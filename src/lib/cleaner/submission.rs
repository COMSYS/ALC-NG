use anyhow::{Context, Result};
use flate2::{Compression, write::GzEncoder};
use image::DynamicImage;
use log::{error, info, trace, warn};
use std::{
    collections::{HashMap, HashSet},
    ffi::OsStr,
    fs::{File, FileTimes},
    io::Write,
    ops::Not,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::Arc,
};
use temp_dir::TempDir;

use crate::{
    arxiv::{FileUsage, ZeroZeroReadMe},
    cleaner::{
        config::CleanerConfig,
        submission::{deletion_stats::DeletionStats, parsed_file::ParsedFile},
    },
    compare::{Comparer, PixelPerfect},
    exif_tool,
    helper::{ResultOkWithWarning as _, SourceFile, find_mains},
};

pub mod deletion_stats;
pub mod parsed_file;

type CompareResult = Result<HashMap<String, HashMap<usize, (DynamicImage, DynamicImage)>>>;

/// This struct contains all information needed to clean a LaTeX project.
pub struct Submission {
    /// Configuration of the cleaner. Defines how to handle cases and defines behavior.
    cleaner_config: Arc<CleanerConfig>,
    /// The input directory of the latex project.
    input_path: PathBuf,
    /// A directory that will contain the cleaned latex project.
    target_path: PathBuf,
    /// A temporary directory that is used to determine used files. This uses `TempDir` to create a temporary directory and ensures it is deleted when the struct is dropped.
    cache_path: TempDir,
    /// Potentially store a parsed 00readme.
    zzrm: Option<ZeroZeroReadMe>,
    /// HashSet that stores all initial files that are part of the project.
    initial_file_list: HashSet<ParsedFile>,
    /// Files recorded as inputs during the process.
    recorder_inputs: HashSet<SourceFile>,
    /// Files recorded as outputs during the process.
    recorder_outputs: HashSet<SourceFile>,
    /// Map storing the output of the compilation process for each potential main file.
    latex_output: HashMap<SourceFile, std::io::Result<Output>>,
    /// List of bib files that were referenced in the log output during the compilation process of latexmk. A .bib file that is compiled by bibtex during latexmk is not listed in the recorder option of pdflatex.
    latexmk_referenced_bibs: HashSet<SourceFile>,
    /// A set of possible latex files that contain text hinting at them being a main tex file.
    possible_main_files: HashSet<SourceFile>,
    /// The command used to compile the LaTeX document.
    latex_cmd: String,
    /// Additional parameters passed to the LaTeX compiler.
    latex_parameters: Vec<String>,
    /// Store grammar errors
    pub grammar_errors: Vec<String>,
    /// Store deleted string
    pub deletion_stats: DeletionStats,
}

impl Submission {
    /// Creates a new `Submission` instance.
    ///
    /// This function sets up all the necessary paths, creates a temporary cache
    /// directory, attempts to read a `00Readme` file from the input folder, and
    /// prepares the internal data structures used during the cleaning process.
    ///
    /// # Parameters
    /// - `input_path`: Path to the original LaTeX project directory.
    /// - `target_path`: Path where the cleaned project will be written.
    /// - `latex_cmd`: LaTeX compiler executable (e.g., `"pdflatex"`).
    /// - `latex_parameters`: Additional command line arguments for the compiler.
    ///
    /// # Returns
    /// A `Result<Self>` containing the fully initialized `Submission` on success,
    /// or an error if any of the required filesystem operations fail.
    ///
    /// # Notes
    /// - The function attempts to read a `00Readme` file; if it cannot be
    ///   obtained, a warning is logged and the field is set to `None`.
    /// - Both `target_path` and the temporary cache directory are created
    ///   if they do not already exist.
    pub fn new(
        cleaner_config: Arc<CleanerConfig>,
        input_path: impl AsRef<Path>,
        target_path: impl AsRef<Path>,
        latex_cmd: &str,
        latex_parameters: &Vec<String>,
    ) -> Result<Self> {
        use std::fs::create_dir_all;

        trace!(
            "Initializing Submission with input_path: {:?}",
            input_path.as_ref()
        );
        trace!("Target path: {:?}", target_path.as_ref());
        trace!("LaTeX command: {}", latex_cmd);
        trace!("LaTeX parameters: {:?}", latex_parameters);

        // Create a temporary directory for caching files during the cleaning process.
        let cache_dir =
            TempDir::with_prefix("alc-ng_").context("Could not obtain a temporary directory.")?;
        trace!("Created temporary cache directory: {:?}", cache_dir.path());

        // Convert the input and target paths to owned `PathBuf`s for later use.
        let input_path = input_path.as_ref().to_path_buf();
        let target_path = target_path.as_ref().to_path_buf();

        // Attempt to read a 00Readme file from the input folder.
        // If not present or an error occurs, log the information and set `zzrm` to `None`.
        trace!(
            "Attempting to parse 00Readme file (no_zzrm config: {})",
            cleaner_config.no_zzrm
        );
        let zzrm = match cleaner_config
            .no_zzrm
            .not()
            .then(|| ZeroZeroReadMe::new_from_folder(&input_path))
        {
            Some(Ok(zzrm)) => {
                trace!("Successfully parsed 00Readme file");
                Some(zzrm)
            }
            None => {
                trace!("00Readme parsing skipped (no_zzrm is enabled)");
                None
            }
            Some(Err(e)) => {
                info!(
                    "00Readme could not be obtained due to the following reason: {}",
                    e
                );
                None
            }
        };

        // Ensure the output directory exists; create it if it does not.
        create_dir_all(&target_path).context("Failed to create output directory")?;
        trace!("Created/verified target directory: {:?}", target_path);

        // Ensure the temporary cache directory exists.
        create_dir_all(&cache_dir).context("Faild to create cache directory")?;
        trace!("Created/verified cache directory: {:?}", cache_dir.path());

        // Construct the `Submission` struct with all fields initialized.
        trace!("Submission instance created successfully");
        Ok(Self {
            cleaner_config,
            input_path,
            target_path,
            cache_path: cache_dir,
            zzrm,
            initial_file_list: HashSet::new(),
            recorder_inputs: HashSet::new(),
            recorder_outputs: HashSet::new(),
            latex_output: HashMap::new(),
            latexmk_referenced_bibs: HashSet::new(),
            possible_main_files: HashSet::new(),
            latex_cmd: latex_cmd.to_string(),
            latex_parameters: latex_parameters.clone(),
            grammar_errors: Vec::new(),
            deletion_stats: DeletionStats::default(),
        })
    }

    /// Returns a set of main source files to compile.
    ///
    /// If a `00Readme` was successfully parsed (`zzrm` is `Some`),
    /// its top‑level files are used as the main files. Otherwise,
    /// the set of possible main files discovered earlier is returned.
    pub fn get_mains(&self) -> HashSet<SourceFile> {
        // If a 00Readme exists, use its declared top‑level files.
        if let Some(zzrm) = &self.zzrm {
            zzrm.top_level_files(&self.cache_path)
        } else {
            // No 00Readme, fall back to the previously found possible mains.
            self.possible_main_files.clone()
        }
    }

    /// Creates a temporary cache of the project by copying all input files into
    /// a temporary directory while building an internal list of parsed files.
    fn create_caches(&mut self) {
        use std::fs::{copy, create_dir_all};
        use walkdir::WalkDir;

        trace!(
            "Starting cache creation from input path: {:?}",
            self.input_path
        );

        // Create a WalkDir to iterate over all files and directories in the input path
        let walker = WalkDir::new(&self.input_path);

        // Build the list of initial files by walking the directory tree,
        // copying files into the cache, and creating ParsedFile instances.
        let _: Vec<_> = walker
            // Filter out entries that failed to read, logging any errors.
            .into_iter()
            .filter_map(|entry| match entry {
                Ok(entry) => Some(entry),
                Err(e) => {
                    error!("Error reading directory entry {:?}", e);
                    None
                }
            })
            // Process directories separately: ensure the corresponding cache
            // directories exist, but do not add them to the file list.
            .filter_map(|entry| {
                if entry.path().is_dir() {
                    let relative_path = entry
                        .path()
                        .strip_prefix(&self.input_path)
                        .context("Failed to strip prefix")
                        .ok_with_warning()?;

                    let target = self.cache_path.path().join(relative_path);
                    if let Err(err) = create_dir_all(target).context("Failed to create directory") {
                        error!("Failed to create directory: {}", err);
                    }

                    None
                } else {
                    Some(entry)
                }
            })
            // For files: copy them into the cache and create a ParsedFile.
            .map(|entry| -> anyhow::Result<()> {
                let relative_path = entry
                    .path()
                    .strip_prefix(&self.input_path)
                    .context("Failed to strip prefix")?;

                let target = self.cache_path.path().join(relative_path);

                let full_path = entry
                    .path()
                    .canonicalize()
                    .context("Failed to canonicalize path")?;

                if full_path.is_file() {
                    let _ = copy(&full_path, &target).context(format!(
                        "Failed to copy file from {:?} to {:?}",
                        full_path, target,
                    ))?;

                    trace!("Processing file: {:?}", entry.path());
                }

                Ok(())
            })
            // Ignore any errors from ParsedFile construction, logging them.
            .filter_map(|v| match v {
                Ok(v) => Some(v),
                Err(err) => {
                    error!("Error while copying file: {:?}", err);
                    None
                }
            })
            // Collect the resulting ParsedFile objects into a HashSet.
            .collect();
    }

    fn set_inital_file_list(&mut self) {
        use walkdir::WalkDir;

        let walker = WalkDir::new(self.cache_path.path());

        self.initial_file_list = walker
            // Filter out entries that failed to read, logging any errors.
            .into_iter()
            .filter_map(|entry| match entry {
                Ok(entry) => Some(entry),
                Err(e) => {
                    error!("Error reading directory entry {:?}", e);
                    None
                }
            })
            // Process directories separately: ensure the corresponding cache
            // directories exist, but do not add them to the file list.
            .filter_map(|entry| {
                if entry.path().is_dir() {
                    None
                } else {
                    Some(entry)
                }
            })
            // For files: copy them into the cache and create a ParsedFile.
            .map(|entry| {
                let full_path = entry
                    .path()
                    .canonicalize()
                    .context("Failed to canonicalize path")?;

                ParsedFile::new(
                    self.cleaner_config.clone(),
                    full_path,
                    self.cache_path.path(),
                    &self.input_path,
                )
                .context("Failed to create ParsedFile instance")
            })
            // Ignore any errors from ParsedFile construction, logging them.
            .filter_map(|v| match v {
                Ok(v) => Some(v),
                Err(err) => {
                    error!("Error while parsing file: {:?}", err);
                    None
                }
            })
            // Collect the resulting ParsedFile objects into a HashSet.
            .collect();
    }

    /// Compiles the identified LaTeX main files and records the compiler output.
    fn compile(&mut self) {
        use std::process::{Command, Stdio};

        let main_files = self.get_mains();

        // Build a vector of (main_file, command) tuples for each LaTeX main file.
        // Each command is configured with the compiler executable, flags,
        // additional parameters, the path to the main file, and output piping.
        let commands: Vec<_> = main_files
            .iter()
            .map(|file| {
                let mut cmd = Command::new(&self.latex_cmd);

                // Run in batch mode, record used files, and output PDF.
                cmd.args([
                    "-cd",
                    "-f",
                    "-pdf",
                    "-interaction=nonstopmode",
                    "-synctex=1",
                    "-recorder",
                ]);

                // Append any additional LaTeX compiler arguments.
                cmd.args(&self.latex_parameters);

                // Target the main TeX file.
                cmd.arg(file.full());

                // Execute the command in the cache directory.
                cmd.current_dir(&self.cache_path);

                // Capture stdout and stderr for storage.
                cmd.stdout(Stdio::piped());
                cmd.stderr(Stdio::piped());

                // Return a clone of the file reference along with its command.
                (file.clone(), cmd)
            })
            .collect();

        // If no commands were created, log a warning to indicate a missing main file.
        if commands.is_empty() {
            warn!("No LaTeX commands to run. Either no main files found or 00readme is empty.");
        }

        // Execute each command, collecting the resulting Output into the `latex_output` map.
        // The map key is the main file; the value is the Result of the command execution.
        self.latex_output = commands
            .into_iter()
            .map(|(main_file, mut cmd)| {
                info!(
                    "Compiling main file (this may take a while): {}",
                    main_file.relative().display()
                );
                let result = cmd.output();
                match &result {
                    Ok(output) => {
                        if output.status.success() {
                            trace!("Compilation successful for {:?}", main_file.relative());
                        } else {
                            warn!(
                                "Compilation failed for {:?} with status: {}:{:?}",
                                main_file.relative(),
                                output.status,
                                String::from_utf8_lossy(&output.stderr)
                            );
                        }
                    }
                    Err(e) => {
                        error!(
                            "Failed to execute compilation for {:?}: {}",
                            main_file.relative(),
                            e
                        );
                    }
                }
                (main_file, result)
            })
            .collect();
    }

    /// Detects BibTeX references that were not captured by the LaTeX recorder.
    ///
    /// The function analyzes the stdout of each successful LaTeX compilation and
    /// extracts referenced .bib files that appear in the log output.
    pub fn determine_latexmk_bib_references(&mut self) {
        use crate::helper::find_referenced_bibs;

        trace!("Detecting BibTeX references from latexmk output");

        self.latexmk_referenced_bibs = self
            .latex_output
            .values()
            .map(|o| o.as_ref()) // keep only successful outputs
            .filter_map(Result::ok_with_warning)
            .map(|o| String::from_utf8_lossy(&o.stdout[..])) // decode stdout as UTF-8 lossily
            .flat_map(|content| find_referenced_bibs(content.as_ref(), &self.cache_path)) // find bib files in the output
            .collect(); // gather into a HashSet

        trace!(
            "Found {} BibTeX file(s) referenced in latexmk output",
            self.latexmk_referenced_bibs.len()
        );
    }

    /// Parses .fls recorder files to collect input and output files used during the build.
    fn process_recorder_files(&mut self) -> Result<()> {
        use crate::helper::parse_fls;
        use glob::glob;

        trace!("Processing .fls recorder files");
        let full_cache_path = self.cache_path.path().canonicalize()?;
        trace!("Cache path: {:?}", full_cache_path);

        // Find all .fls files in the cache directory, parse them, and collect
        // their inputs and outputs into separate HashSets
        let (inputs, outputs) = glob(&format!("{}/*.fls", full_cache_path.to_string_lossy()))
            // Convert any glob errors into an anyhow error for better context
            .map_err(|e| anyhow::anyhow!("Failed to create glob pattern: {}", e))?
            // Ignore any failed glob entries
            .filter_map(Result::ok_with_warning)
            // Parse each .fls file; discard any that fail to parse
            .filter_map(|p| parse_fls(p, &full_cache_path).ok_with_warning())
            // Accumulate the resulting input and output sets
            .fold(
                (HashSet::new(), HashSet::new()),
                |(mut acc_i, mut acc_o), (i, o)| {
                    acc_i.extend(i);
                    acc_o.extend(o);
                    (acc_i, acc_o)
                },
            );

        // Store the collected recorder inputs and outputs on the struct for later use
        self.recorder_inputs = inputs;
        self.recorder_outputs = outputs;

        trace!(
            "Recorder processing complete: {} input files, {} output files",
            self.recorder_inputs.len(),
            self.recorder_outputs.len()
        );

        trace!("Copy and clean phase completed");
        Ok(())
    }

    /// Copies only the files that were actually used during the build into the cleaned output directory.
    ///
    /// Unused files are omitted. Additionally, all `.bbl` files produced by BibTeX are preserved.
    fn copy_and_clean(&mut self) -> Result<()> {
        use std::fs::create_dir_all;

        trace!("Starting copy and clean phase");
        trace!(
            "Initial file list contains {} files",
            self.initial_file_list.len()
        );

        // Gather all files that are required by the LaTeX build:
        // recorder inputs and bib references from latexmk.
        let all_used: HashSet<_> = self
            .recorder_inputs
            .union(&self.latexmk_referenced_bibs)
            .collect();
        trace!("Total files marked as used: {}", all_used.len());

        // Iterate over every file that was initially present in the source tree.
        trace!(
            "Processing {} files from initial list",
            self.initial_file_list.len()
        );

        for parsed in &self.initial_file_list {
            let zzrm_usage = if let Some(zzrm) = &self.zzrm
                && let Some(zzrm_source) = zzrm.sources.get(
                    &parsed
                        .source_file()
                        .relative()
                        .to_string_lossy()
                        .to_string(),
                ) {
                zzrm_source.usage.as_ref()
            } else {
                None
            };

            use std::path::Component;

            let is_anc_file = parsed.source_file().relative().components().next()
                == Some(Component::Normal(OsStr::new("anc")));

            // Only copy files that are actually used in the build.
            // Consider the file usage defined in the 00Readme.
            let _ = match (
                all_used.contains(parsed.source_file()),
                zzrm_usage,
                is_anc_file,
            ) {
                (_, Some(FileUsage::Include), _) => {
                    let target = self.target_path.join(parsed.source_file().relative());
                    // Ensure the parent directory exists before writing.
                    if let Some(parent) = target.parent() {
                        create_dir_all(parent)
                            .context(format!("Could not create directory {}", parent.display()))?;
                    }
                    parsed.copy_raw_to(&target)?;

                    Some(target)
                }
                (_, _, true) => {
                    trace!(
                        "Including file {} as it is an ancillary file",
                        parsed.source_file().relative().display()
                    );
                    let target = self.target_path.join(parsed.source_file().relative());
                    // Ensure the parent directory exists before writing.
                    if let Some(parent) = target.parent() {
                        create_dir_all(parent)
                            .context(format!("Could not create directory {}", parent.display()))?;
                    }
                    parsed.copy_raw_to(&target)?;

                    Some(target)
                }
                (_, Some(FileUsage::TopLevel), _) | (true, _, _) => {
                    let target = self.target_path.join(parsed.source_file().relative());
                    // Ensure the parent directory exists before writing.
                    if let Some(parent) = target.parent() {
                        create_dir_all(parent)
                            .context(format!("Could not create directory {}", parent.display()))?;
                    }
                    parsed.copy_cleaned_to(&target, &mut self.deletion_stats)?;

                    Some(target)
                }
                (_, Some(FileUsage::Ignore), _) => {
                    trace!(
                        "Ignoring file {} due to 00Readme",
                        parsed.source_file().relative().display()
                    );

                    None
                }
                (false, _, _) => {
                    trace!(
                        "File {} is not used in the build",
                        parsed.source_file().relative().display()
                    );
                    self.deletion_stats
                        .files_deleted
                        .push(parsed.source_file().relative().display().to_string());

                    None
                }
            };
        }

        // Copy all .bbl files that were used during the LaTeX build into the cleaned output directory.
        // These files are typically generated by BibTeX and need to be preserved.
        for file in self
            .recorder_inputs
            .iter()
            .filter(|v| matches!(v.extension(), Some(x) if x == "bbl"))
        {
            use std::fs::copy;

            // Determine the destination path for the backup file in the cleaned target directory.
            let target = self.target_path.join(file.relative());
            // Copy the original .bbl file to the target location
            copy(file.full(), &target)
                .context(format!("Could not backup file {}", target.display()))?;
        }

        if !self.cleaner_config.skip_watermark {
            for file in &self.possible_main_files {
                let target = self.target_path.join(file.relative());
                let version = env!("CARGO_PKG_VERSION");
                let watermark = format!(
                    "\n% Processed with alc-ng ({}) (https://github.com/COMSYS/ALC-NG)",
                    version
                );

                let handle = std::fs::OpenOptions::new()
                    .append(true)
                    .create(false)
                    .open(target);

                if let Ok(mut handle) = handle {
                    handle.write_all(watermark.as_bytes())?;
                }

                trace!("Added watermark to {:?}", file.relative());
            }
        }

        Ok(())
    }

    /// Provides access to the map of LaTeX compilation outputs for each main file.
    pub fn latex_output(&self) -> &HashMap<SourceFile, std::io::Result<Output>> {
        &self.latex_output
    }

    /// Returns a reference to the optional parsed `00Readme` file.
    pub fn zzrm(&self) -> &Option<ZeroZeroReadMe> {
        &self.zzrm
    }

    fn scramble_timestamps(&self) -> Result<()> {
        use std::time::{Duration, SystemTime};
        use walkdir::WalkDir;

        #[cfg(target_os = "windows")]
        {
            info!("Scrambling timestamps on windows files is not supported");
            return Ok(());
        }

        let walker = WalkDir::new(&self.target_path).into_iter();

        let ts = SystemTime::now()
            .checked_sub(Duration::from_secs(fastrand::u64(10000000..100000000)))
            .ok_or_else(|| anyhow::anyhow!("Failed to calculate timestamp"))?;

        for fs_item in walker.filter_map(Result::ok_with_warning) {
            let file = File::open(fs_item.path())?;

            #[cfg(target_os = "macos")]
            {
                use std::os::macos::fs::FileTimesExt;
                file.set_times(
                    FileTimes::new()
                        .set_accessed(ts)
                        .set_modified(ts)
                        .set_created(ts),
                )?;
            }

            #[cfg(not(target_os = "macos"))]
            {
                file.set_times(FileTimes::new().set_accessed(ts).set_modified(ts))?;
            }
        }

        Ok(())
    }

    pub fn pack_as_tar(&self) -> Result<()> {
        use std::fs::File;
        use std::io::stdout;
        use tar::Builder;

        trace!("Starting tar packaging");
        let output_file: Box<dyn Write> = match &self.cleaner_config.tar {
            Some(x) if x == "-" => {
                trace!("Writing tar archive to stdout");
                Box::new(GzEncoder::new(stdout(), Compression::default()))
            }
            Some(filename) if filename.ends_with(".tar.gz") => {
                trace!("Writing tar archive to: {}", filename);
                Box::new(GzEncoder::new(
                    File::create(filename)?,
                    Compression::default(),
                ))
            }
            Some(filename) => {
                let filename = format!("{}.tar.gz", filename);
                trace!("Writing tar archive to: {}", filename);
                Box::new(GzEncoder::new(
                    File::create(filename)?,
                    Compression::default(),
                ))
            }
            None => {
                trace!("Tar packaging skipped (no tar option configured)");
                return Ok(());
            }
        };

        let mut tar = Builder::new(output_file);
        // Explicitly do not preserve timestamps and permissions
        tar.mode(tar::HeaderMode::Deterministic);
        // Append target directory
        tar.append_dir_all("", &self.target_path)?;
        trace!("Added target directory to tar archive");
        // Write termination section of archive
        tar.finish()?;

        trace!("Tar packaging completed successfully");
        Ok(())
    }

    fn latexmk_clean(&self) -> Result<()> {
        let mut cmd = Command::new("latexmk");
        cmd.args([
            "-cd",
            "-f",
            "-pdf",
            "-interaction=nonstopmode",
            "-synctex=1",
        ]);
        cmd.arg("-C");
        cmd.current_dir(self.cache_path.path());
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::null());
        cmd.output()?;

        Ok(())
    }

    pub fn compare(&self) -> CompareResult {
        use walkdir::WalkDir;
        let compile_folder = TempDir::with_prefix("alc-ng_")?;

        let walker = WalkDir::new(self.target_path());

        for entry in walker.into_iter().filter_map(Result::ok_with_warning) {
            use std::fs::{copy, create_dir_all};

            let stripped = entry.path().strip_prefix(&self.target_path)?;
            let target = compile_folder.path().join(stripped);

            let metadata = entry.metadata()?;
            if metadata.is_dir() {
                create_dir_all(target)?;
            } else {
                copy(entry.path(), target)?;
            }
        }

        for main_file in self.get_mains() {
            use std::process::{Command, Stdio};

            info!(
                "Compiling cleaned main file for comparison: {}",
                main_file.relative().display()
            );

            let mut cmd = Command::new(&self.latex_cmd);

            // Run in batch mode, record used files, and output PDF.
            cmd.args([
                "-cd",
                "-f",
                "-pdf",
                "-interaction=nonstopmode",
                "-synctex=1",
                "-recorder",
                "-bibtex-",
            ]);

            // Append any additional LaTeX compiler arguments.
            cmd.args(&self.latex_parameters);

            // Target the main TeX file.
            cmd.arg(main_file.relative());

            // Execute the command in the cache directory.
            cmd.current_dir(&compile_folder);

            cmd.stdout(Stdio::null());
            cmd.stderr(Stdio::null());

            // Return a clone of the file reference along with its command.
            if !cmd.status()?.success() {
                warn!(
                    "Failed to compile cleaned main file {}",
                    main_file.relative().display()
                );
            };
        }

        self
            .get_mains()
            .iter()
            .map(|f| {
                let original = f.full().with_extension("pdf");
                let new_version = compile_folder
                    .path()
                    .join(f.relative().with_extension("pdf"));

                if !original.exists() || !new_version.exists() {
                    return Ok((f.relative().display().to_string(), HashMap::new()));
                }
                let r = PixelPerfect::diff(&original, &new_version)?;

                if !original.exists() {
                    warn!("Original version did not compile successfully. Expected file {:?} to exist.", original)
                }

                if !new_version.exists() {
                    warn!("Cleaned version did not compile successfully. Expected file {:?} to exist.", new_version)
                }


                Ok((f.relative().display().to_string(), r))
            })
            .collect()
    }

    /// Executes the full cleaning pipeline.
    ///
    /// This method orchestrates cache creation, main file detection, LaTeX compilation,
    /// recorder parsing, bib reference determination, and file copying/cleaning.
    pub fn run(&mut self) -> Result<()> {
        // Build caches by copying the input files into a temporary directory.
        info!("Cleaning project");
        self.create_caches();

        // Remove temporary files before trying to compile submission
        info!("Cleaning temporary files with latexmk");
        self.latexmk_clean()?;

        // Remove temporary files before trying to compile submission
        self.set_inital_file_list();

        // Detect possible main TeX files by scanning the cached directory.
        self.possible_main_files = match self.cleaner_config.user_provided_main_files.clone() {
            Some(provided) => provided
                .into_iter()
                .map(|p| SourceFile::from_path(&self.cache_path.path().join(p), &self.cache_path))
                .collect::<Result<HashSet<_>, _>>()?,
            None => find_mains(self.cache_path.path())?,
        };
        info!(
            "Found the following main TeX file(s):\n{}",
            self.possible_main_files
                .iter()
                .map(|f| format!(" - {}", f.relative().to_string_lossy()))
                .collect::<Vec<_>>()
                .join("\n")
        );

        // Compile the identified main files using LaTeX.
        self.compile();

        // Process LaTeX recorder files (.fls) to gather input and output files.
        self.process_recorder_files()?;

        // Determine BibTeX references that were not recorded by the LaTeX compiler.
        self.determine_latexmk_bib_references();

        // Copy only the files that were actually used into the cleaned output directory.
        info!("Copying cleaned project");
        self.copy_and_clean()?;

        // Scramble timestamps of newly created files
        info!("Scrambling timestamps");
        self.scramble_timestamps()?;

        // Create a tar archive if set using CONFIG
        self.pack_as_tar()?;

        if self.cleaner_config.strip_exif {
            exif_tool::clean_inplace(
                &self.target_path,
                &self.cleaner_config.exiftool_cmd,
                &self.cleaner_config.exiftool_args,
            )?;
        } else {
            trace!("EXIF stripping skipped (not configured)");
        }

        Ok(())
    }

    pub fn input_path(&self) -> &Path {
        &self.input_path
    }

    pub fn target_path(&self) -> &Path {
        &self.target_path
    }

    pub fn cache_path(&self) -> &Path {
        self.cache_path.path()
    }

    pub fn stats(&self) -> &DeletionStats {
        &self.deletion_stats
    }
}
