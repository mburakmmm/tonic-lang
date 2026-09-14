use std::cell::RefCell;
use tonic_compiler::compile;
use tonic_core::diagnostic::Result;
use tonic_runtime::{Context, Handle, PersistentHandle, RuntimePhase, Vm};

thread_local! {
    static CALLBACK: RefCell<Option<PersistentHandle>> = const { RefCell::new(None) };
}

fn save_callback(context: &mut Context<'_>, arguments: &[Handle]) -> Result<Handle> {
    let callback = context.persist(arguments[0])?;
    CALLBACK.with(|slot| *slot.borrow_mut() = Some(callback));
    context.none()
}

fn take_callback() -> PersistentHandle {
    CALLBACK.with(|slot| slot.borrow_mut().take().expect("saved callback"))
}

#[test]
fn persistent_closure_reenters_its_retained_module_after_run() {
    CALLBACK.with(|slot| *slot.borrow_mut() = None);
    let program = compile(
        "import callback\ndef make(base):\n    def add(value):\n        return base+value\n    return add\ncallback.save(make(40))",
        "callback",
    )
    .unwrap();
    let mut vm = Vm::new().unwrap();
    vm.register_native("callback", "save", 1, save_callback)
        .unwrap();
    vm.run(&program, &mut Vec::new()).unwrap();
    drop(program);
    let callback = take_callback();
    assert!(vm.collect_garbage().unwrap().moved > 0);
    let argument = {
        let mut context = vm.context().unwrap();
        let local = context.from_i64(2).unwrap();
        context.persist(local).unwrap()
    };
    let result = vm
        .call_persistent(&callback, &[&argument], &mut Vec::new())
        .unwrap();
    {
        let mut context = vm.context().unwrap();
        let local = context.borrow_persistent(&result).unwrap();
        assert_eq!(context.to_i64(local).unwrap(), 42);
        context.release_persistent(&result).unwrap();
        context.release_persistent(&argument).unwrap();
        context.release_persistent(&callback).unwrap();
    }
    assert_eq!(vm.stats.callback_calls, 1);
    assert_eq!(vm.active_handles(), 0);
}

#[test]
fn callback_exception_cleans_frames_and_allows_a_later_callback() {
    CALLBACK.with(|slot| *slot.borrow_mut() = None);
    let program = compile(
        "import callback\ndef divide(value):\n    return 10//value\ncallback.save(divide)",
        "callback",
    )
    .unwrap();
    let mut vm = Vm::new().unwrap();
    vm.register_native("callback", "save", 1, save_callback)
        .unwrap();
    vm.run(&program, &mut Vec::new()).unwrap();
    let callback = take_callback();
    let (zero, two) = {
        let mut context = vm.context().unwrap();
        let zero = context.from_i64(0).unwrap();
        let zero = context.persist(zero).unwrap();
        let two = context.from_i64(2).unwrap();
        let two = context.persist(two).unwrap();
        (zero, two)
    };
    assert_eq!(
        vm.call_persistent(&callback, &[&zero], &mut Vec::new())
            .unwrap_err()
            .kind,
        "ZeroDivisionError"
    );
    let result = vm
        .call_persistent(&callback, &[&two], &mut Vec::new())
        .unwrap();
    let mut context = vm.context().unwrap();
    let local = context.borrow_persistent(&result).unwrap();
    assert_eq!(context.to_i64(local).unwrap(), 5);
    for handle in [&result, &zero, &two, &callback] {
        context.release_persistent(handle).unwrap();
    }
    drop(context);
    assert_eq!(vm.stats.callback_calls, 2);
}

#[test]
fn detached_runtime_can_move_and_attach_to_another_thread() {
    let program = compile("print(42)", "thread").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.detach_current_thread().unwrap();
    let (mut vm, output) = std::thread::spawn(move || {
        vm.attach_current_thread().unwrap();
        let mut output = Vec::new();
        vm.run(&program, &mut output).unwrap();
        vm.detach_current_thread().unwrap();
        (vm, output)
    })
    .join()
    .unwrap();
    assert_eq!(output, b"42\n");
    vm.attach_current_thread().unwrap();
    let mut output = Vec::new();
    vm.run(&compile("print(7)", "main").unwrap(), &mut output)
        .unwrap();
    assert_eq!(output, b"7\n");
}

#[test]
fn attached_runtime_rejects_use_from_a_foreign_thread() {
    let program = compile("pass", "thread").unwrap();
    let vm = Vm::new().unwrap();
    let (mut vm, kind) = std::thread::spawn(move || {
        let mut vm = vm;
        let kind = vm.run(&program, &mut Vec::new()).unwrap_err().kind;
        (vm, kind)
    })
    .join()
    .unwrap();
    assert_eq!(kind, "ThreadError");
    vm.detach_current_thread().unwrap();
}

#[test]
fn staged_shutdown_invalidates_roots_and_rejects_future_work() {
    let mut vm = Vm::new().unwrap();
    let persistent = {
        let mut context = vm.context().unwrap();
        let local = context.from_f64_buffer(&[1.0, 2.0], &[2], false).unwrap();
        context.persist(local).unwrap()
    };
    vm.begin_shutdown().unwrap();
    assert_eq!(vm.phase(), RuntimePhase::ShuttingDown);
    assert_eq!(vm.context().err().unwrap().kind, "RuntimeError");
    let collection = vm.finalize_shutdown().unwrap();
    assert_eq!(vm.phase(), RuntimePhase::Finalizing);
    assert_eq!(collection.survivors, 0);
    assert_eq!(vm.active_handles(), 0);
    vm.complete_shutdown().unwrap();
    assert_eq!(vm.phase(), RuntimePhase::Dead);
    assert_eq!(vm.attach_current_thread().unwrap_err().kind, "RuntimeError");
    assert_eq!(
        vm.run(&compile("pass", "dead").unwrap(), &mut Vec::new())
            .unwrap_err()
            .kind,
        "RuntimeError"
    );
    let _invalidated_host_token = persistent;
}
