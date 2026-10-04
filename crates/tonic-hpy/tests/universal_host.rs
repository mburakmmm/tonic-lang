#![cfg(any(target_os = "linux", target_os = "macos"))]
#![allow(unsafe_code)]

use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};
use tonic_compiler::compile;
use tonic_hpy::UniversalModule;
use tonic_runtime::{ExecutionMode, Vm};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    directory: PathBuf,
    library: PathBuf,
}

impl Fixture {
    fn compile() -> Self {
        Self::compile_source(SOURCE)
    }

    fn compile_source(contents: &str) -> Self {
        let serial = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "tonic-hpy-h1-module-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir_all(&directory).expect("create fixture directory");
        let source = directory.join("h1demo.c");
        let library = directory.join("h1demo.hpy0.so");
        fs::write(&source, contents).expect("write fixture source");
        let include = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("vendor/hpy-0.9.0/include");

        let mut compiler = Command::new("cc");
        #[cfg(target_os = "macos")]
        compiler.arg("-dynamiclib");
        #[cfg(target_os = "linux")]
        compiler.args(["-shared", "-fPIC"]);
        let output = compiler
            .arg("-std=c11")
            .arg("-Wall")
            .arg("-Wextra")
            .arg("-DHPY_ABI_UNIVERSAL")
            .arg("-I")
            .arg(include)
            .arg("-o")
            .arg(&library)
            .arg(&source)
            .output()
            .expect("run C compiler");
        assert!(
            output.status.success(),
            "HPy fixture compile failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let mut audit = if cfg!(target_os = "macos") {
            let mut command = Command::new("otool");
            command.arg("-L");
            command
        } else {
            let mut command = Command::new("readelf");
            command.arg("-d");
            command
        };
        let audit = audit.arg(&library).output().expect("audit dependencies");
        assert!(audit.status.success(), "dependency audit failed");
        let dependencies = String::from_utf8_lossy(&audit.stdout).to_lowercase();
        assert!(
            !dependencies.contains("libpython"),
            "HPy Universal fixture must not link libpython:\n{dependencies}"
        );
        Self { directory, library }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn module(fixture: &Fixture) -> UniversalModule {
    // SAFETY: this test compiles the source below against the exact vendored
    // HPy 0.9 headers and does not execute untrusted constructors.
    unsafe { UniversalModule::load("h1demo", &fixture.library) }.expect("load HPy module")
}

fn run(module: &UniversalModule, source: &str, mode: ExecutionMode) -> (Vm, String) {
    let mut vm = Vm::new().expect("create VM");
    vm.execution_mode = mode;
    module.register(&mut vm).expect("register HPy module");
    let program = compile(source, "hpy-h1").expect("compile source");
    let mut output = Vec::new();
    vm.run(&program, &mut output).expect("run source");
    (vm, String::from_utf8(output).expect("UTF-8 output"))
}

#[test]
fn official_hpy_headers_execute_constant_fibonacci_unicode_and_handles() {
    let fixture = Fixture::compile();
    let module = module(&fixture);
    assert_eq!(module.module_name(), "h1demo");
    assert_eq!(
        module.method_names(),
        [
            "constant",
            "fib",
            "greet",
            "utf8_size",
            "double",
            "module_self",
            "recover",
            "save",
            "load_saved",
            "use_after_close",
            "null_without_error",
            "result_with_error",
        ]
    );

    let source = concat!(
        "import h1demo\n",
        "print(h1demo.constant())\n",
        "print(h1demo.fib(40))\n",
        "print(h1demo.greet())\n",
        "print(h1demo.utf8_size('hé'))\n",
        "print(h1demo.double(21))\n",
        "print(h1demo.module_self())\n",
        "print(h1demo.recover('not-an-int'))\n",
    );
    for mode in [ExecutionMode::Interpreter, ExecutionMode::Jit] {
        let (vm, output) = run(&module, source, mode);
        assert_eq!(output, "42\n102334155\ntonic-hpy\n3\n42\n<module>\n7\n");
        assert_eq!(vm.active_handles(), 0);
    }
}

#[test]
fn hpy_exception_state_and_failure_cleanup_are_guest_visible() {
    let fixture = Fixture::compile();
    let module = module(&fixture);
    for (expression, kind) in [
        ("h1demo.fib(-1)", "ValueError"),
        ("h1demo.use_after_close(1)", "HandleError"),
        ("h1demo.null_without_error()", "SystemError"),
        ("h1demo.result_with_error()", "SystemError"),
    ] {
        let mut vm = Vm::new().unwrap();
        module.register(&mut vm).unwrap();
        let program = compile(&format!("import h1demo\n{expression}"), "hpy-errors").unwrap();
        let error = vm.run(&program, &mut Vec::new()).unwrap_err();
        assert_eq!(error.kind, kind, "expression: {expression}");
        assert_eq!(vm.active_handles(), 0, "expression: {expression}");
    }
}

#[test]
fn saved_local_is_rejected_across_calls_and_runtimes() {
    let fixture = Fixture::compile();
    let first = module(&fixture);
    let second = module(&fixture);

    let mut first_vm = Vm::new().unwrap();
    first.register(&mut first_vm).unwrap();
    let program = compile(
        "import h1demo\nh1demo.save(42)\nh1demo.load_saved()",
        "hpy-stale",
    )
    .unwrap();
    let error = first_vm.run(&program, &mut Vec::new()).unwrap_err();
    assert_eq!(error.kind, "HandleError");
    assert!(error.message.contains("stale or cross-runtime"));
    assert_eq!(first_vm.active_handles(), 0);

    let save = compile("import h1demo\nh1demo.save(99)", "hpy-save").unwrap();
    first_vm.run(&save, &mut Vec::new()).unwrap();
    assert_eq!(first_vm.active_handles(), 0);

    let mut second_vm = Vm::new().unwrap();
    second.register(&mut second_vm).unwrap();
    let program = compile("import h1demo\nh1demo.load_saved()", "hpy-cross-runtime").unwrap();
    let error = second_vm.run(&program, &mut Vec::new()).unwrap_err();
    assert_eq!(error.kind, "HandleError");
    assert!(error.message.contains("stale or cross-runtime"));
    assert_eq!(second_vm.active_handles(), 0);
}

#[test]
fn unsupported_h2_method_signature_fails_closed_during_load() {
    let fixture = Fixture::compile_source(UNSUPPORTED_SOURCE);
    // SAFETY: this test compiles the source below against the pinned headers;
    // the host rejects its unsupported method before it can execute.
    let error = unsafe { UniversalModule::load("h1demo", &fixture.library) }.unwrap_err();
    assert!(error.to_string().contains("H2 argument signature"));
}

const SOURCE: &str = r#"
#include <hpy.h>

HPyDef_METH(constant, "constant", HPyFunc_NOARGS)
static HPy constant_impl(HPyContext *ctx, HPy self) {
    (void)self;
    return HPyLong_FromInt64_t(ctx, 42);
}

HPyDef_METH(fib, "fib", HPyFunc_O)
static HPy fib_impl(HPyContext *ctx, HPy self, HPy arg) {
    (void)self;
    int64_t n = HPyLong_AsInt64_t(ctx, arg);
    if (HPyErr_Occurred(ctx)) return HPy_NULL;
    if (n < 0) return HPyErr_SetString(ctx, ctx->h_ValueError, "fib requires n >= 0");
    int64_t a = 0;
    int64_t b = 1;
    for (int64_t i = 0; i < n; i++) {
        int64_t next = a + b;
        a = b;
        b = next;
    }
    return HPyLong_FromInt64_t(ctx, a);
}

HPyDef_METH(greet, "greet", HPyFunc_NOARGS)
static HPy greet_impl(HPyContext *ctx, HPy self) {
    (void)self;
    return HPyUnicode_FromString(ctx, "tonic-hpy");
}

HPyDef_METH(utf8_size, "utf8_size", HPyFunc_O)
static HPy utf8_size_impl(HPyContext *ctx, HPy self, HPy arg) {
    (void)self;
    HPy_ssize_t size = 0;
    if (HPyUnicode_AsUTF8AndSize(ctx, arg, &size) == NULL) return HPy_NULL;
    return HPyLong_FromInt64_t(ctx, (int64_t)size);
}

HPyDef_METH(double_value, "double", HPyFunc_O)
static HPy double_value_impl(HPyContext *ctx, HPy self, HPy arg) {
    (void)self;
    HPy duplicate = HPy_Dup(ctx, arg);
    if (HPy_IsNull(duplicate)) return HPy_NULL;
    int64_t value = HPyLong_AsInt64_t(ctx, duplicate);
    HPy_Close(ctx, duplicate);
    if (HPyErr_Occurred(ctx)) return HPy_NULL;
    return HPyLong_FromInt64_t(ctx, value * 2);
}

HPyDef_METH(module_self, "module_self", HPyFunc_NOARGS)
static HPy module_self_impl(HPyContext *ctx, HPy self) {
    return HPy_Dup(ctx, self);
}

HPyDef_METH(recover, "recover", HPyFunc_O)
static HPy recover_impl(HPyContext *ctx, HPy self, HPy arg) {
    (void)self;
    (void)HPyLong_AsInt64_t(ctx, arg);
    if (HPyErr_Occurred(ctx)) HPyErr_Clear(ctx);
    return HPyLong_FromInt64_t(ctx, 7);
}

static HPy saved = {0};

HPyDef_METH(save, "save", HPyFunc_O)
static HPy save_impl(HPyContext *ctx, HPy self, HPy arg) {
    (void)self;
    saved = HPy_Dup(ctx, arg);
    if (HPy_IsNull(saved)) return HPy_NULL;
    return HPy_Dup(ctx, ctx->h_None);
}

HPyDef_METH(load_saved, "load_saved", HPyFunc_NOARGS)
static HPy load_saved_impl(HPyContext *ctx, HPy self) {
    (void)self;
    int64_t value = HPyLong_AsInt64_t(ctx, saved);
    if (HPyErr_Occurred(ctx)) return HPy_NULL;
    return HPyLong_FromInt64_t(ctx, value);
}

HPyDef_METH(use_after_close, "use_after_close", HPyFunc_O)
static HPy use_after_close_impl(HPyContext *ctx, HPy self, HPy arg) {
    (void)self;
    HPy duplicate = HPy_Dup(ctx, arg);
    if (HPy_IsNull(duplicate)) return HPy_NULL;
    HPy_Close(ctx, duplicate);
    (void)HPyLong_AsInt64_t(ctx, duplicate);
    return HPy_NULL;
}

HPyDef_METH(null_without_error, "null_without_error", HPyFunc_NOARGS)
static HPy null_without_error_impl(HPyContext *ctx, HPy self) {
    (void)ctx;
    (void)self;
    return HPy_NULL;
}

HPyDef_METH(result_with_error, "result_with_error", HPyFunc_NOARGS)
static HPy result_with_error_impl(HPyContext *ctx, HPy self) {
    (void)self;
    HPyErr_SetString(ctx, ctx->h_ValueError, "bad extension result");
    return HPyLong_FromInt64_t(ctx, 1);
}

static HPyDef *module_defines[] = {
    &constant,
    &fib,
    &greet,
    &utf8_size,
    &double_value,
    &module_self,
    &recover,
    &save,
    &load_saved,
    &use_after_close,
    &null_without_error,
    &result_with_error,
    NULL,
};

static HPyModuleDef module_definition = {
    .doc = "Tonic H1 integration fixture",
    .size = 0,
    .legacy_methods = NULL,
    .defines = module_defines,
    .globals = NULL,
};

HPy_MODINIT(h1demo, module_definition)
"#;

const UNSUPPORTED_SOURCE: &str = r#"
#include <hpy.h>

HPyDef_METH(varargs, "varargs", HPyFunc_VARARGS)
static HPy varargs_impl(HPyContext *ctx, HPy self, const HPy *args, size_t nargs) {
    (void)self;
    (void)args;
    return HPyLong_FromInt64_t(ctx, (int64_t)nargs);
}

static HPyDef *module_defines[] = { &varargs, NULL };
static HPyModuleDef module_definition = {
    .doc = "unsupported H2 signature fixture",
    .size = 0,
    .legacy_methods = NULL,
    .defines = module_defines,
    .globals = NULL,
};

HPy_MODINIT(h1demo, module_definition)
"#;
