use std::{
    collections::HashMap,
    fs::File,
    io::{BufWriter, Read, Write},
    path::Path,
    sync::{LazyLock, OnceLock},
};

use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use image::DynamicImage;
use itertools::Itertools;
use pdfium_render::prelude::{PdfPageRenderRotation, PdfRenderConfig, Pdfium};
use sha3::{Digest, Sha3_256};
use temp_dir::TempDir;

use crate::{compare::comparer::Comparer, helper::ResultOkWithWarning as _};

static DY_LIB_GZ: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/libpdfium.gz"));
static EXPECTED_CHECKSUM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/libpdfium.sha3"));
static BINDINGS: OnceLock<Pdfium> = OnceLock::new();
static LIB_PATH: LazyLock<TempDir> = LazyLock::new(|| TempDir::with_prefix("alc_ng-").unwrap());

fn set_library() -> anyhow::Result<()> {
    use pdfium_render::prelude::*;

    if BINDINGS.get().is_none() {
        let mut decoder = GzDecoder::new(DY_LIB_GZ);
        // Use .dll extension for Windows support. Does not make any difference on other platforms.
        let lib_path = LIB_PATH.path().join("libpdfium.dll");
        let mut file = BufWriter::new(File::create(&lib_path)?);

        std::io::copy(&mut decoder, &mut file)?;

        // Flush the file to disk before verifying the checksum and drop handle to remove lock on Windows
        file.flush()?;
        drop(file);

        verify_checksum(&lib_path)?;

        let _ = BINDINGS.set(Pdfium::new(
            Pdfium::bind_to_system_library().or_else(|_| Pdfium::bind_to_library(lib_path))?,
        ));
    }

    Ok(())
}

fn verify_checksum(lib_path: &Path) -> Result<()> {
    let mut file = File::open(lib_path)?;
    let mut contents = Vec::new();
    file.read_to_end(&mut contents)?;
    let mut hasher = Sha3_256::new();
    hasher.update(&contents);

    let expected_checksum = EXPECTED_CHECKSUM;
    let actual_checksum = hasher.finalize();

    if &actual_checksum[..] != expected_checksum {
        anyhow::bail!("DLL checksum mismatch!");
    }

    Ok(())
}

static RENDER_CONFIG: LazyLock<PdfRenderConfig> = LazyLock::new(|| {
    PdfRenderConfig::new()
        .set_target_width(3000)
        .set_maximum_height(3000)
        .rotate_if_landscape(PdfPageRenderRotation::Degrees90, true)
});

pub struct PixelPerfect {}

impl PixelPerfect {
    pub fn pdfimages<S>(src: S) -> Result<Vec<DynamicImage>>
    where
        S: AsRef<Path>,
    {
        set_library()?;
        let pdfium = BINDINGS.get().unwrap();

        let document = pdfium.load_pdf_from_file(src.as_ref().to_str().unwrap(), None)?;

        let images = document
            .pages()
            .iter()
            .filter_map(|p| {
                p.render_with_config(&RENDER_CONFIG)
                    .ok_with_warning()
                    .map(|b| b.as_image())
            })
            .collect::<std::result::Result<Vec<_>, _>>()?;

        drop(document);

        Ok(images)
    }
}

impl Comparer for PixelPerfect {
    fn diff<S: AsRef<Path>, T: AsRef<Path>>(
        left: S,
        right: T,
    ) -> anyhow::Result<HashMap<usize, (DynamicImage, DynamicImage)>> {
        let left_images = Self::pdfimages(left).context("Failed to handle left images")?;
        let right_images = Self::pdfimages(right).context("Failed to handle right images")?;

        Ok(left_images
            .into_iter()
            .zip_longest(right_images)
            .enumerate()
            .filter_map(|(i, v)| match v {
                itertools::EitherOrBoth::Both(l, r) => {
                    if l != r {
                        return Some((i, (l, r)));
                    }
                    None
                }
                _ => None,
            })
            .collect())
    }

    fn are_equal<S: AsRef<Path>, T: AsRef<Path>>(left: S, right: T) -> bool {
        let left_images = Self::pdfimages(left).expect("Failed to load left images");
        let right_images = Self::pdfimages(right).expect("Failed to load right images");

        left_images
            .iter()
            .zip(right_images.iter())
            .all(|(left_image, right_image)| left_image == right_image)
    }
}
