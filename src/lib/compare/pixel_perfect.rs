#![allow(dead_code)]

use std::{path::Path, sync::LazyLock};

use anyhow::{Context, Result};
use image::DynamicImage;
use itertools::Itertools;
use pdfium_render::prelude::{
    PdfPageRenderRotation, PdfRenderConfig, PdfiumError, PdfiumLibraryBindings,
};

use crate::{compare::comparer::Comparer, helper::ResultOkWithWarning as _};

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn get_library() -> std::result::Result<Box<dyn PdfiumLibraryBindings + 'static>, PdfiumError> {
    use std::path::PathBuf;

    use pdfium_render::prelude::*;
    let path = PathBuf::from(std::env::var("PDFIUM_PATH").unwrap_or("./external/pdfium/".into()))
        .join("lib_mac_arm64");

    Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(&path))
        .or_else(|_| Pdfium::bind_to_system_library())
}

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
fn get_library() -> std::result::Result<Box<dyn PdfiumLibraryBindings + 'static>, PdfiumError> {
    use pdfium_render::prelude::*;
    use std::path::PathBuf;

    let path = PathBuf::from(std::env::var("PDFIUM_PATH").unwrap_or("./external/pdfium/".into()))
        .join("lib_linux_arm64");

    Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(&path))
        .or_else(|_| Pdfium::bind_to_system_library())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn get_library() -> std::result::Result<Box<dyn PdfiumLibraryBindings + 'static>, PdfiumError> {
    use pdfium_render::prelude::*;
    use std::path::PathBuf;

    let path = PathBuf::from(std::env::var("PDFIUM_PATH").unwrap_or("./external/pdfium/".into()))
        .join("lib_linux_x86_64");

    Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(&path))
        .or_else(|_| Pdfium::bind_to_system_library())
}

#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
fn get_library() -> std::result::Result<Box<dyn PdfiumLibraryBindings + 'static>, PdfiumError> {
    use pdfium_render::prelude::*;
    use std::path::PathBuf;

    let path = PathBuf::from(std::env::var("PDFIUM_PATH").unwrap_or("./external/pdfium/".into()))
        .join("lib_mac_x86_64");

    Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(&path))
        .or_else(|_| Pdfium::bind_to_system_library())
}

static RENDER_CONFIG: LazyLock<PdfRenderConfig> = LazyLock::new(|| {
    PdfRenderConfig::new()
        .set_target_width(2000)
        .set_maximum_height(2000)
        .rotate_if_landscape(PdfPageRenderRotation::Degrees90, true)
});

pub struct PixelPerfect {}

impl PixelPerfect {
    pub fn pdfimages<S>(src: S) -> Result<Vec<DynamicImage>>
    where
        S: AsRef<Path>,
    {
        use pdfium_render::prelude::*;

        let bindings = get_library().expect("failed to bind to static library");
        let pdfium = Pdfium::new(bindings);

        let document = pdfium.load_pdf_from_file(src.as_ref().to_str().unwrap(), None)?;

        let images = document
            .pages()
            .iter()
            .filter_map(|p| {
                p.render_with_config(&RENDER_CONFIG)
                    .ok_with_warning()
                    .map(|b| b.as_image())
            })
            .collect();

        drop(document);
        drop(pdfium);

        Ok(images)
    }
}

impl Comparer for PixelPerfect {
    fn diff<S: AsRef<Path>, T: AsRef<Path>>(left: S, right: T) -> anyhow::Result<Vec<bool>> {
        let left_images = Self::pdfimages(left).context("Failed to handle left images");
        let right_images = Self::pdfimages(right).context("Failed to handle right images");

        Ok(left_images
            .iter()
            .zip_longest(right_images.iter())
            .map(|v| match v {
                itertools::EitherOrBoth::Both(l, r) => l == r,
                _ => false,
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
