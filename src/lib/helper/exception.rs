use anyhow::Result;
use log::warn;

pub fn exception<T>(err: anyhow::Error, otherwise: T, target: &str, force: bool) -> Result<T> {
    if force {
        warn!(target: target, "{}", err);
        warn!(target: target, "You can force the cleaner to continue cleaning using the --force / -f flag");
        Ok(otherwise)
    } else {
        Err(err)
    }
}
