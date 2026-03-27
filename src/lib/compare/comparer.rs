#![allow(dead_code)]

use std::path::Path;

pub trait Comparer {
    fn diff<S: AsRef<Path>, T: AsRef<Path>>(left: S, right: T) -> anyhow::Result<Vec<bool>>;

    fn are_equal<S: AsRef<Path>, T: AsRef<Path>>(left: S, right: T) -> bool;
}
