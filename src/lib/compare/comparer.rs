#![allow(dead_code)]

use std::{collections::HashMap, path::Path};

use image::DynamicImage;

pub trait Comparer {
    fn diff<S: AsRef<Path>, T: AsRef<Path>>(
        left: S,
        right: T,
    ) -> anyhow::Result<HashMap<usize, (DynamicImage, DynamicImage)>>;

    fn are_equal<S: AsRef<Path>, T: AsRef<Path>>(left: S, right: T) -> bool;
}
