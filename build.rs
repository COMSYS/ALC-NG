use flate2::Compression;
use flate2::write::GzEncoder;
use sha3::{Digest, Sha3_256};
use std::env;
use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;

fn main() {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();

    let (lib_name, src_dir) = match (target_os.as_str(), target_arch.as_str()) {
        ("macos", "aarch64") => ("libpdfium.dylib", "lib_mac_arm64"),
        ("macos", "x86_64") => ("libpdfium.dylib", "lib_mac_x86_64"),
        ("linux", "aarch64") => ("libpdfium.so", "lib_linux_arm64"),
        ("linux", "x86_64") => ("libpdfium.so", "lib_linux_x86_64"),
        ("windows", "x86_64") => ("pdfium.dll", "lib_windows_x86_64"),
        _ => {
            panic!(
                "Unsupported target: {}-{}",
                env::var("CARGO_CFG_TARGET_OS").unwrap_or_default(),
                env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default()
            );
        }
    };

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let src_path = manifest_dir
        .join("external")
        .join("pdfium")
        .join(src_dir)
        .join(lib_name);
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    let mut src_file = File::open(&src_path).unwrap_or_else(|e| {
        panic!("Failed to open {}: {}", src_path.display(), e);
    });

    let mut src_data = Vec::new();
    src_file.read_to_end(&mut src_data).unwrap_or_else(|e| {
        panic!("Failed to read {}: {}", src_path.display(), e);
    });

    let dst_path = out_dir.join("libpdfium.gz");
    let mut dst_file = File::create(&dst_path).unwrap_or_else(|e| {
        panic!("Failed to create {}: {}", dst_path.display(), e);
    });

    let mut encoder = GzEncoder::new(&mut dst_file, Compression::default());
    encoder.write_all(&src_data).unwrap_or_else(|e| {
        panic!("Failed to compress {}: {}", src_path.display(), e);
    });
    encoder.finish().unwrap_or_else(|e| {
        panic!("Failed to finalize compression: {}", e);
    });

    let sha3_result = Sha3_256::digest(&src_data);
    let checksum_path = out_dir.join("libpdfium.sha3");
    let mut checksum_file = File::create(&checksum_path).unwrap_or_else(|e| {
        panic!("Failed to create {}: {}", checksum_path.display(), e);
    });
    checksum_file.write_all(&sha3_result).unwrap_or_else(|e| {
        panic!(
            "Failed to write checksum to {}: {}",
            checksum_path.display(),
            e
        );
    });

    println!("cargo:rerun-if-changed=external/pdfium/{src_dir}/{lib_name}");
}
