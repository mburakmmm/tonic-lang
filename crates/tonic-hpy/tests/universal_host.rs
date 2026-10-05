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
    run_with_gc(module, source, mode, None)
}

fn run_with_gc(
    module: &UniversalModule,
    source: &str,
    mode: ExecutionMode,
    gc_interval: Option<u64>,
) -> (Vm, String) {
    let mut vm = Vm::new().expect("create VM");
    vm.execution_mode = mode;
    vm.gc_interval = gc_interval;
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
            "make_list",
            "make_tuple",
            "make_tuple_builder",
            "make_dict",
            "kind_code",
            "cancel_builder",
            "incomplete_builder",
            "leak_builder",
            "tracker_sum",
            "tracker_forget",
            "tracker_closed_handle",
            "stale_tracker",
            "leak_tracker",
            "negative_tracker",
            "argument_count",
            "argument_shape",
            "attr_roundtrip",
            "list_roundtrip",
            "dict_roundtrip",
            "call_positional",
            "call_tuple_dict",
            "call_keywords",
            "call_method",
            "scalar_values",
            "scalar_conversions",
            "as_u64",
            "as_i32",
            "as_float",
            "set_object",
            "exception_matches",
            "no_memory",
            "global_store",
            "global_load",
            "global_is_null",
            "global_clear",
            "field_store",
            "field_load",
            "field_is_null",
            "field_clear",
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
        "print(h1demo.make_list())\n",
        "print(h1demo.make_tuple())\n",
        "print(h1demo.make_tuple_builder())\n",
        "print(h1demo.make_dict())\n",
        "print(h1demo.kind_code(h1demo.make_list()))\n",
        "print(h1demo.kind_code(h1demo.make_tuple()))\n",
        "print(h1demo.kind_code(h1demo.make_dict()))\n",
        "print(h1demo.cancel_builder())\n",
    );
    for mode in [ExecutionMode::Interpreter, ExecutionMode::Jit] {
        let (vm, output) = run(&module, source, mode);
        assert_eq!(
            output,
            concat!(
                "42\n102334155\ntonic-hpy\n3\n42\n<module>\n7\n",
                "[1, 2, 3, 4]\n(5, 6)\n(7, 8)\n{}\n1\n2\n4\n9\n"
            )
        );
        assert_eq!(vm.active_handles(), 0);
    }
}

#[test]
fn h3_trackers_close_or_release_owned_handles_deterministically() {
    let fixture = Fixture::compile();
    let module = module(&fixture);
    let source = concat!(
        "import h1demo\n",
        "print(h1demo.tracker_sum())\n",
        "print(h1demo.tracker_forget())\n",
    );
    for mode in [ExecutionMode::Interpreter, ExecutionMode::Jit] {
        for gc_interval in [None, Some(1)] {
            let (vm, output) = run_with_gc(&module, source, mode, gc_interval);
            assert_eq!(output, "42\n43\n", "mode={mode:?}, gc={gc_interval:?}");
            assert_eq!(vm.active_handles(), 0, "mode={mode:?}, gc={gc_interval:?}");
        }
    }
}

#[test]
fn h2_scalar_and_exception_surface_handles_bigints_floats_and_forced_gc() {
    let fixture = Fixture::compile();
    let module = module(&fixture);
    let source = concat!(
        "import h1demo\n",
        "print(h1demo.scalar_values())\n",
        "print(h1demo.scalar_conversions())\n",
        "print(h1demo.as_u64(18446744073709551615))\n",
        "print(h1demo.as_i32(-2147483648))\n",
        "print(h1demo.as_float(1.25))\n",
        "print(h1demo.exception_matches())\n",
    );
    let expected = concat!(
        "(True, False, -2147483648, 4294967295, 18446744073709551615, 42, -7, 1.25)\n",
        "3.5\n18446744073709551615\n-2147483648\n1.25\n11\n",
    );
    for mode in [ExecutionMode::Interpreter, ExecutionMode::Jit] {
        for gc_interval in [None, Some(1)] {
            let (vm, output) = run_with_gc(&module, source, mode, gc_interval);
            assert_eq!(output, expected, "mode={mode:?}, gc={gc_interval:?}");
            assert_eq!(vm.active_handles(), 0, "mode={mode:?}, gc={gc_interval:?}");
        }
    }
}

#[test]
fn h3_globals_are_per_runtime_and_fields_trace_without_rooting_cycles() {
    let fixture = Fixture::compile();
    let module = module(&fixture);

    let mut first = Vm::new().unwrap();
    module.register(&mut first).unwrap();
    let store = compile(
        concat!(
            "import h1demo\n",
            "value=[]\n",
            "value += [value]\n",
            "h1demo.global_store(value)\n",
        ),
        "hpy-h3-global-store",
    )
    .unwrap();
    first.run(&store, &mut Vec::new()).unwrap();
    let rooted = first.collect_garbage().unwrap();
    assert!(rooted.survivors > 0);
    let load = compile(
        "import h1demo\nprint(h1demo.global_load())",
        "hpy-h3-global-load",
    )
    .unwrap();
    let mut output = Vec::new();
    first.run(&load, &mut output).unwrap();
    assert_eq!(String::from_utf8(output).unwrap(), "[[...]]\n");

    let mut second = Vm::new().unwrap();
    module.register(&mut second).unwrap();
    let is_null = compile(
        "import h1demo\nprint(h1demo.global_is_null())",
        "hpy-h3-global-isolation",
    )
    .unwrap();
    let mut output = Vec::new();
    second.run(&is_null, &mut output).unwrap();
    assert_eq!(String::from_utf8(output).unwrap(), "True\n");

    let clear = compile(
        "import h1demo\nh1demo.global_clear()",
        "hpy-h3-global-clear",
    )
    .unwrap();
    first.run(&clear, &mut Vec::new()).unwrap();
    assert!(first.collect_garbage().unwrap().reclaimed >= 1);

    let field_source = concat!(
        "import h1demo\n",
        "def field_roundtrip():\n",
        "    owner=[]\n",
        "    value=[42]\n",
        "    h1demo.field_store(owner,value)\n",
        "    noise=[1,2,3]\n",
        "    print(h1demo.field_load(owner))\n",
        "    h1demo.field_clear(owner)\n",
        "    print(h1demo.field_is_null(owner))\n",
        "field_roundtrip()\n",
        "def make_cycle():\n",
        "    owner=[]\n",
        "    value=[owner]\n",
        "    h1demo.field_store(owner,value)\n",
        "make_cycle()\n",
    );
    for mode in [ExecutionMode::Interpreter, ExecutionMode::Jit] {
        let (mut vm, output) = run_with_gc(&module, field_source, mode, Some(1));
        assert_eq!(output, "[42]\nTrue\n");
        assert!(vm.collect_garbage().unwrap().reclaimed >= 2);
        assert_eq!(vm.active_handles(), 0);
    }
    assert_eq!(first.active_handles(), 0);
    assert_eq!(second.active_handles(), 0);
}

#[test]
fn h2_object_and_call_surface_executes_from_real_universal_extension() {
    let fixture = Fixture::compile();
    let module = module(&fixture);
    let source = concat!(
        "import h1demo\n",
        "class Box:\n",
        "    def __init__(self):\n",
        "        object.__setattr__(self,'stored',0)\n",
        "    def __setattr__(self,name,value):\n",
        "        object.__setattr__(self,'stored',value+1)\n",
        "    def __getattribute__(self,name):\n",
        "        if name=='native_value':\n",
        "            return object.__getattribute__(self,'stored')+1\n",
        "        return object.__getattribute__(self,name)\n",
        "    def __repr__(self):\n",
        "        return 'hooked-box'\n",
        "    def add(self,left,right=0):\n",
        "        return left+right\n",
        "def combine(left,right=0):\n",
        "    return left+right\n",
        "class Sequence:\n",
        "    def __init__(self):\n",
        "        self.data=[1,2,3]\n",
        "    def __len__(self):\n",
        "        return len(self.data)\n",
        "    def __getitem__(self,key):\n",
        "        return self.data[key]\n",
        "    def __setitem__(self,key,value):\n",
        "        self.data[key]=value\n",
        "    def __delitem__(self,key):\n",
        "        del self.data[key]\n",
        "    def __contains__(self,value):\n",
        "        return value in self.data\n",
        "box=Box()\n",
        "print(h1demo.argument_count(1,2,3))\n",
        "print(h1demo.argument_shape(1,2,flag=3,other=4))\n",
        "print(h1demo.attr_roundtrip(box))\n",
        "print(box.native_value)\n",
        "items=[1,2,3]\n",
        "print(h1demo.list_roundtrip(items))\n",
        "print(items)\n",
        "sequence=Sequence()\n",
        "print(h1demo.list_roundtrip(sequence))\n",
        "print(sequence.data)\n",
        "print(h1demo.dict_roundtrip())\n",
        "print(h1demo.call_positional(combine,20,22))\n",
        "print(h1demo.call_tuple_dict(combine))\n",
        "print(h1demo.call_keywords(combine))\n",
        "print(h1demo.call_method(box))\n",
    );
    let expected = concat!(
        "3\n22\nhooked-box\n75\n",
        "(99, 99, 1, 3, 2)\n[99, 3]\n",
        "(99, 99, 1, 3, 2)\n[99, 3]\n",
        "(41, 42, 1, 0)\n42\n42\n42\n42\n",
    );
    for mode in [ExecutionMode::Interpreter, ExecutionMode::Jit] {
        let (vm, output) = run(&module, source, mode);
        assert_eq!(output, expected);
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
        ("h1demo.incomplete_builder()", "SystemError"),
        ("h1demo.leak_builder()", "HandleError"),
        ("h1demo.tracker_closed_handle()", "HandleError"),
        ("h1demo.stale_tracker()", "HandleError"),
        ("h1demo.leak_tracker()", "HandleError"),
        ("h1demo.negative_tracker()", "ValueError"),
        ("h1demo.argument_count(flag=1)", "TypeError"),
        ("h1demo.list_roundtrip(1)", "TypeError"),
        ("h1demo.call_positional(1)", "TypeError"),
        ("h1demo.call_keywords(1)", "TypeError"),
        ("h1demo.as_u64(-1)", "OverflowError"),
        ("h1demo.as_i32(2147483648)", "OverflowError"),
        ("h1demo.as_float('not-a-number')", "TypeError"),
        ("h1demo.set_object('native failure')", "ValueError"),
        ("h1demo.no_memory()", "MemoryError"),
    ] {
        for mode in [ExecutionMode::Interpreter, ExecutionMode::Jit] {
            for gc_interval in [None, Some(1)] {
                let mut vm = Vm::new().unwrap();
                vm.execution_mode = mode;
                vm.gc_interval = gc_interval;
                module.register(&mut vm).unwrap();
                let program =
                    compile(&format!("import h1demo\n{expression}"), "hpy-errors").unwrap();
                let error = vm.run(&program, &mut Vec::new()).unwrap_err();
                assert_eq!(
                    error.kind, kind,
                    "expression={expression}, mode={mode:?}, gc={gc_interval:?}"
                );
                if expression.contains("set_object") {
                    assert_eq!(error.message, "native failure");
                }
                assert_eq!(
                    vm.active_handles(),
                    0,
                    "expression={expression}, mode={mode:?}, gc={gc_interval:?}"
                );
            }
        }
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
fn unsupported_method_signature_fails_closed_during_load() {
    let fixture = Fixture::compile_source(UNSUPPORTED_SOURCE);
    // SAFETY: this test compiles the source below against the pinned headers;
    // the host rejects its unsupported method before it can execute.
    let error = unsafe { UniversalModule::load("h1demo", &fixture.library) }.unwrap_err();
    assert!(error.to_string().contains("unsupported signature 99"));
}

const SOURCE: &str = r#"
#include <hpy.h>
#include <stdint.h>

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
static HPyGlobal saved_global = {0};
static HPyField saved_field = {0};

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

HPyDef_METH(make_list, "make_list", HPyFunc_NOARGS)
static HPy make_list_impl(HPyContext *ctx, HPy self) {
    (void)self;
    HPyListBuilder builder = HPyListBuilder_New(ctx, 3);
    for (int64_t i = 0; i < 3; i++) {
        HPy item = HPyLong_FromInt64_t(ctx, i + 1);
        if (HPy_IsNull(item)) { HPyListBuilder_Cancel(ctx, builder); return HPy_NULL; }
        HPyListBuilder_Set(ctx, builder, (HPy_ssize_t)i, item);
        HPy_Close(ctx, item);
    }
    HPy list = HPyListBuilder_Build(ctx, builder);
    if (HPy_IsNull(list)) return HPy_NULL;
    HPy item = HPyLong_FromInt64_t(ctx, 4);
    if (HPy_IsNull(item) || HPyList_Append(ctx, list, item) < 0) {
        HPy_Close(ctx, item);
        HPy_Close(ctx, list);
        return HPy_NULL;
    }
    HPy_Close(ctx, item);
    return list;
}

HPyDef_METH(make_tuple, "make_tuple", HPyFunc_NOARGS)
static HPy make_tuple_impl(HPyContext *ctx, HPy self) {
    (void)self;
    HPy items[2] = { HPyLong_FromInt64_t(ctx, 5), HPyLong_FromInt64_t(ctx, 6) };
    if (HPy_IsNull(items[0]) || HPy_IsNull(items[1])) return HPy_NULL;
    HPy tuple = HPyTuple_FromArray(ctx, items, 2);
    HPy_Close(ctx, items[0]);
    HPy_Close(ctx, items[1]);
    return tuple;
}

HPyDef_METH(make_tuple_builder, "make_tuple_builder", HPyFunc_NOARGS)
static HPy make_tuple_builder_impl(HPyContext *ctx, HPy self) {
    (void)self;
    HPyTupleBuilder builder = HPyTupleBuilder_New(ctx, 2);
    HPy first = HPyLong_FromInt64_t(ctx, 7);
    HPy second = HPyLong_FromInt64_t(ctx, 8);
    if (HPy_IsNull(first) || HPy_IsNull(second)) {
        HPyTupleBuilder_Cancel(ctx, builder);
        return HPy_NULL;
    }
    HPyTupleBuilder_Set(ctx, builder, 0, first);
    HPyTupleBuilder_Set(ctx, builder, 1, second);
    HPy_Close(ctx, first);
    HPy_Close(ctx, second);
    return HPyTupleBuilder_Build(ctx, builder);
}

HPyDef_METH(make_dict, "make_dict", HPyFunc_NOARGS)
static HPy make_dict_impl(HPyContext *ctx, HPy self) {
    (void)self;
    return HPyDict_New(ctx);
}

HPyDef_METH(kind_code, "kind_code", HPyFunc_O)
static HPy kind_code_impl(HPyContext *ctx, HPy self, HPy value) {
    (void)self;
    int code = HPyList_Check(ctx, value) + 2 * HPyTuple_Check(ctx, value)
        + 4 * HPyDict_Check(ctx, value);
    return HPyLong_FromInt64_t(ctx, code);
}

HPyDef_METH(cancel_builder, "cancel_builder", HPyFunc_NOARGS)
static HPy cancel_builder_impl(HPyContext *ctx, HPy self) {
    (void)self;
    HPyListBuilder builder = HPyListBuilder_New(ctx, 1);
    HPyListBuilder_Cancel(ctx, builder);
    return HPyLong_FromInt64_t(ctx, 9);
}

HPyDef_METH(incomplete_builder, "incomplete_builder", HPyFunc_NOARGS)
static HPy incomplete_builder_impl(HPyContext *ctx, HPy self) {
    (void)self;
    HPyTupleBuilder builder = HPyTupleBuilder_New(ctx, 1);
    return HPyTupleBuilder_Build(ctx, builder);
}

HPyDef_METH(leak_builder, "leak_builder", HPyFunc_NOARGS)
static HPy leak_builder_impl(HPyContext *ctx, HPy self) {
    (void)self;
    (void)HPyListBuilder_New(ctx, 1);
    return HPyLong_FromInt64_t(ctx, 10);
}

HPyDef_METH(tracker_sum, "tracker_sum", HPyFunc_NOARGS)
static HPy tracker_sum_impl(HPyContext *ctx, HPy self) {
    (void)self;
    HPyTracker tracker = HPyTracker_New(ctx, 2);
    if (tracker._i == 0) return HPy_NULL;
    HPy first = HPyLong_FromInt64_t(ctx, 19);
    HPy second = HPyLong_FromInt64_t(ctx, 23);
    if (HPy_IsNull(first) || HPy_IsNull(second)) {
        HPyTracker_Close(ctx, tracker);
        return HPy_NULL;
    }
    if (HPyTracker_Add(ctx, tracker, first) < 0 ||
        HPyTracker_Add(ctx, tracker, second) < 0) {
        HPyTracker_Close(ctx, tracker);
        return HPy_NULL;
    }
    int64_t result = HPyLong_AsInt64_t(ctx, first) + HPyLong_AsInt64_t(ctx, second);
    HPyTracker_Close(ctx, tracker);
    if (HPyErr_Occurred(ctx)) return HPy_NULL;
    return HPyLong_FromInt64_t(ctx, result);
}

HPyDef_METH(tracker_forget, "tracker_forget", HPyFunc_NOARGS)
static HPy tracker_forget_impl(HPyContext *ctx, HPy self) {
    (void)self;
    HPyTracker tracker = HPyTracker_New(ctx, 0);
    if (tracker._i == 0) return HPy_NULL;
    HPy value = HPyLong_FromInt64_t(ctx, 43);
    if (HPy_IsNull(value) || HPyTracker_Add(ctx, tracker, value) < 0) {
        HPyTracker_Close(ctx, tracker);
        return HPy_NULL;
    }
    HPyTracker_ForgetAll(ctx, tracker);
    int64_t result = HPyLong_AsInt64_t(ctx, value);
    HPy_Close(ctx, value);
    if (HPyErr_Occurred(ctx)) return HPy_NULL;
    return HPyLong_FromInt64_t(ctx, result);
}

HPyDef_METH(tracker_closed_handle, "tracker_closed_handle", HPyFunc_NOARGS)
static HPy tracker_closed_handle_impl(HPyContext *ctx, HPy self) {
    (void)self;
    HPyTracker tracker = HPyTracker_New(ctx, 1);
    if (tracker._i == 0) return HPy_NULL;
    HPy value = HPyLong_FromInt64_t(ctx, 1);
    if (HPy_IsNull(value) || HPyTracker_Add(ctx, tracker, value) < 0) {
        HPyTracker_Close(ctx, tracker);
        return HPy_NULL;
    }
    HPyTracker_Close(ctx, tracker);
    return HPy_Dup(ctx, value);
}

HPyDef_METH(stale_tracker, "stale_tracker", HPyFunc_NOARGS)
static HPy stale_tracker_impl(HPyContext *ctx, HPy self) {
    (void)self;
    HPyTracker tracker = HPyTracker_New(ctx, 0);
    if (tracker._i == 0) return HPy_NULL;
    HPyTracker_Close(ctx, tracker);
    if (HPyTracker_Add(ctx, tracker, ctx->h_None) < 0) return HPy_NULL;
    return HPy_Dup(ctx, ctx->h_None);
}

HPyDef_METH(leak_tracker, "leak_tracker", HPyFunc_NOARGS)
static HPy leak_tracker_impl(HPyContext *ctx, HPy self) {
    (void)self;
    (void)HPyTracker_New(ctx, 0);
    return HPyLong_FromInt64_t(ctx, 10);
}

HPyDef_METH(negative_tracker, "negative_tracker", HPyFunc_NOARGS)
static HPy negative_tracker_impl(HPyContext *ctx, HPy self) {
    (void)self;
    HPyTracker tracker = HPyTracker_New(ctx, -1);
    if (tracker._i == 0) return HPy_NULL;
    HPyTracker_Close(ctx, tracker);
    return HPy_Dup(ctx, ctx->h_None);
}

HPyDef_METH(argument_count, "argument_count", HPyFunc_VARARGS)
static HPy argument_count_impl(HPyContext *ctx, HPy self, const HPy *args, size_t nargs) {
    (void)self;
    (void)args;
    return HPyLong_FromInt64_t(ctx, (int64_t)nargs);
}

HPyDef_METH(argument_shape, "argument_shape", HPyFunc_KEYWORDS)
static HPy argument_shape_impl(
    HPyContext *ctx, HPy self, const HPy *args, size_t nargs, HPy kwnames
) {
    (void)self;
    (void)args;
    HPy_ssize_t keyword_count = HPy_IsNull(kwnames) ? 0 : HPy_Length(ctx, kwnames);
    if (keyword_count < 0) return HPy_NULL;
    return HPyLong_FromInt64_t(ctx, (int64_t)nargs * 10 + (int64_t)keyword_count);
}

HPyDef_METH(attr_roundtrip, "attr_roundtrip", HPyFunc_O)
static HPy attr_roundtrip_impl(HPyContext *ctx, HPy self, HPy owner) {
    (void)self;
    HPy value = HPyLong_FromInt64_t(ctx, 73);
    HPy name = HPyUnicode_FromString(ctx, "native_value");
    if (HPy_IsNull(value) || HPy_IsNull(name)) return HPy_NULL;
    if (HPy_SetAttr_s(ctx, owner, "native_value", value) < 0) return HPy_NULL;
    if (HPy_SetAttr(ctx, owner, name, value) < 0) return HPy_NULL;
    int has_string = HPy_HasAttr_s(ctx, owner, "native_value");
    int has_handle = HPy_HasAttr(ctx, owner, name);
    if (has_string < 0 || has_handle < 0) return HPy_NULL;
    if (!has_string || !has_handle) {
        return HPyErr_SetString(ctx, ctx->h_AttributeError, "attribute roundtrip failed");
    }
    HPy from_string = HPy_GetAttr_s(ctx, owner, "native_value");
    HPy from_handle = HPy_GetAttr(ctx, owner, name);
    if (HPy_IsNull(from_string) || HPy_IsNull(from_handle)) return HPy_NULL;
    int64_t from_string_value = HPyLong_AsInt64_t(ctx, from_string);
    int64_t from_handle_value = HPyLong_AsInt64_t(ctx, from_handle);
    if (HPyErr_Occurred(ctx)) return HPy_NULL;
    if (from_string_value != 75 || from_handle_value != 75) {
        return HPyErr_SetString(ctx, ctx->h_ValueError, "attribute hook was bypassed");
    }
    HPy result = HPy_Repr(ctx, owner);
    HPy_Close(ctx, from_string);
    HPy_Close(ctx, from_handle);
    HPy_Close(ctx, name);
    HPy_Close(ctx, value);
    return result;
}

HPyDef_METH(list_roundtrip, "list_roundtrip", HPyFunc_O)
static HPy list_roundtrip_impl(HPyContext *ctx, HPy self, HPy list) {
    (void)self;
    HPy_ssize_t before = HPy_Length(ctx, list);
    if (before < 0) return HPy_NULL;
    HPy value = HPyLong_FromInt64_t(ctx, 99);
    HPy key = HPyLong_FromInt64_t(ctx, 0);
    if (HPy_IsNull(value) || HPy_IsNull(key)) return HPy_NULL;
    if (HPy_SetItem_i(ctx, list, 0, value) < 0) return HPy_NULL;
    HPy from_index = HPy_GetItem_i(ctx, list, 0);
    HPy from_key = HPy_GetItem(ctx, list, key);
    int contains = HPy_Contains(ctx, list, value);
    if (HPy_IsNull(from_index) || HPy_IsNull(from_key) || contains < 0) return HPy_NULL;
    if (HPy_DelItem_i(ctx, list, 1) < 0) return HPy_NULL;
    HPy_ssize_t after = HPy_Length(ctx, list);
    if (after < 0) return HPy_NULL;
    HPy items[5] = {
        from_index,
        from_key,
        HPyLong_FromInt64_t(ctx, contains),
        HPyLong_FromInt64_t(ctx, (int64_t)before),
        HPyLong_FromInt64_t(ctx, (int64_t)after),
    };
    HPy result = HPyTuple_FromArray(ctx, items, 5);
    for (size_t index = 0; index < 5; index++) HPy_Close(ctx, items[index]);
    HPy_Close(ctx, key);
    HPy_Close(ctx, value);
    return result;
}

HPyDef_METH(dict_roundtrip, "dict_roundtrip", HPyFunc_NOARGS)
static HPy dict_roundtrip_impl(HPyContext *ctx, HPy self) {
    (void)self;
    HPy dict = HPyDict_New(ctx);
    HPy key = HPyUnicode_FromString(ctx, "answer");
    HPy first = HPyLong_FromInt64_t(ctx, 41);
    HPy second = HPyLong_FromInt64_t(ctx, 42);
    if (HPy_IsNull(dict) || HPy_IsNull(key) || HPy_IsNull(first) || HPy_IsNull(second)) {
        return HPy_NULL;
    }
    if (HPy_SetItem_s(ctx, dict, "answer", first) < 0) return HPy_NULL;
    HPy from_key = HPy_GetItem(ctx, dict, key);
    int contains = HPy_Contains(ctx, dict, key);
    if (HPy_IsNull(from_key) || contains < 0) return HPy_NULL;
    if (HPy_DelItem(ctx, dict, key) < 0) return HPy_NULL;
    if (HPy_SetItem(ctx, dict, key, second) < 0) return HPy_NULL;
    HPy from_string = HPy_GetItem_s(ctx, dict, "answer");
    if (HPy_IsNull(from_string)) return HPy_NULL;
    if (HPy_DelItem_s(ctx, dict, "answer") < 0) return HPy_NULL;
    HPy_ssize_t length = HPy_Length(ctx, dict);
    if (length < 0) return HPy_NULL;
    HPy items[4] = {
        from_key,
        from_string,
        HPyLong_FromInt64_t(ctx, contains),
        HPyLong_FromInt64_t(ctx, (int64_t)length),
    };
    HPy result = HPyTuple_FromArray(ctx, items, 4);
    for (size_t index = 0; index < 4; index++) HPy_Close(ctx, items[index]);
    HPy_Close(ctx, second);
    HPy_Close(ctx, first);
    HPy_Close(ctx, key);
    HPy_Close(ctx, dict);
    return result;
}

HPyDef_METH(call_positional, "call_positional", HPyFunc_VARARGS)
static HPy call_positional_impl(
    HPyContext *ctx, HPy self, const HPy *args, size_t nargs
) {
    (void)self;
    if (nargs < 1 || !HPyCallable_Check(ctx, args[0])) {
        return HPyErr_SetString(ctx, ctx->h_TypeError, "expected a callable");
    }
    return HPy_Call(ctx, args[0], args + 1, nargs - 1, HPy_NULL);
}

HPyDef_METH(call_tuple_dict, "call_tuple_dict", HPyFunc_O)
static HPy call_tuple_dict_impl(HPyContext *ctx, HPy self, HPy callable) {
    (void)self;
    HPy twenty = HPyLong_FromInt64_t(ctx, 20);
    HPy twenty_two = HPyLong_FromInt64_t(ctx, 22);
    HPy args = HPyTuple_FromArray(ctx, &twenty, 1);
    HPy keywords = HPyDict_New(ctx);
    if (HPy_IsNull(args) || HPy_IsNull(keywords)) return HPy_NULL;
    if (HPy_SetItem_s(ctx, keywords, "right", twenty_two) < 0) return HPy_NULL;
    HPy result = HPy_CallTupleDict(ctx, callable, args, keywords);
    HPy_Close(ctx, keywords);
    HPy_Close(ctx, args);
    HPy_Close(ctx, twenty_two);
    HPy_Close(ctx, twenty);
    return result;
}

HPyDef_METH(call_keywords, "call_keywords", HPyFunc_O)
static HPy call_keywords_impl(HPyContext *ctx, HPy self, HPy callable) {
    (void)self;
    HPy arguments[2] = {
        HPyLong_FromInt64_t(ctx, 20),
        HPyLong_FromInt64_t(ctx, 22),
    };
    HPy name = HPyUnicode_FromString(ctx, "right");
    HPy names = HPyTuple_FromArray(ctx, &name, 1);
    if (HPy_IsNull(arguments[0]) || HPy_IsNull(arguments[1]) || HPy_IsNull(names)) {
        return HPy_NULL;
    }
    HPy result = HPy_Call(ctx, callable, arguments, 1, names);
    HPy_Close(ctx, names);
    HPy_Close(ctx, name);
    HPy_Close(ctx, arguments[1]);
    HPy_Close(ctx, arguments[0]);
    return result;
}

HPyDef_METH(call_method, "call_method", HPyFunc_O)
static HPy call_method_impl(HPyContext *ctx, HPy self, HPy receiver) {
    (void)self;
    HPy name = HPyUnicode_FromString(ctx, "add");
    HPy keyword_name = HPyUnicode_FromString(ctx, "right");
    HPy keyword_names = HPyTuple_FromArray(ctx, &keyword_name, 1);
    HPy arguments[3] = {
        receiver,
        HPyLong_FromInt64_t(ctx, 35),
        HPyLong_FromInt64_t(ctx, 7),
    };
    if (HPy_IsNull(name) || HPy_IsNull(keyword_names)
        || HPy_IsNull(arguments[1]) || HPy_IsNull(arguments[2])) return HPy_NULL;
    HPy result = HPy_CallMethod(ctx, name, arguments, 2, keyword_names);
    HPy_Close(ctx, arguments[2]);
    HPy_Close(ctx, arguments[1]);
    HPy_Close(ctx, keyword_names);
    HPy_Close(ctx, keyword_name);
    HPy_Close(ctx, name);
    return result;
}

HPyDef_METH(scalar_values, "scalar_values", HPyFunc_NOARGS)
static HPy scalar_values_impl(HPyContext *ctx, HPy self) {
    (void)self;
    HPy items[8] = {
        HPyBool_FromBool(ctx, true),
        HPyBool_FromBool(ctx, false),
        HPyLong_FromInt32_t(ctx, INT32_MIN),
        HPyLong_FromUInt32_t(ctx, UINT32_MAX),
        HPyLong_FromUInt64_t(ctx, UINT64_MAX),
        HPyLong_FromSize_t(ctx, (size_t)42),
        HPyLong_FromSsize_t(ctx, (HPy_ssize_t)-7),
        HPyFloat_FromDouble(ctx, 1.25),
    };
    for (size_t index = 0; index < 8; index++) {
        if (HPy_IsNull(items[index])) return HPy_NULL;
    }
    HPy result = HPyTuple_FromArray(ctx, items, 8);
    for (size_t index = 0; index < 8; index++) HPy_Close(ctx, items[index]);
    return result;
}

HPyDef_METH(scalar_conversions, "scalar_conversions", HPyFunc_NOARGS)
static HPy scalar_conversions_impl(HPyContext *ctx, HPy self) {
    (void)self;
    HPy minus_one = HPyLong_FromInt32_t(ctx, -1);
    HPy pointer_value = HPyLong_FromSize_t(ctx, (size_t)42);
    HPy exact_float_integer = HPyLong_FromUInt64_t(ctx, UINT64_C(9007199254740992));
    HPy floating = HPyFloat_FromDouble(ctx, 1.5);
    if (HPy_IsNull(minus_one) || HPy_IsNull(pointer_value)
        || HPy_IsNull(exact_float_integer) || HPy_IsNull(floating)) return HPy_NULL;
    uint32_t mask32 = HPyLong_AsUInt32_tMask(ctx, minus_one);
    uint64_t mask64 = HPyLong_AsUInt64_tMask(ctx, minus_one);
    size_t size = HPyLong_AsSize_t(ctx, pointer_value);
    HPy_ssize_t ssize = HPyLong_AsSsize_t(ctx, minus_one);
    void *pointer = HPyLong_AsVoidPtr(ctx, pointer_value);
    double integer_float = HPyLong_AsDouble(ctx, exact_float_integer);
    double value_float = HPyFloat_AsDouble(ctx, floating);
    HPy_Close(ctx, floating);
    HPy_Close(ctx, exact_float_integer);
    HPy_Close(ctx, pointer_value);
    HPy_Close(ctx, minus_one);
    if (HPyErr_Occurred(ctx)) return HPy_NULL;
    if (mask32 != UINT32_MAX || mask64 != UINT64_MAX || size != 42
        || ssize != -1 || (uintptr_t)pointer != 42
        || integer_float != 9007199254740992.0 || value_float != 1.5) {
        return HPyErr_SetString(ctx, ctx->h_ValueError, "scalar conversion mismatch");
    }
    return HPyFloat_FromDouble(ctx, 3.5);
}

HPyDef_METH(as_u64, "as_u64", HPyFunc_O)
static HPy as_u64_impl(HPyContext *ctx, HPy self, HPy value) {
    (void)self;
    uint64_t converted = HPyLong_AsUInt64_t(ctx, value);
    if (HPyErr_Occurred(ctx)) return HPy_NULL;
    return HPyLong_FromUInt64_t(ctx, converted);
}

HPyDef_METH(as_i32, "as_i32", HPyFunc_O)
static HPy as_i32_impl(HPyContext *ctx, HPy self, HPy value) {
    (void)self;
    int32_t converted = HPyLong_AsInt32_t(ctx, value);
    if (HPyErr_Occurred(ctx)) return HPy_NULL;
    return HPyLong_FromInt32_t(ctx, converted);
}

HPyDef_METH(as_float, "as_float", HPyFunc_O)
static HPy as_float_impl(HPyContext *ctx, HPy self, HPy value) {
    (void)self;
    double converted = HPyFloat_AsDouble(ctx, value);
    if (HPyErr_Occurred(ctx)) return HPy_NULL;
    return HPyFloat_FromDouble(ctx, converted);
}

HPyDef_METH(set_object, "set_object", HPyFunc_O)
static HPy set_object_impl(HPyContext *ctx, HPy self, HPy value) {
    (void)self;
    return HPyErr_SetObject(ctx, ctx->h_ValueError, value);
}

HPyDef_METH(exception_matches, "exception_matches", HPyFunc_NOARGS)
static HPy exception_matches_impl(HPyContext *ctx, HPy self) {
    (void)self;
    HPyErr_SetString(ctx, ctx->h_ValueError, "match me");
    int value = HPyErr_ExceptionMatches(ctx, ctx->h_ValueError);
    int base = HPyErr_ExceptionMatches(ctx, ctx->h_Exception);
    int other = HPyErr_ExceptionMatches(ctx, ctx->h_TypeError);
    HPy expected_items[2] = { ctx->h_TypeError, ctx->h_ValueError };
    HPy expected = HPyTuple_FromArray(ctx, expected_items, 2);
    if (HPy_IsNull(expected)) return HPy_NULL;
    int tuple = HPyErr_ExceptionMatches(ctx, expected);
    HPy_Close(ctx, expected);
    if (value < 0 || base < 0 || other < 0 || tuple < 0) return HPy_NULL;
    if (tuple != 1) return HPyErr_SetString(ctx, ctx->h_ValueError, "tuple did not match");
    HPyErr_Clear(ctx);
    return HPyLong_FromInt32_t(ctx, value * 10 + base + other * 100);
}

HPyDef_METH(no_memory, "no_memory", HPyFunc_NOARGS)
static HPy no_memory_impl(HPyContext *ctx, HPy self) {
    (void)self;
    return HPyErr_NoMemory(ctx);
}

HPyDef_METH(global_store, "global_store", HPyFunc_O)
static HPy global_store_impl(HPyContext *ctx, HPy self, HPy value) {
    (void)self;
    HPyGlobal_Store(ctx, &saved_global, value);
    if (HPyErr_Occurred(ctx)) return HPy_NULL;
    return HPy_Dup(ctx, ctx->h_None);
}

HPyDef_METH(global_load, "global_load", HPyFunc_NOARGS)
static HPy global_load_impl(HPyContext *ctx, HPy self) {
    (void)self;
    return HPyGlobal_Load(ctx, saved_global);
}

HPyDef_METH(global_is_null, "global_is_null", HPyFunc_NOARGS)
static HPy global_is_null_impl(HPyContext *ctx, HPy self) {
    (void)self;
    HPy value = HPyGlobal_Load(ctx, saved_global);
    bool is_null = HPy_IsNull(value);
    HPy_Close(ctx, value);
    return HPyBool_FromBool(ctx, is_null);
}

HPyDef_METH(global_clear, "global_clear", HPyFunc_NOARGS)
static HPy global_clear_impl(HPyContext *ctx, HPy self) {
    (void)self;
    HPyGlobal_Store(ctx, &saved_global, HPy_NULL);
    if (HPyErr_Occurred(ctx)) return HPy_NULL;
    return HPy_Dup(ctx, ctx->h_None);
}

HPyDef_METH(field_store, "field_store", HPyFunc_VARARGS)
static HPy field_store_impl(
    HPyContext *ctx, HPy self, const HPy *args, size_t nargs
) {
    (void)self;
    if (nargs != 2) return HPyErr_SetString(ctx, ctx->h_TypeError, "expected owner and value");
    HPyField_Store(ctx, args[0], &saved_field, args[1]);
    if (HPyErr_Occurred(ctx)) return HPy_NULL;
    return HPy_Dup(ctx, ctx->h_None);
}

HPyDef_METH(field_load, "field_load", HPyFunc_O)
static HPy field_load_impl(HPyContext *ctx, HPy self, HPy owner) {
    (void)self;
    return HPyField_Load(ctx, owner, saved_field);
}

HPyDef_METH(field_is_null, "field_is_null", HPyFunc_O)
static HPy field_is_null_impl(HPyContext *ctx, HPy self, HPy owner) {
    (void)self;
    HPy value = HPyField_Load(ctx, owner, saved_field);
    bool is_null = HPy_IsNull(value);
    HPy_Close(ctx, value);
    return HPyBool_FromBool(ctx, is_null);
}

HPyDef_METH(field_clear, "field_clear", HPyFunc_O)
static HPy field_clear_impl(HPyContext *ctx, HPy self, HPy owner) {
    (void)self;
    HPyField_Store(ctx, owner, &saved_field, HPy_NULL);
    if (HPyErr_Occurred(ctx)) return HPy_NULL;
    return HPy_Dup(ctx, ctx->h_None);
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
    &make_list,
    &make_tuple,
    &make_tuple_builder,
    &make_dict,
    &kind_code,
    &cancel_builder,
    &incomplete_builder,
    &leak_builder,
    &tracker_sum,
    &tracker_forget,
    &tracker_closed_handle,
    &stale_tracker,
    &leak_tracker,
    &negative_tracker,
    &argument_count,
    &argument_shape,
    &attr_roundtrip,
    &list_roundtrip,
    &dict_roundtrip,
    &call_positional,
    &call_tuple_dict,
    &call_keywords,
    &call_method,
    &scalar_values,
    &scalar_conversions,
    &as_u64,
    &as_i32,
    &as_float,
    &set_object,
    &exception_matches,
    &no_memory,
    &global_store,
    &global_load,
    &global_is_null,
    &global_clear,
    &field_store,
    &field_load,
    &field_is_null,
    &field_clear,
    NULL,
};

static HPyGlobal *module_globals[] = {
    &saved_global,
    NULL,
};

static HPyModuleDef module_definition = {
    .doc = "Tonic H1/H2 integration fixture",
    .size = 0,
    .legacy_methods = NULL,
    .defines = module_defines,
    .globals = module_globals,
};

HPy_MODINIT(h1demo, module_definition)
"#;

const UNSUPPORTED_SOURCE: &str = r#"
#include <hpy.h>

static HPy invalid_impl(HPyContext *ctx, HPy self) {
    (void)self;
    return HPyLong_FromInt64_t(ctx, 0);
}

static HPyDef invalid = {
    .kind = HPyDef_Kind_Meth,
    .meth = {
        .name = "invalid",
        .impl = (HPyCFunction)invalid_impl,
        .cpy_trampoline = NULL,
        .signature = (HPyFunc_Signature)99,
        .doc = NULL,
    },
};

static HPyDef *module_defines[] = { &invalid, NULL };
static HPyModuleDef module_definition = {
    .doc = "unsupported signature fixture",
    .size = 0,
    .legacy_methods = NULL,
    .defines = module_defines,
    .globals = NULL,
};

HPy_MODINIT(h1demo, module_definition)
"#;
