#![allow(dead_code)]

use std::{collections::HashMap, path::Path};

use image::DynamicImage;

/// Differing pages by page number, with the rendered left and right page (if present).
pub type PageDiffs = HashMap<usize, (Option<DynamicImage>, Option<DynamicImage>)>;

pub trait Comparer {
    fn diff<S: AsRef<Path>, T: AsRef<Path>>(left: S, right: T) -> anyhow::Result<PageDiffs>;

    fn are_equal<S: AsRef<Path>, T: AsRef<Path>>(left: S, right: T) -> bool;
}
