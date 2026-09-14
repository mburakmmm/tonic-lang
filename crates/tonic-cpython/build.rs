use std::{env, process::Command};

fn main() {
    println!("cargo:rerun-if-env-changed=PYTHON_CONFIG");
    let program = env::var("PYTHON_CONFIG").unwrap_or_else(|_| "python3-config".into());
    let output = Command::new(&program)
        .args(["--ldflags", "--embed"])
        .output()
        .unwrap_or_else(|error| panic!("failed to run {program}: {error}"));
    assert!(
        output.status.success(),
        "{program} --ldflags --embed failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let flags = String::from_utf8(output.stdout).expect("python3-config emitted non-UTF-8 flags");
    let version = flags
        .split_whitespace()
        .find_map(|word| word.strip_prefix("-lpython"))
        .and_then(|version| {
            let mut parts = version.split('.');
            Some((
                parts.next()?.parse::<u32>().ok()?,
                parts.next()?.parse::<u32>().ok()?,
            ))
        })
        .expect("python3-config embed flags did not identify a libpython version");
    assert!(
        version >= (3, 12),
        "tonic-cpython requires CPython 3.12 or newer, found {}.{}",
        version.0,
        version.1
    );
    let mut words = flags.split_whitespace();
    while let Some(word) = words.next() {
        if let Some(path) = word.strip_prefix("-L") {
            println!("cargo:rustc-link-search=native={path}");
        } else if let Some(library) = word.strip_prefix("-l") {
            println!("cargo:rustc-link-lib={library}");
        } else if word == "-framework" {
            let framework = words.next().expect("-framework requires a name");
            println!("cargo:rustc-link-lib=framework={framework}");
        } else {
            println!("cargo:rustc-link-arg={word}");
        }
    }
}
