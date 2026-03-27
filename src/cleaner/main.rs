use alc_ng::{
    cleaner::{config::CleanerConfig, submission::Submission},
    exif_tool,
    helper::{ResultOkWithWarning, image_diff, parse_hex_color},
};

use clap::{Parser, ValueHint};
use itertools::Itertools;
use log::{LevelFilter, info, trace};
use std::{
    fs::remove_dir_all,
    io::IsTerminal,
    path::PathBuf,
    sync::{Arc, LazyLock},
};
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
    /// LaTeX compiler command to use. `-recorder` and `-interaction=nonstopmode`
    /// are always set.
    #[arg(long, default_value = "latexmk", value_hint = ValueHint::CommandName)]
    pub latex_cmd: String,
    /// Additional command‑line parameters passed to the LaTeX compiler.
    #[arg(long)]
    pub latex_args: Vec<String>,
    /// If set, enable debug‑level logging.
    #[arg(long, short, default_value_t = false)]
    pub verbose: bool,
    #[arg(long = "vv", default_value_t = false)]
    pub debug: bool,
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
    /// Do not add a watermark at the end of main tex files.
    #[arg(long)]
    pub skip_watermark: bool,
    /// Paths to main tex files, relative to working directory. This can usually be inferred automatically. Only use this flag if the cleaner does not automatically detect the main files correctly.
    #[arg(long, short, value_hint = ValueHint::FilePath)]
    pub main_files: Option<Vec<PathBuf>>,
    /// Color for changed pixels in diff images (hex format, e.g., ff0000 for red, 00ff00 for green)
    #[arg(long, default_value = "ff0000")]
    pub diff_color: String,
}

impl Config {
    pub fn to_cleaner_config(&self) -> CleanerConfig {
        CleanerConfig {
            keep_bib: self.keep_bib,
            resize_images: self.resize_images,
            im_size: self.im_size,
            clean_classes: self.clean_classes,
            no_zzrm: self.no_zzrm,
            tar: self.tar.clone(),
            exiftool_args: self.exiftool_args.clone(),
            exiftool_cmd: self.exiftool_cmd.clone(),
            force: self.force,
            strip_exif: self.strip_exif,
            skip_watermark: self.skip_watermark,
            user_provided_main_files: self.main_files.clone(),
        }
    }

    pub fn debug_print(&self) {
        trace!("Configuration:");
        trace!("  input_path: {:?}", self.input_path);
        trace!("  target_path: {:?}", self.target_path);
        trace!("  latex_cmd: {}", self.latex_cmd);
        trace!("  latex_args: {:?}", self.latex_args);
        trace!("  verbose: {}", self.verbose);
        trace!("  compare: {}", self.compare);
        trace!("  force: {}", self.force);
        trace!("  exiftool_cmd: {}", self.exiftool_cmd);
        trace!("  exiftool_args: {:?}", self.exiftool_args);
        trace!("  strip_exif: {}", self.strip_exif);
        trace!("  keep_bib: {}", self.keep_bib);
        trace!("  resize_images: {}", self.resize_images);
        trace!("  im_size: {}", self.im_size);
        trace!("  clean_classes: {}", self.clean_classes);
        trace!("  no_zzrm: {}", self.no_zzrm);
        trace!("  tar: {:?}", self.tar);
        trace!("  skip_watermark: {}", self.skip_watermark);
        trace!("  main_files: {:?}", self.main_files);
    }
}

/// Global configuration parsed from command‑line arguments.
static CONFIG: LazyLock<Config> = LazyLock::new(Config::parse);

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

    let status = cmd.status().is_ok_and(|v| v.success());

    if !status {
        panic!("The latex command is not available. Please check the configuration.");
    }

    if CONFIG.strip_exif {
        exif_tool::ensure_requirements(&CONFIG.exiftool_cmd);
    }
}

fn main() -> anyhow::Result<(), anyhow::Error> {
    let log_level = match (CONFIG.debug, CONFIG.verbose) {
        (true, _) => log::LevelFilter::Trace,
        (false, true) => log::LevelFilter::Debug,
        (false, false) => log::LevelFilter::Info,
    };

    env_logger::builder()
        .format(move |buf, record| {
            use std::io::Write;
            if log_level <= LevelFilter::Info {
                writeln!(buf, "{}", record.args())
            } else {
                writeln!(buf, "{}: {}", record.level(), record.args())
            }
        })
        .filter_level(log_level)
        .init();

    CONFIG.debug_print();

    ensure_requirements();

    if !CONFIG.input_path.exists() {
        anyhow::bail!("Input path {:?} does not exist", CONFIG.input_path);
    }

    if let Some(main_files) = CONFIG.main_files.as_ref() {
        if main_files.is_empty() {
            anyhow::bail!("No main files specified");
        }

        for main_file in main_files {
            if !CONFIG.input_path.join(main_file).exists() {
                anyhow::bail!("Main file {:?} does not exist", main_file);
            }
        }

        info!("Using main file override.");
    }

    if CONFIG.target_path.exists() {
        use dialoguer::Confirm;
        if Confirm::new()
            .with_prompt("The target path already exists, do you want to override it?")
            .interact()?
            || !std::io::stdin().is_terminal()
        {
            info!(
                "Removing existing target path {}",
                CONFIG.target_path.display()
            );
            let _ = remove_dir_all(&CONFIG.target_path);
        } else {
            anyhow::bail!("Target path already exists, aborting!");
        }
    }

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

        let mut same = true;

        for (f, res) in submission.compare()?.into_iter() {
            if res.is_empty() {
                continue;
            }

            same = false;

            info!(
                "File {} has differing pages: {}",
                f,
                res.keys()
                    .sorted()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );

            let color = parse_hex_color(&CONFIG.diff_color)?;
            for (page, (l, r)) in res {
                let path = CONFIG.target_path.join(format!("{}_{}.jpg", f, page));
                image_diff(&l, &r, color)?.save_with_format(&path, image::ImageFormat::Jpeg)?;

                info!(
                    "Saved image diff for page {} of file {}: {}",
                    page,
                    f,
                    path.display()
                );
            }
        }

        if same {
            info!(
                "The cleaned submission has no differences from the source. They are pixel-identical."
            );
        }
    }

    Ok(())
}
