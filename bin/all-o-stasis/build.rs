fn main() {
    #[allow(clippy::expect_used)] // build-time failures are fine
    built::write_built_file().expect("Failed to acquire build-time information");
}
