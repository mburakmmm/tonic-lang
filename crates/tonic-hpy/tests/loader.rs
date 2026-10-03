#![cfg(any(target_os = "linux", target_os = "macos"))]
#![allow(unsafe_code)]

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};
use tonic_hpy::{LoadError, PinnedUniversalModule};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    directory: PathBuf,
    library: PathBuf,
}

impl Fixture {
    fn compile(module: &str, body: &str) -> Self {
        let serial = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let directory =
            std::env::temp_dir().join(format!("tonic-hpy-loader-{}-{serial}", std::process::id()));
        fs::create_dir_all(&directory).expect("create fixture directory");
        let source = directory.join(format!("{module}.c"));
        let library = directory.join(format!("{module}.hpy0.so"));
        fs::write(&source, body).expect("write fixture source");

        let mut compiler = Command::new("cc");
        #[cfg(target_os = "macos")]
        compiler.arg("-dynamiclib");
        #[cfg(target_os = "linux")]
        compiler.args(["-shared", "-fPIC"]);
        let output = compiler
            .arg("-std=c11")
            .arg("-Wall")
            .arg("-Wextra")
            .arg("-Werror")
            .arg("-o")
            .arg(&library)
            .arg(&source)
            .output()
            .expect("run C compiler");
        assert!(
            output.status.success(),
            "fixture compile failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        Self { directory, library }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn module_source(module: &str, major: u32, minor: u32, module_pointer: &str) -> String {
    format!(
        r#"
#include <stdint.h>

uint32_t get_required_hpy_major_version_{module}(void) {{ return {major}; }}
uint32_t get_required_hpy_minor_version_{module}(void) {{ return {minor}; }}
void HPyInitGlobalContext_{module}(void *ctx) {{ (void)ctx; }}
void *HPyInit_{module}(void) {{ {module_pointer} }}
"#
    )
}

unsafe fn load(module: &str, path: &Path) -> Result<PinnedUniversalModule, LoadError> {
    // SAFETY: every caller passes a C fixture compiled in this test process with
    // the exact four function signatures required by `PinnedUniversalModule`.
    unsafe { PinnedUniversalModule::load(module, path) }
}

#[test]
fn loads_and_validates_the_pinned_universal_contract() {
    let fixture = Fixture::compile(
        "constant",
        &module_source("constant", 0, 0, "static int def; return &def;"),
    );
    // SAFETY: `fixture` was compiled above with the exact expected ABI.
    let module = unsafe { load("constant", &fixture.library) }.expect("load module");

    assert_eq!(module.module_name(), "constant");
    assert_eq!(module.path(), fixture.library);
    assert_eq!(module.required_abi(), (0, 0));
    assert!(module.has_context_initializer());
    assert!(module.is_pinned());
    // SAFETY: the test fixture's initializer has the promised signature and
    // returns a process-static non-null address.
    assert!(unsafe { module.module_definition() }.is_ok());
}

#[test]
fn rejects_invalid_name_and_filename_before_opening() {
    let missing = Path::new("not-the-module.hpy0.so");
    // SAFETY: validation fails before any native library is opened.
    let name_error = unsafe { load("bad-name", missing) }.unwrap_err();
    assert!(matches!(name_error, LoadError::ModuleName { .. }));

    // SAFETY: filename validation fails before any native library is opened.
    let filename_error = unsafe { load("constant", missing) }.unwrap_err();
    assert_eq!(
        filename_error,
        LoadError::Filename {
            expected: "constant.hpy0.so".to_owned(),
            actual: "not-the-module.hpy0.so".to_owned(),
        }
    );
}

#[test]
fn rejects_incompatible_abi() {
    let fixture = Fixture::compile(
        "wrongabi",
        &module_source("wrongabi", 1, 0, "static int def; return &def;"),
    );
    // SAFETY: the fixture uses the promised C signatures; its returned version
    // deliberately exercises the ordinary fail-closed ABI check.
    let error = unsafe { load("wrongabi", &fixture.library) }.unwrap_err();
    assert_eq!(
        error,
        LoadError::AbiMismatch {
            required_major: 1,
            required_minor: 0,
            supported_major: 0,
            supported_minor: 0,
        }
    );
}

#[test]
fn reports_the_exact_missing_symbol() {
    let source = r#"
#include <stdint.h>
uint32_t get_required_hpy_major_version_missing(void) { return 0; }
uint32_t get_required_hpy_minor_version_missing(void) { return 0; }
void HPyInitGlobalContext_missing(void *ctx) { (void)ctx; }
"#;
    let fixture = Fixture::compile("missing", source);
    // SAFETY: present fixture symbols have the promised signatures; the absent
    // symbol is diagnosed without being called.
    let error = unsafe { load("missing", &fixture.library) }.unwrap_err();
    assert!(matches!(
        error,
        LoadError::MissingSymbol { ref symbol, .. } if symbol == "HPyInit_missing"
    ));
}

#[test]
fn rejects_a_null_module_definition() {
    let fixture = Fixture::compile("nulldef", &module_source("nulldef", 0, 0, "return 0;"));
    // SAFETY: `fixture` was compiled above with the exact expected ABI.
    let module = unsafe { load("nulldef", &fixture.library) }.expect("load module");
    // SAFETY: the fixture's module initializer has the promised signature and
    // intentionally returns null to exercise validation.
    assert_eq!(
        unsafe { module.module_definition() },
        Err(LoadError::NullModuleDefinition)
    );
}

#[test]
fn dropping_a_loaded_module_keeps_the_library_mapped() {
    let serial = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    let marker_directory = std::env::temp_dir().join(format!(
        "tonic-hpy-unload-marker-dir-{}-{serial}",
        std::process::id()
    ));
    fs::create_dir_all(&marker_directory).expect("create marker directory");
    let marker = marker_directory.join("unloaded");
    let marker_literal = marker
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let source = format!(
        r#"
#include <stdint.h>
#include <stdio.h>

uint32_t get_required_hpy_major_version_pinned(void) {{ return 0; }}
uint32_t get_required_hpy_minor_version_pinned(void) {{ return 0; }}
void HPyInitGlobalContext_pinned(void *ctx) {{ (void)ctx; }}
void *HPyInit_pinned(void) {{ static int def; return &def; }}
__attribute__((destructor)) static void record_unload(void) {{
    FILE *file = fopen("{marker_literal}", "w");
    if (file != NULL) {{ fclose(file); }}
}}
"#
    );
    let fixture = Fixture::compile("pinned", &source);
    let _ = fs::remove_file(&marker);
    // SAFETY: `fixture` was compiled above with the exact expected ABI.
    let module = unsafe { load("pinned", &fixture.library) }.expect("load module");
    drop(module);

    assert!(
        !marker.exists(),
        "dropping the loader must not run the library destructor"
    );
    fs::remove_dir_all(marker_directory).expect("remove marker directory");
}
