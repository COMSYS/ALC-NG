use alc_ng::{
    cleaner::{config::CleanerConfig, submission::Submission},
    exif_tool,
    helper::ResultOkWithWarning,
};
use anyhow::anyhow;
use clap::{Parser, ValueHint};
use log::{debug, info, warn};
use std::{
    fs::remove_dir_all,
    path::PathBuf,
    process::ExitCode,
    sync::{Arc, LazyLock},
};
use tabled::{Table, Tabled, settings::Style};
use temp_dir::TempDir;
use walkdir::WalkDir;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Config {
    /// Input path
    #[arg(value_hint = ValueHint::DirPath)]
    pub input_path: PathBuf,
    /// Target path where cleaned files will be written
    #[arg(default_value = "./cleaned", value_hint = ValueHint::DirPath)]
    pub target_path: PathBuf,
    /// LaTeX compiler command to use. `-recorder` and `-interaction=nonstop`
    /// are always set.
    #[arg(long, default_value = "latexmk", value_hint = ValueHint::CommandName)]
    pub latex_cmd: String,
    /// Additional command‑line parameters passed to the LaTeX compiler.
    #[arg(long)]
    pub latex_args: Vec<String>,
    /// If set, enable debug‑level logging.
    #[arg(long, short, default_value_t = false)]
    pub verbose: bool,
    /// Also compile the cleaned folder and compare the cleaned pdf output with the original pdfs.
    #[arg(long, short, default_value_t = false)]
    pub compare: bool,
    /// Continue cleaning despite hitting errors.
    #[arg(long, short, default_value_t = false)]
    pub force: bool,
    /// The command for exiftool.
    #[arg(long, default_value = "exiftool", value_hint = ValueHint::CommandName)]
    pub exiftool_cmd: String,
    /// Additional parameters to pass to exiftool.
    #[arg(long)]
    pub exiftool_args: Vec<String>,
    /// Strip exif data using exiftool.
    #[arg(long, default_value_t = false)]
    pub strip_exif: bool,
    /// Preserve .bib files
    #[arg(long, default_value_t = false)]
    pub keep_bib: bool,
    /// Resize images to preserve space
    #[arg(long, default_value_t = false)]
    pub resize_images: bool,
    /// Size of the output images (in pixels, longest side).
    /// Fine tune this to get as close to 10MB as possible.
    #[arg(long, default_value_t = 512)]
    pub im_size: u32,
    /// Also clean .sty and .cls latex files.
    #[arg(long, default_value_t = false)]
    pub clean_classes: bool,
    /// Ignore the 00readme even though it is defined.
    #[arg(long, default_value_t = false)]
    pub no_zzrm: bool,
    /// Create a tar archive that is ready to be uploaded.
    /// If the value is '-' then the archive is piped to stdout.
    #[arg(long)]
    pub tar: Option<String>,
    #[arg(long)]
    pub skip_watermark: bool,
}

impl Config {
    pub fn to_cleaner_config(&self) -> CleanerConfig {
        CleanerConfig {
            keep_bib: self.keep_bib.clone(),
            resize_images: self.resize_images.clone(),
            im_size: self.im_size.clone(),
            clean_classes: self.clean_classes.clone(),
            no_zzrm: self.no_zzrm.clone(),
            tar: self.tar.clone(),
            exiftool_args: self.exiftool_args.clone(),
            exiftool_cmd: self.exiftool_cmd.clone(),
            force: self.force.clone(),
            strip_exif: self.strip_exif.clone(),
            skip_watermark: self.skip_watermark,
        }
    }

    pub fn debug_print(&self) {
        debug!("Configuration:");
        debug!("  input_path: {:?}", self.input_path);
        debug!("  target_path: {:?}", self.target_path);
        debug!("  latex_cmd: {}", self.latex_cmd);
        debug!("  latex_args: {:?}", self.latex_args);
        debug!("  verbose: {}", self.verbose);
        debug!("  compare: {}", self.compare);
        debug!("  force: {}", self.force);
        debug!("  exiftool_cmd: {}", self.exiftool_cmd);
        debug!("  exiftool_args: {:?}", self.exiftool_args);
        debug!("  strip_exif: {}", self.strip_exif);
        debug!("  keep_bib: {}", self.keep_bib);
        debug!("  resize_images: {}", self.resize_images);
        debug!("  im_size: {}", self.im_size);
        debug!("  clean_classes: {}", self.clean_classes);
        debug!("  no_zzrm: {}", self.no_zzrm);
        debug!("  tar: {:?}", self.tar);
    }
}

/// Global configuration parsed from command‑line arguments.
static CONFIG: LazyLock<Config> = LazyLock::new(|| Config::parse());

/// Ensures that all prerequisites for running the program are satisfied.
/// It verifies that the LaTeX compiler specified by `CONFIG.latex_cmd` can be executed
/// successfully. If the check fails, the program panics with a helpful error message.
/// # Panics
/// Panics if the LaTeX command cannot be executed or returns a non‑zero exit status.
pub fn ensure_requirements() {
    use std::process::{Command, Stdio};

    let mut cmd = Command::new(&CONFIG.latex_cmd);
    cmd.args(&CONFIG.latex_args);
    cmd.arg("--version");
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::null());

    let status = cmd.status().map_or(false, |v| v.success());

    if !status {
        panic!("The latex command is not available. Please check the configuration.");
    }

    if CONFIG.strip_exif {
        exif_tool::ensure_requirements(&CONFIG.exiftool_cmd);
    }
}

fn nested_main() -> anyhow::Result<Vec<String>> {
    if CONFIG.verbose {
        simple_logger::init_with_level(log::Level::Debug)?;
    } else {
        simple_logger::init_with_level(log::Level::Info)?;
    }

    CONFIG.debug_print();

    ensure_requirements();

    if !CONFIG.input_path.exists() {
        return Err(anyhow!("Input path {:?} does not exist", CONFIG.input_path));
    }

    let _ = remove_dir_all(&CONFIG.target_path);

    let mut submission = Submission::new(
        Arc::new(CONFIG.to_cleaner_config()),
        &CONFIG.input_path,
        &CONFIG.target_path,
        &CONFIG.latex_cmd,
        &CONFIG.latex_args,
    )?;
    submission.run()?;

    submission.stats().pretty_print(CONFIG.verbose);

    if CONFIG.compare {
        let compile_folder = TempDir::with_prefix("alc-ng_")?;

        let walker = WalkDir::new(submission.target_path());

        for entry in walker.into_iter().filter_map(Result::ok_with_warning) {
            use std::fs::{copy, create_dir_all};

            let stripped = entry.path().strip_prefix(&CONFIG.target_path)?;
            let target = compile_folder.path().join(stripped);

            let metadata = entry.metadata()?;
            if metadata.is_dir() {
                create_dir_all(target)?;
            } else {
                copy(entry.path(), target)?;
            }
        }

        for main_file in submission.get_mains() {
            use std::process::{Command, Stdio};

            let mut cmd = Command::new(&CONFIG.latex_cmd);

            // Run in batch mode, record used files, and output PDF.
            cmd.args([
                "-cd",
                "-f",
                "-pdf",
                "-interaction=nonstop",
                "-synctex=1",
                "-recorder",
                "-bibtex-",
            ]);

            // Append any additional LaTeX compiler arguments.
            cmd.args(&CONFIG.latex_args);

            // Target the main TeX file.
            cmd.arg(main_file.relative());

            // Execute the command in the cache directory.
            cmd.current_dir(&compile_folder);

            cmd.stdout(Stdio::null());
            cmd.stderr(Stdio::null());

            // Return a clone of the file reference along with its command.
            cmd.status()?;
        }

        #[derive(Tabled)]
        struct ComparisonTable {
            name: String,
            different_pages: String,
            equal: bool,
            successful_compile: bool,
        }

        let pdf_files: Vec<ComparisonTable> = submission
            .compare()?
            .into_iter()
            .map(|(f, res)| {
                if let Some(pages) = res {
                    let different_pages: Vec<String> = pages
                        .iter()
                        .zip(1..)
                        .filter(|(diff, _)| !**diff)
                        .map(|(_, index)| index.to_string())
                        .collect();
                    ComparisonTable {
                        name: f,
                        equal: pages.iter().all(|v| *v),
                        successful_compile: true,
                        different_pages: different_pages.join(", "),
                    }
                } else {
                    ComparisonTable {
                        name: f,
                        different_pages: String::new(),
                        equal: false,
                        successful_compile: false,
                    }
                }
            })
            .collect();

        let mut table = Table::new(pdf_files);
        table.with(Style::modern());

        info!("Comparison of cleaned version:\n{}", table);
    }
    Ok(submission.grammar_errors)
}

/// Main entry point of the program.
/// Initializes logging according to the `verbose` flag, verifies LaTeX command availability,
/// creates a `Submission` instance with the provided paths and command options,
/// runs the submission process, and finally prints a comparison of the processed
/// files to their originals.
/// # Errors
/// Propagates any errors from the `Submission` constructor or `run` method.
fn main() -> ExitCode {
    let grammar_errors = nested_main();

    if let Err(e) = grammar_errors {
        eprintln!("Error: {}", e);
        return ExitCode::FAILURE;
    }

    if let Ok(gn) = grammar_errors
        && !gn.is_empty()
    {
        for e in gn {
            warn!("{}", e);
        }
        ExitCode::from(72)
    } else {
        ExitCode::SUCCESS
    }
}
