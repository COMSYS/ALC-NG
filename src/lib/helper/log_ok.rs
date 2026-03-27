pub trait ResultOkWithWarning<T, E> {
    fn ok_with_warning(self) -> Option<T>;
}

impl<T, E: std::fmt::Debug> ResultOkWithWarning<T, E> for Result<T, E> {
    fn ok_with_warning(self) -> Option<T> {
        match self {
            Ok(val) => Some(val),
            Err(err) => {
                log::warn!("{:?}", err);
                None
            }
        }
    }
}
