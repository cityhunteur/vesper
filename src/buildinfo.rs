pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const COMMIT: &str = match option_env!("VESPER_GIT_COMMIT") {
    Some(value) => value,
    None => "unknown",
};
pub const BUILD_DATE: &str = match option_env!("VESPER_BUILD_DATE") {
    Some(value) => value,
    None => "unknown",
};
