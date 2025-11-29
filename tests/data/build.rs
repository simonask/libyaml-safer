#![allow(clippy::uninlined_format_args)]

use anyhow::Result;
use flate2::read::GzDecoder;
use std::fs;
use std::path::Path;
use tar::Archive;

const TAG: &str = "data-2020-02-11";

fn main() {
    let needs_clone = match fs::read_to_string("yaml-test-suite/COMMIT") {
        Err(_) => true,
        Ok(contents) => contents.trim() != TAG,
    };
    if needs_clone {
        download_and_unpack().unwrap();
    }
}

fn download_and_unpack() -> Result<()> {
    let url = format!("https://github.com/yaml/yaml-test-suite/archive/refs/tags/{TAG}.tar.gz");
    let response = minreq::get(&url).send()?;
    let bytes = response.as_bytes();
    let decoder = GzDecoder::new(bytes);
    let mut archive = Archive::new(decoder);
    let prefix = format!("yaml-test-suite-{}", TAG);

    let yaml_test_suite = Path::new("yaml-test-suite");
    if yaml_test_suite.exists() {
        fs::remove_dir_all(yaml_test_suite)?;
    }

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;
        if path == Path::new("pax_global_header") {
            continue;
        }
        let relative = path.strip_prefix(&prefix)?;
        let out = yaml_test_suite.join(relative);
        entry.unpack(&out)?;
    }

    fs::write("yaml-test-suite/COMMIT", TAG)?;
    Ok(())
}
