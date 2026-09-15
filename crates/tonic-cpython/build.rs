use std::{env, path::PathBuf, process::Command};

fn command_output(program: &str, arguments: &[&str]) -> String {
    let output = Command::new(program)
        .args(arguments)
        .output()
        .unwrap_or_else(|error| panic!("failed to run {program}: {error}"));
    assert!(
        output.status.success(),
        "{program} {} failed: {}",
        arguments.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("Python configuration emitted non-UTF-8 flags")
}

fn build_refcount_shim(python_config: &str) {
    let output_directory = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set"));
    let object = output_directory.join("refcount_shim.o");
    let archive = output_directory.join("libtonic_cpython_refcount.a");
    let compiler = env::var("CC").unwrap_or_else(|_| "cc".into());
    let archiver = env::var("AR").unwrap_or_else(|_| "ar".into());
    let include_flags = command_output(python_config, &["--includes"]);
    let mut compile = Command::new(&compiler);
    compile
        .arg("-std=c11")
        .arg("-fPIC")
        .arg("-c")
        .arg("src/refcount_shim.c")
        .arg("-o")
        .arg(&object);
    compile.args(include_flags.split_whitespace());
    let status = compile
        .status()
        .unwrap_or_else(|error| panic!("failed to run {compiler}: {error}"));
    assert!(status.success(), "failed to compile CPython refcount shim");
    let status = Command::new(&archiver)
        .arg("crus")
        .arg(&archive)
        .arg(&object)
        .status()
        .unwrap_or_else(|error| panic!("failed to run {archiver}: {error}"));
    assert!(status.success(), "failed to archive CPython refcount shim");
    println!(
        "cargo:rustc-link-search=native={}",
        output_directory.display()
    );
    println!("cargo:rustc-link-lib=static=tonic_cpython_refcount");
}

fn main() {
    println!("cargo:rerun-if-env-changed=PYTHON_CONFIG");
    println!("cargo:rerun-if-env-changed=CC");
    println!("cargo:rerun-if-env-changed=AR");
    println!("cargo:rerun-if-changed=src/refcount_shim.c");
    let program = env::var("PYTHON_CONFIG").unwrap_or_else(|_| "python3-config".into());
    let flags = command_output(&program, &["--ldflags", "--embed"]);
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
    build_refcount_shim(&program);
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
