use alc_ng::{
    cleaner::{config::CleanerConfig, file_renames::FileRenames, submission::Submission},
    exif_tool,
    helper::{image_diff, parse_hex_color},
};

use clap::{ArgAction, Parser, ValueHint};
use itertools::Itertools;
use log::{LevelFilter, info, trace, warn};
use std::{
    fs::remove_dir_all,
    path::PathBuf,
    sync::{Arc, LazyLock},
};
use temp_dir::TempDir;
use walkdir::WalkDir;

#[derive(Parser, Debug, Clone)]
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
    #[arg(short, long, action = ArgAction::Count)]
    pub verbose: u8,
    /// Also compile the cleaned folder and compare the cleaned pdf output with the original pdfs.
    #[arg(long, short, default_value_t = false)]
    pub compare: bool,
    /// Continue cleaning despite hitting errors and skip all interactive prompts.
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
    /// Rename files in the cleaned project. Spaces in file names are replaced
    /// with underscores (as arXiv does on upload) and references to the
    /// renamed files are updated.
    #[arg(long, default_value_t = false)]
    pub rename_files: bool,
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
            rename_files: self.rename_files,
            exiftool_args: self.exiftool_args.clone(),
            exiftool_cmd: self.exiftool_cmd.clone(),
            force: self.force,
            strip_exif: self.strip_exif,
            skip_watermark: self.skip_watermark,
            user_provided_main_files: self.main_files.clone(),
            verbose: self.verbose != 0,
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
        trace!("  rename_files: {}", self.rename_files);
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

/// Counts the files in `root` whose names would be renamed by
/// [`FileRenames`] (i.e. paths that contain spaces).
fn count_renamable_files(root: &std::path::Path) -> usize {
    let relative_paths: Vec<PathBuf> = WalkDir::new(root)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
        .filter_map(|entry| entry.path().strip_prefix(root).ok().map(PathBuf::from))
        .collect();

    FileRenames::new(relative_paths.iter().map(PathBuf::as_path))
        .iter()
        .count()
}

/// Suggests enabling [`Config::rename_files`] when a tar archive is created
/// and the project contains files that would be renamed.
///
/// The prompt is skipped when renaming is already enabled, when `--force` is
/// set, or when no TTY is attached (CI, piped input).
fn suggest_rename_files(config: &mut Config) -> anyhow::Result<()> {
    if config.tar.is_none() || config.rename_files || config.force {
        return Ok(());
    }

    use std::io::IsTerminal;
    if !std::io::stdin().is_terminal() {
        // No TTY attached (CI, piped input) — keep the default and do not prompt
        return Ok(());
    }

    let renamable = count_renamable_files(&config.input_path);
    if renamable == 0 {
        return Ok(());
    }

    use dialoguer::Confirm;
    let rename = Confirm::new()
        .with_prompt(format!(
            "ArXiv replaces spaces in file names with underscores. {} file(s) in the project would be renamed and their references updated. Do you want to rename them now automatically?",
            renamable
        ))
        .default(true)
        .interact()?;

    if rename {
        config.rename_files = true;
        info!("Enabling file renaming.");
    }

    Ok(())
}

fn main() -> anyhow::Result<(), anyhow::Error> {
    let log_level = match CONFIG.verbose {
        0 => log::LevelFilter::Info,
        1 => log::LevelFilter::Debug,
        _ => log::LevelFilter::Trace,
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

    // Local mutable copy of the configuration so interactive suggestions can adjust it.
    let mut config = (*CONFIG).clone();

    config.debug_print();

    ensure_requirements();

    if !config.input_path.exists() {
        anyhow::bail!("Input path {:?} does not exist", config.input_path);
    }

    suggest_rename_files(&mut config)?;

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
        use std::io::IsTerminal;
        let should_overwrite = if CONFIG.force {
            true
        } else if !std::io::stdin().is_terminal() {
            // No TTY attached (CI, piped input) — auto-proceed
            true
        } else {
            use dialoguer::Confirm;
            Confirm::new()
                .with_prompt(format!(
                    "The target path ({}) already exists, do you want to override its contents?",
                    CONFIG.target_path.display()
                ))
                .interact()?
        };

        if should_overwrite {
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
        Arc::new(config.to_cleaner_config()),
        &config.input_path,
        &config.target_path,
        &config.latex_cmd,
        &config.latex_args,
    )?;
    submission.run()?;

    if CONFIG.compare {
        let compile_folder = TempDir::with_prefix("alc-ng_")?;

        let walker = WalkDir::new(submission.target_path());

        for entry in walker.into_iter().filter_map(|entry| match entry {
            Ok(entry) => Some(entry),
            Err(e) => {
                warn!(
                    "Failed to read directory entry in {:?} while copying to compile folder: {}",
                    CONFIG.target_path, e
                );
                None
            }
        }) {
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
        let mut has_fail = false;

        let comparison = submission.compare()?;

        for (f, res) in comparison.into_iter() {
            let res = match res {
                Some(res) => res,
                None => {
                    has_fail = true;
                    continue;
                }
            };

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
            for (page, imgs) in res.into_iter().sorted_by(|l, r| l.0.cmp(&r.0)) {
                let path = CONFIG.target_path.join(format!("{}_{}.jpg", f, page));

                match imgs {
                    (Some(l), Some(r)) => {
                        image_diff(&l, &r, color)?
                            .save_with_format(&path, image::ImageFormat::Jpeg)?;
                        info!(
                            "Saved image diff for page {} of file {}: {}",
                            page,
                            f,
                            path.display()
                        );
                    }
                    (Some(l), None) => {
                        l.save_with_format(&path, image::ImageFormat::Jpeg)?;
                        info!(
                            "Saved image of original version for page {} of file {} (cleaned did not have this page): {}",
                            page,
                            f,
                            path.display()
                        );
                    }
                    (None, Some(r)) => {
                        r.save_with_format(&path, image::ImageFormat::Jpeg)?;
                        info!(
                            "Saved image of cleaned version for page {} of file {} (original did not have this page): {}",
                            page,
                            f,
                            path.display()
                        );
                    }
                    _ => {}
                }
            }
        }

        if same && !has_fail {
            info!(
                "The cleaned submission has no differences from the source. They are pixel-identical."
            );
        }

        if has_fail {
            anyhow::bail!("At least one main file has problems.");
        }
    }

    submission.stats().pretty_print(CONFIG.verbose != 0);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::count_renamable_files;
    use temp_dir::TempDir;

    #[test]
    fn counts_files_that_would_be_renamed() {
        let dir = TempDir::with_prefix("alc-ng-test-").unwrap();
        for file in ["main.tex", "a b.tex", "sub dir/c.tex", "x y.png", "x_y.png"] {
            let path = dir.path().join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"").unwrap();
        }
        std::fs::create_dir(dir.path().join("empty dir")).unwrap();

        // `a b.tex` and `sub dir/c.tex`: `x y.png` is not renamed as `x_y.png` exists, and
        // directories do not count on their own. Files count whether the build uses them or not.
        assert_eq!(count_renamable_files(dir.path()), 2);
    }
}
