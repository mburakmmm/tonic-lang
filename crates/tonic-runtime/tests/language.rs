use tonic_compiler::{compile, compile_modules, ModuleSource};
use tonic_core::diagnostic::{Diagnostic, Result};
use tonic_runtime::{Context, ExecutionMode, Handle, Vm};
fn output(source: &str) -> String {
    let p = compile(source, "test.tonic").unwrap();
    let mut out = Vec::new();
    Vm::new().unwrap().run(&p, &mut out).unwrap();
    String::from_utf8(out).unwrap()
}
fn assert_output_under_stress_gc_and_jit(source: &str, expected: &[u8]) {
    let program = compile(source, "stress-language").unwrap();
    for mode in [ExecutionMode::Interpreter, ExecutionMode::Jit] {
        let mut vm = Vm::new().unwrap();
        vm.execution_mode = mode;
        vm.gc_interval = Some(1);
        let mut out = Vec::new();
        vm.run(&program, &mut out).unwrap();
        assert_eq!(out, expected);
    }
}
fn error(source: &str) -> Diagnostic {
    let p = compile(source, "test.tonic").unwrap();
    Vm::new().unwrap().run(&p, &mut Vec::new()).unwrap_err()
}
#[test]
fn fib() {
    assert_eq!(
        output(include_str!("../../../examples/fib.tonic")),
        "102334155\n"
    );
}

#[test]
fn f_strings_format_in_order_with_nested_specs_and_conversions() {
    let source = "import asyncio\ndef mark(value):\n    print('mark',value)\n    return value\nname='Tönic'\nwidth=6\nprint(f'hello {name} {mark(42):04d} {3.14159:.2f}')\nprint(f'{42:{width}d}',f'{name!r}',f'{name!a}',f'{name:*^9.3s}')\nclass Display:\n    def __str__(self):\n        print('str-call')\n        return 'string'\n    def __repr__(self):\n        print('repr-call')\n        return 'répr'\n    def __format__(self,spec):\n        print('format-call',spec)\n        return '['+spec+']'\nvalue=Display()\nprint(f'{value!s}',f'{value!r}',f'{value!a}',f'{value:custom}')\nasync def get():\n    return 7\nasync def render():\n    return f'{await get():04d}'\nprint(asyncio.run(render()))";
    assert_output_under_stress_gc_and_jit(
        source,
        b"mark 42\nhello T\xc3\xb6nic 0042 3.14\n    42 'T\xc3\xb6nic' 'T\\xf6nic' ***T\xc3\xb6n***\nstr-call\nrepr-call\nrepr-call\nformat-call custom\nstring r\xc3\xa9pr r\\xe9pr [custom]\n0007\n",
    );
    assert_eq!(
        error("class Bad:\n    def __format__(self,spec): return 1\nprint(f'{Bad()}')").kind,
        "TypeError"
    );
}

#[test]
fn generators_suspend_resume_and_feed_all_builtin_consumers() {
    let source = "def generate(n):\n    i=0\n    while i<n:\n        sent=(yield i)\n        print('sent',sent)\n        i+=1\nprint(type(generate(0)).__name__)\nmethods=generate(1)\nprint(iter(methods)==methods,methods.__iter__()==methods,methods.__next__())\ntry:\n    methods.__next__()\nexcept StopIteration:\n    print('method-stopped')\ng=generate(2)\nprint('next',next(g),next(g),next(g,'done'))\ntry:\n    next(g)\nexcept StopIteration:\n    print('stopped')\nprint(list(generate(3)))\nfor value in generate(2):\n    print('for',value)\na,b=generate(2)\nprint('unpack',a,b)\ndef collect(*values):\n    print('star',values)\ncollect(*generate(3))\nprint('exhausted',list(generate(0)))";
    assert_eq!(
        output(source),
        "generator\nTrue True 0\nsent None\nmethod-stopped\nsent None\nsent None\nnext 0 1 done\nstopped\nsent None\nsent None\nsent None\n[0, 1, 2]\nfor 0\nsent None\nfor 1\nsent None\nsent None\nsent None\nunpack 0 1\nsent None\nsent None\nsent None\nstar (0, 1, 2)\nexhausted []\n"
    );
}

#[test]
fn generator_frames_preserve_closures_exceptions_and_gc_roots() {
    let source = "def outer():\n    captured=['kept']\n    def generate():\n        try:\n            yield captured\n            raise ValueError('handled')\n        except ValueError:\n            yield captured\n    return generate()\nprint(list(outer()))";
    let program = compile(source, "generator-roots").unwrap();
    for mode in [ExecutionMode::Interpreter, ExecutionMode::Jit] {
        let mut vm = Vm::new().unwrap();
        vm.execution_mode = mode;
        vm.gc_interval = Some(1);
        let mut out = Vec::new();
        vm.run(&program, &mut out).unwrap();
        assert_eq!(out, b"[['kept'], ['kept']]\n");
    }
}

#[test]
fn generator_send_throw_and_close_follow_suspension_protocol() {
    let source = "def dialogue():\n    received=(yield 'ready')\n    yield received\ng=dialogue()\ntry:\n    g.send('early')\nexcept TypeError:\n    print('send-before-start')\nprint(g.send(None))\nprint(g.send('value'))\ntry:\n    g.send('late')\nexcept StopIteration:\n    print('send-stopped')\ndef guarded():\n    try:\n        yield 'start'\n    except ValueError as error:\n        yield 'caught '+str(error)\n    yield 'end'\nt=guarded()\nprint(next(t))\nprint(t.throw(ValueError('boom')))\nprint(next(t))\ndef closing(ignore):\n    try:\n        yield 1\n    except GeneratorExit:\n        print('closing',ignore)\n        if ignore:\n            yield 2\nok=closing(False)\nprint(next(ok),ok.close(),next(ok,'done'))\nbad=closing(True)\nprint(next(bad))\ntry:\n    bad.close()\nexcept RuntimeError as error:\n    print(str(error))\nfresh=closing(False)\nprint(fresh.close(),next(fresh,'done'))";
    assert_eq!(
        output(source),
        "send-before-start\nready\nvalue\nsend-stopped\nstart\ncaught boom\nend\nclosing False\n1 None done\n1\nclosing True\ngenerator ignored GeneratorExit\nNone done\n"
    );
}

#[test]
fn generator_throw_supports_legacy_type_value_and_traceback_forms() {
    let source = "def catcher():\n    try:\n        yield 'ready'\n    except Exception as error:\n        yield type(error).__name__,error.args,error.__traceback__==None\ndef run(*arguments):\n    g=catcher()\n    print(next(g),g.throw(*arguments))\nrun(ValueError)\nrun(ValueError,'message')\nrun(ValueError,(1,2),None)\ninstance=ValueError('instance')\ng=catcher()\nnext(g)\ntry:\n    g.throw(instance,'separate')\nexcept TypeError as error:\n    print(str(error))\ntry:\n    raise RuntimeError('source')\nexcept RuntimeError as source:\n    traceback=source.__traceback__\ng=catcher()\nprint(next(g),g.throw(TypeError,'with-traceback',traceback))\nclass CustomError(Exception):\n    def __init__(self,value):\n        print('custom-init',value)\ng=catcher()\nprint(next(g),g.throw(CustomError,'custom'))\ng=catcher()\nnext(g)\ntry:\n    g.throw(ValueError,'bad-traceback',1)\nexcept TypeError as error:\n    print(str(error))";
    assert_output_under_stress_gc_and_jit(
        source,
        b"ready ('ValueError', (), False)\nready ('ValueError', ('message',), False)\nready ('ValueError', (1, 2), False)\ninstance exception may not have a separate value\nready ('TypeError', ('with-traceback',), False)\ncustom-init custom\nready ('CustomError', ('custom',), False)\nthrow traceback must be a traceback object or None\n",
    );
}

#[test]
fn yield_from_delegates_to_generators_and_builtin_iterables() {
    let source = "def inner():\n    yield 1\n    yield 2\n    return 9\ndef outer():\n    yield 0\n    result=(yield from inner())\n    print('delegated-result',result)\n    yield from [3,4]\n    yield 5\nprint(list(outer()))";
    assert_eq!(output(source), "delegated-result 9\n[0, 1, 2, 3, 4, 5]\n");
}

#[test]
fn yield_from_forwards_send_and_preserves_delegate_results() {
    let source = "def inner():\n    received=yield 'inner-ready'\n    yield received\n    return 7\ndef outer():\n    result=yield from inner()\n    print('inner-result',result)\ng=outer()\nprint(next(g),g.send('sent'))\ntry:\n    next(g)\nexcept StopIteration as error:\n    print('outer-result',error.value)\nclass Sender:\n    def __init__(self):\n        self.state=0\n    def __iter__(self):\n        return self\n    def __next__(self):\n        if self.state==0:\n            self.state=1\n            return 'custom-ready'\n        raise StopIteration(8)\n    def send(self,value):\n        self.state=2\n        return value\ndef custom_outer():\n    result=yield from Sender()\n    print('custom-result',result)\nc=custom_outer()\nprint(next(c),c.send('custom-sent'))\ntry:\n    next(c)\nexcept StopIteration:\n    pass\ndef list_outer():\n    yield from [1,2]\nl=list_outer()\nprint(next(l))\ntry:\n    l.send(3)\nexcept AttributeError:\n    print('no-send')";
    assert_output_under_stress_gc_and_jit(
        source,
        b"inner-ready sent\ninner-result 7\nouter-result None\ncustom-ready custom-sent\ncustom-result 8\n1\nno-send\n",
    );
}

#[test]
fn yield_from_forwards_throw_and_close_through_outer_handlers() {
    let source = "def guarded():\n    try:\n        try:\n            yield 'ready'\n        except ValueError as error:\n            yield 'caught '+str(error)\n        return 11\n    finally:\n        print('inner-finally')\ndef delegated():\n    try:\n        result=yield from guarded()\n        print('delegated-result',result)\n    finally:\n        print('outer-finally')\ng=delegated()\nprint(next(g),g.throw(ValueError('boom')))\ntry:\n    next(g)\nexcept StopIteration:\n    print('delegated-done')\nclass Plain:\n    def __iter__(self):\n        return self\n    def __next__(self):\n        return 'plain'\ndef catches_missing_throw():\n    try:\n        yield from Plain()\n    except ValueError as error:\n        yield 'outer-caught '+str(error)\np=catches_missing_throw()\nprint(next(p),p.throw(ValueError('missing')))\nclass Custom:\n    def __iter__(self):\n        return self\n    def __next__(self):\n        return 'custom'\n    def throw(self,error):\n        print('custom-throw',str(error))\n        return 'custom-caught'\n    def close(self):\n        print('custom-close')\ndef custom_outer():\n    try:\n        yield from Custom()\n    finally:\n        print('custom-outer-finally')\nc=custom_outer()\nprint(next(c),c.throw(ValueError('custom-error')))\nprint(c.close())\ndef closing_inner():\n    try:\n        yield 'closing'\n    finally:\n        print('generator-inner-close')\ndef closing_outer():\n    try:\n        yield from closing_inner()\n    finally:\n        print('generator-outer-close')\nx=closing_outer()\nprint(next(x),x.close())";
    assert_output_under_stress_gc_and_jit(
        source,
        b"ready caught boom\ninner-finally\ndelegated-result 11\nouter-finally\ndelegated-done\nplain outer-caught missing\ncustom-throw custom-error\ncustom custom-caught\ncustom-close\ncustom-outer-finally\nNone\ngenerator-inner-close\ngenerator-outer-close\nclosing None\n",
    );
}

#[test]
fn stop_iteration_value_survives_generators_custom_iterators_and_gc() {
    let source = "class Finished:\n    def __iter__(self):\n        return self\n    def __next__(self):\n        raise StopIteration(12)\ndef delegated():\n    result=yield from Finished()\n    print('custom-result',result)\ndef returning():\n    yield 'ready'\n    return ['kept']\nprint(list(delegated()))\ng=returning()\nprint(next(g))\ntry:\n    next(g)\nexcept StopIteration as error:\n    print(error.value,error.args)\n    error.value='changed'\n    print(error.value,error.args)\n    del error.value\n    print(error.value,error.args)\ntry:\n    next(g)\nexcept StopIteration as error:\n    print(error.value,error.args)\ne=StopIteration(1,2)\nprint(e.value,e.args)";
    let program = compile(source, "stop-iteration-value").unwrap();
    for mode in [ExecutionMode::Interpreter, ExecutionMode::Jit] {
        let mut vm = Vm::new().unwrap();
        vm.execution_mode = mode;
        vm.gc_interval = Some(1);
        let mut out = Vec::new();
        vm.run(&program, &mut out).unwrap();
        assert_eq!(
            out,
            b"custom-result 12\n[]\nready\n['kept'] (['kept'],)\nchanged (['kept'],)\nNone (['kept'],)\nNone ()\n1 (1, 2)\n"
        );
    }
}

#[test]
fn generator_stop_iteration_is_converted_at_the_boundary() {
    let source = "def caught():\n    try:\n        raise StopIteration('inside')\n    except StopIteration as error:\n        yield str(error)\ndef escaped():\n    yield 'start'\n    raise StopIteration('escaped')\nprint(list(caught()))\ng=escaped()\nprint(next(g))\ntry:\n    next(g)\nexcept RuntimeError as error:\n    print(str(error))";
    assert_eq!(
        output(source),
        "['inside']\nstart\ngenerator raised StopIteration\n"
    );
}

#[test]
fn unreachable_suspended_generators_run_finally_outside_the_collector() {
    let source = "def closing(label):\n    try:\n        yield label\n    finally:\n        print('finalized',label)\ndef abandon():\n    generator=closing('one')\n    print(next(generator))\nabandon()\nprint('body-complete')";
    let program = compile(source, "generator-finalization").unwrap();
    for mode in [ExecutionMode::Interpreter, ExecutionMode::Jit] {
        let mut vm = Vm::new().unwrap();
        vm.execution_mode = mode;
        vm.gc_interval = None;
        let mut out = Vec::new();
        vm.run(&program, &mut out).unwrap();
        assert_eq!(out, b"one\nbody-complete\n");
        vm.collect_garbage_with_output(&mut out).unwrap();
        assert_eq!(out, b"one\nbody-complete\nfinalized one\n");
        assert_eq!(vm.stats.generator_finalizers, 1);
        assert_eq!(vm.stats.generator_finalizer_errors, 0);
        let reclaimed = vm.collect_garbage().unwrap().reclaimed;
        assert!(reclaimed >= 1, "physical reclamation follows logical close");
    }
}

#[test]
fn unreachable_suspended_async_generators_run_finally_outside_the_collector() {
    let source = r#"async def closing(label):
    try:
        yield label
    finally:
        print('async-finalized',label)
def abandon():
    generator=closing('one')
    try:
        generator.__anext__().send(None)
    except StopIteration as error:
        print(error.value)
abandon()
print('body-complete')"#;
    let program = compile(source, "async-generator-finalization").unwrap();
    for mode in [ExecutionMode::Interpreter, ExecutionMode::Jit] {
        let mut vm = Vm::new().unwrap();
        vm.execution_mode = mode;
        vm.gc_interval = None;
        let mut out = Vec::new();
        vm.run(&program, &mut out).unwrap();
        assert_eq!(out, b"one\nbody-complete\n");
        vm.collect_garbage_with_output(&mut out).unwrap();
        assert_eq!(out, b"one\nbody-complete\nasync-finalized one\n");
        assert_eq!(vm.stats.generator_finalizers, 1);
        assert_eq!(vm.stats.generator_finalizer_errors, 0);
        let reclaimed = vm.collect_garbage().unwrap().reclaimed;
        assert!(reclaimed >= 1, "physical reclamation follows logical close");
    }
}

#[test]
fn generator_finalization_closes_delegates_and_contains_unraisable_errors() {
    let source = "def inner():\n    try:\n        yield 'ready'\n    finally:\n        print('inner-finally')\ndef outer():\n    try:\n        yield from inner()\n    finally:\n        print('outer-finally')\ndef bad():\n    try:\n        yield 'bad-ready'\n    finally:\n        print('bad-finally')\n        raise ValueError('unraisable')\ndef abandon():\n    delegated=outer()\n    broken=bad()\n    print(next(delegated),next(broken))\nabandon()\ni=0\nwhile i<40:\n    marker=[i]\n    i+=1\nprint('body-complete')";
    let program = compile(source, "delegated-generator-finalization").unwrap();
    for mode in [ExecutionMode::Interpreter, ExecutionMode::Jit] {
        let mut vm = Vm::new().unwrap();
        vm.execution_mode = mode;
        vm.gc_interval = Some(1);
        let mut out = Vec::new();
        vm.run(&program, &mut out).unwrap();
        assert_eq!(
            out,
            b"ready bad-ready\nbad-finally\ninner-finally\nouter-finally\nbody-complete\n"
        );
        assert_eq!(vm.stats.generator_finalizers, 2);
        assert_eq!(vm.stats.generator_finalizer_errors, 1);
        assert_eq!(vm.stats.unraisable_hook_calls, 1);
        assert_eq!(vm.stats.unraisable_hook_errors, 0);
    }
}

#[test]
fn user_finalizers_run_once_support_resurrection_and_keep_gc_errors_unraisable() {
    let source = "survivor=None\nclass Rescue:\n    def __del__(self):\n        global survivor\n        print('rescue-finalizer')\n        survivor=self\nclass Broken:\n    def __del__(self):\n        print('broken-finalizer')\n        raise ValueError('ignored')\nrescue=Rescue()\nbroken=Broken()\nrescue=None\nbroken=None\nprint(survivor is None)";
    let program = compile(source, "object-finalization").unwrap();
    for mode in [ExecutionMode::Interpreter, ExecutionMode::Jit] {
        let mut vm = Vm::new().unwrap();
        vm.execution_mode = mode;
        vm.gc_interval = None;
        let mut out = Vec::new();
        vm.run(&program, &mut out).unwrap();
        assert_eq!(out, b"True\n");
        vm.collect_garbage_with_output(&mut out).unwrap();
        assert_eq!(out, b"True\nbroken-finalizer\nrescue-finalizer\n");
        assert_eq!(vm.stats.object_finalizers, 2);
        assert_eq!(vm.stats.object_finalizer_errors, 1);
        assert_eq!(vm.stats.unraisable_hook_calls, 1);
        assert_eq!(vm.stats.unraisable_hook_errors, 0);

        // The surviving object was resurrected, but its finalizer is never
        // scheduled a second time while that root remains live.
        vm.collect_garbage().unwrap();
        assert_eq!(vm.stats.object_finalizers, 2);
    }
}

#[test]
fn user_unraisable_hook_receives_rooted_exception_metadata_and_hook_errors_are_contained() {
    let source = "import sys\ndef capture(args):\n    print(type(args).__name__,args.exc_type.__name__,str(args.exc_value),args.exc_traceback is not None,args.err_msg is None,args.object is Broken.__del__)\n    raise RuntimeError('hook failure')\nsys.unraisablehook=capture\nclass Broken:\n    def __del__(self):\n        raise ValueError('ignored')\nvalue=Broken()\nvalue=None\nprint('body-complete')";
    let program = compile(source, "unraisable-hook").unwrap();
    for mode in [ExecutionMode::Interpreter, ExecutionMode::Jit] {
        let mut vm = Vm::new().unwrap();
        vm.execution_mode = mode;
        vm.gc_interval = None;
        let mut out = Vec::new();
        vm.run(&program, &mut out).unwrap();
        assert_eq!(out, b"body-complete\n");
        // The hook-argument instance is allocated after the explicit collection.
        // Collect again before its first guest instruction to prove every field
        // is retained through `finalizer_roots` rather than native stack luck.
        vm.gc_interval = Some(1);
        vm.collect_garbage_with_output(&mut out).unwrap();
        assert_eq!(
            out,
            b"body-complete\nUnraisableHookArgs ValueError ignored True True True\n"
        );
        assert_eq!(vm.stats.object_finalizers, 1);
        assert_eq!(vm.stats.object_finalizer_errors, 1);
        assert_eq!(vm.stats.unraisable_hook_calls, 1);
        assert_eq!(vm.stats.unraisable_hook_errors, 1);
    }
}

#[test]
fn finalization_categories_run_generators_then_objects() {
    let source = "class Finalized:\n    def __del__(self):\n        print('object-finalizer')\ndef closing():\n    try:\n        yield 'ready'\n    finally:\n        print('generator-finalizer')\ndef abandon():\n    generator=closing()\n    next(generator)\n    value=Finalized()\nabandon()\nprint('body-complete')";
    let program = compile(source, "finalization-order").unwrap();
    for mode in [ExecutionMode::Interpreter, ExecutionMode::Jit] {
        let mut vm = Vm::new().unwrap();
        vm.execution_mode = mode;
        vm.gc_interval = None;
        let mut out = Vec::new();
        vm.run(&program, &mut out).unwrap();
        assert_eq!(out, b"body-complete\n");
        vm.collect_garbage_with_output(&mut out).unwrap();
        assert_eq!(
            out,
            b"body-complete\ngenerator-finalizer\nobject-finalizer\n"
        );
        assert_eq!(vm.stats.generator_finalizers, 1);
        assert_eq!(vm.stats.object_finalizers, 1);
    }
}

#[test]
fn coroutines_are_lazy_and_await_nested_tonic_coroutines() {
    let source = "async def inner(value):\n    print('inner',value)\n    return value+1\nasync def outer():\n    print('outer-start')\n    value=await inner(41)\n    print('outer-result',value)\n    return [value]\nprobe=outer()\nwrapper=probe.__await__()\nprint(type(probe).__name__,type(wrapper).__name__,iter(wrapper)==wrapper)\nprint('probe-close',wrapper.close())\nasync def instant():\n    return ['wrapped']\nwrapped=instant().__await__()\ntry:\n    next(wrapped)\nexcept StopIteration as error:\n    print('wrapped',error.value,error.args)\ntry:\n    next(wrapped)\nexcept RuntimeError as error:\n    print(str(error))\ncoroutine=outer()\ntry:\n    iter(coroutine)\nexcept TypeError:\n    print('not-iterable')\ntry:\n    coroutine.send('early')\nexcept TypeError:\n    print('send-before-start')\ntry:\n    coroutine.send(None)\nexcept StopIteration as error:\n    print('result',error.value,error.args)\ntry:\n    coroutine.send(None)\nexcept RuntimeError as error:\n    print(str(error))";
    assert_output_under_stress_gc_and_jit(
        source,
        b"coroutine coroutine_wrapper True\nprobe-close None\nwrapped ['wrapped'] (['wrapped'],)\ncannot reuse already awaited coroutine\nnot-iterable\nsend-before-start\nouter-start\ninner 41\nouter-result 42\nresult [42] ([42],)\ncannot reuse already awaited coroutine\n",
    );
}

#[test]
fn custom_awaitables_suspend_and_forward_send_throw_and_close() {
    let source = "class Pause:\n    def __init__(self,label):\n        self.label=label\n    def __await__(self):\n        try:\n            received=yield 'pause-'+self.label\n            print('received',self.label,received)\n            return 40\n        finally:\n            print('await-finally',self.label)\nasync def completed():\n    value=await Pause('complete')\n    return value+2\ncoroutine=completed()\nprint(coroutine.send(None))\ntry:\n    coroutine.send('resume')\nexcept StopIteration as error:\n    print('completed',error.value)\nasync def caught():\n    try:\n        await Pause('throw')\n    except ValueError as error:\n        return 'caught-'+str(error)\ncoroutine=caught()\nprint(coroutine.send(None))\ntry:\n    coroutine.throw(ValueError('boom'))\nexcept StopIteration as error:\n    print(error.value)\nasync def closing():\n    try:\n        await Pause('close')\n    finally:\n        print('outer-finally')\ncoroutine=closing()\nprint(coroutine.send(None),coroutine.close())\nasync def leaf():\n    return 5\nclass Proxy:\n    def __await__(self):\n        return leaf().__await__()\nasync def proxy():\n    return (await Proxy())+1\ntry:\n    proxy().send(None)\nexcept StopIteration as error:\n    print('proxy',error.value)\nprint('wrapper-list',list(leaf().__await__()))\nclass Invalid:\n    def __await__(self):\n        return []\nasync def invalid():\n    await Invalid()\ntry:\n    invalid().send(None)\nexcept TypeError:\n    print('invalid-awaitable')\nasync def invalid_value():\n    await 1\ntry:\n    invalid_value().send(None)\nexcept TypeError:\n    print('not-awaitable')\nasync def escaped_stop():\n    raise StopIteration('bad')\ntry:\n    escaped_stop().send(None)\nexcept RuntimeError as error:\n    print(str(error))";
    assert_output_under_stress_gc_and_jit(
        source,
        b"pause-complete\nreceived complete resume\nawait-finally complete\ncompleted 42\npause-throw\nawait-finally throw\ncaught-boom\nawait-finally close\nouter-finally\npause-close None\nproxy 6\nwrapper-list []\ninvalid-awaitable\nnot-awaitable\ncoroutine raised StopIteration\n",
    );
}

#[test]
fn unreachable_suspended_coroutines_close_awaited_iterators() {
    let source = "class Pause:\n    def __await__(self):\n        try:\n            yield 'paused'\n        finally:\n            print('await-finally')\nasync def outer():\n    try:\n        await Pause()\n    finally:\n        print('outer-finally')\ndef abandon():\n    coroutine=outer()\n    print(coroutine.send(None))\nabandon()\nprint('body-complete')";
    let program = compile(source, "coroutine-finalization").unwrap();
    for mode in [ExecutionMode::Interpreter, ExecutionMode::Jit] {
        let mut vm = Vm::new().unwrap();
        vm.execution_mode = mode;
        vm.gc_interval = None;
        let mut out = Vec::new();
        vm.run(&program, &mut out).unwrap();
        assert_eq!(out, b"paused\nbody-complete\n");
        vm.collect_garbage_with_output(&mut out).unwrap();
        assert_eq!(
            out,
            b"paused\nbody-complete\nawait-finally\nouter-finally\n"
        );
        assert_eq!(vm.stats.generator_finalizers, 1);
        assert_eq!(vm.stats.generator_finalizer_errors, 0);
    }
}

#[test]
fn asyncio_runs_tasks_futures_callbacks_and_timers() {
    let source = r#"import asyncio
def completed(future):
    print('callback',future.done(),future.result())
async def producer(future):
    await asyncio.sleep(0)
    future.set_result(42)
    return 'producer'
async def worker(name):
    print('start',name,type(asyncio.current_task()).__name__)
    value=await asyncio.sleep(0,name)
    print('end',name)
    return value
async def main():
    print(type(asyncio.get_running_loop()).__name__)
    future=asyncio.Future()
    future.add_done_callback(completed)
    producer_task=asyncio.create_task(producer(future))
    first=asyncio.create_task(worker('a'))
    second=asyncio.create_task(worker('b'))
    print(type(first).__name__,first.done(),await future,await producer_task)
    print(await first,await second)
    return 'ok'
print(asyncio.run(main()))"#;
    assert_output_under_stress_gc_and_jit(
        source,
        b"EventLoop\nstart a Task\nstart b Task\nend a\nend b\ncallback True 42\nTask False 42 producer\na b\nok\n",
    );
}

#[test]
fn asyncio_cancellation_and_failure_propagate_through_task_protocol() {
    let source = r#"import asyncio
async def cancelled_worker():
    try:
        await asyncio.sleep(5)
    except asyncio.CancelledError:
        print('worker-cancelled')
        raise
async def failed_worker():
    await asyncio.sleep(0)
    raise ValueError('failed')
async def main():
    cancelled=asyncio.create_task(cancelled_worker())
    failed=asyncio.create_task(failed_worker())
    await asyncio.sleep(0)
    print('cancel-request',cancelled.cancel())
    try:
        await cancelled
    except asyncio.CancelledError:
        print('cancel-state',cancelled.done(),cancelled.cancelled())
    try:
        await failed
    except ValueError as error:
        print('failure',str(error),type(failed.exception()).__name__)
    return 'done'
print(asyncio.run(main()))"#;
    assert_output_under_stress_gc_and_jit(
        source,
        b"cancel-request True\nworker-cancelled\ncancel-state True True\nfailure failed ValueError\ndone\n",
    );
}

#[test]
fn asyncio_cancellation_preempts_stale_wakes_and_can_be_requested_again() {
    let source = r#"import asyncio
async def worker():
    try:
        await asyncio.sleep(1)
    except asyncio.CancelledError:
        print('caught-first')
    await asyncio.sleep(1)
async def main():
    task=asyncio.create_task(worker())
    await asyncio.sleep(0)
    print('first',task.cancel())
    await asyncio.sleep(0)
    print('second',task.cancel())
    try:
        await task
    except asyncio.CancelledError:
        print('done',task.done(),task.cancelled())
asyncio.run(main())"#;
    assert_output_under_stress_gc_and_jit(
        source,
        b"first True\ncaught-first\nsecond True\ndone True True\n",
    );
}

#[test]
fn asyncio_rejects_invalid_state_and_nested_event_loops() {
    let source = r#"import asyncio
async def inner():
    return 1
async def main():
    future=asyncio.Future()
    for method in [future.result,future.exception]:
        try:
            method()
        except asyncio.InvalidStateError:
            print('pending')
    future.set_result(7)
    try:
        future.set_result(8)
    except asyncio.InvalidStateError:
        print('already-done')
    nested=inner()
    try:
        asyncio.run(nested)
    except RuntimeError as error:
        print(str(error))
        nested.close()
    return await future
print(asyncio.run(main()))"#;
    assert_output_under_stress_gc_and_jit(
        source,
        b"pending\npending\nalready-done\nasyncio.run cannot be called from a running event loop\n7\n",
    );
}

#[test]
fn async_generators_support_iteration_asend_and_suspended_awaits() {
    let source = r#"class Pause:
    def __init__(self,value):
        self.value=value
    def __await__(self):
        yield 'pause-'+str(self.value)
        return self.value
async def values():
    first=yield 1
    print('received-first',first)
    second=await Pause(2)
    received=yield second
    print('received-second',received)
generator=values()
print(type(generator).__name__,generator.__aiter__()==generator)
first=generator.__anext__()
print(type(first).__name__,iter(first)==first)
try:
    first.send(None)
except StopIteration as error:
    print('first',error.value)
try:
    first.send(None)
except RuntimeError as error:
    print(str(error))
second=generator.asend('sent')
print(second.send(None))
try:
    second.send(None)
except StopIteration as error:
    print('second',error.value)
try:
    generator.asend('done').send(None)
except StopAsyncIteration:
    print('manual-done')
fresh=values()
try:
    fresh.asend('early').send(None)
except TypeError as error:
    print(str(error))
async def collect():
    result=[]
    async for value in values():
        result=result+[value]
    return result
coroutine=collect()
print('collect-pause',coroutine.send(None))
try:
    coroutine.send(None)
except StopIteration as error:
    print('collected',error.value)"#;
    assert_output_under_stress_gc_and_jit(
        source,
        b"async_generator True\nasync_generator_asend True\nfirst 1\ncannot reuse already awaited async-generator awaitable\nreceived-first sent\npause-2\nsecond 2\nreceived-second done\nmanual-done\ncannot send non-None value to a just-started generator\nreceived-first None\ncollect-pause pause-2\nreceived-second None\ncollected [1, 2]\n",
    );
}

#[test]
fn async_generator_athrow_aclose_and_exception_boundaries_are_exact() {
    let source = r#"async def guarded():
    try:
        yield 'ready'
    except ValueError as error:
        yield 'caught-'+str(error)
    finally:
        print('guarded-finally')
async def run():
    generator=guarded()
    print(await generator.__anext__())
    print(await generator.athrow(ValueError('boom')))
    print('closed',await generator.aclose())
    return 'done'
try:
    run().send(None)
except StopIteration as error:
    print(error.value)
async def ignores_close():
    try:
        yield 1
    except GeneratorExit:
        yield 2
bad=ignores_close()
try:
    bad.__anext__().send(None)
except StopIteration as error:
    print('bad-first',error.value)
try:
    bad.aclose().send(None)
except RuntimeError as error:
    print(str(error))
async def escaped():
    yield 'start'
    raise StopAsyncIteration('bad')
escaped_generator=escaped()
try:
    escaped_generator.__anext__().send(None)
except StopIteration as error:
    print(error.value)
try:
    escaped_generator.__anext__().send(None)
except RuntimeError as error:
    print(str(error))
class CustomError(Exception):
    def __init__(self,value):
        print('custom-init',value)
async def catches_custom():
    try:
        yield 'custom-ready'
    except CustomError:
        yield 'custom-caught'
custom=catches_custom()
try:
    custom.__anext__().send(None)
except StopIteration as error:
    print(error.value)
try:
    custom.athrow(CustomError,'value').send(None)
except StopIteration as error:
    print(error.value)"#;
    assert_output_under_stress_gc_and_jit(
        source,
        b"ready\ncaught-boom\nguarded-finally\nclosed None\ndone\nbad-first 1\nasync generator ignored GeneratorExit\nstart\nasync generator raised StopAsyncIteration\ncustom-ready\ncustom-init value\ncustom-caught\n",
    );
}

#[test]
fn async_generator_awaitables_forward_throw_and_close_through_await() {
    let source = r#"class Pause:
    def __await__(self):
        try:
            yield 'paused'
        finally:
            print('pause-finally')
async def catches():
    try:
        await Pause()
    except ValueError as error:
        yield 'caught-'+str(error)
async def outer_throw():
    return await catches().__anext__()
coroutine=outer_throw()
print(coroutine.send(None))
try:
    coroutine.throw(ValueError('boom'))
except StopIteration as error:
    print(error.value)
async def closing():
    try:
        await Pause()
        yield 'unreachable'
    finally:
        print('generator-finally')
async def outer_close():
    return await closing().__anext__()
coroutine=outer_close()
print(coroutine.send(None))
print('close-result',coroutine.close())
async def direct():
    try:
        await Pause()
    except CustomError:
        yield 'direct-caught'
class CustomError(Exception):
    def __init__(self,value):
        print('direct-init',value)
awaitable=direct().__anext__()
print(awaitable.send(None))
try:
    awaitable.throw(CustomError,'value')
except StopIteration as error:
    print(error.value)
async def blocked():
    await Pause()
    yield 'released'
blocked_generator=blocked()
owner=blocked_generator.__anext__()
print(owner.send(None))
contender=blocked_generator.__anext__()
try:
    contender.send(None)
except RuntimeError as error:
    print(str(error))
try:
    owner.send(None)
except StopIteration as error:
    print(error.value)"#;
    assert_output_under_stress_gc_and_jit(
        source,
        b"paused\npause-finally\ncaught-boom\npaused\npause-finally\ngenerator-finally\nclose-result None\npaused\ndirect-init value\npause-finally\ndirect-caught\npaused\nasynchronous generator is already running\npause-finally\nreleased\n",
    );
}

#[test]
fn async_for_awaits_anext_and_preserves_loop_control() {
    let source = "class Counter:\n    def __init__(self,limit):\n        self.i=0\n        self.limit=limit\n    def __aiter__(self):\n        print('aiter',self.limit)\n        return self\n    async def __anext__(self):\n        if self.i>=self.limit:\n            raise StopAsyncIteration\n        value=self.i\n        self.i+=1\n        return value\nasync def consume(limit,stop):\n    total=0\n    async for value in Counter(limit):\n        if value==1:\n            continue\n        if value==stop:\n            break\n        total+=value\n    else:\n        print('exhausted',limit)\n    return total\nfor limit,stop in [(4,9),(5,3)]:\n    coroutine=consume(limit,stop)\n    try:\n        coroutine.send(None)\n    except StopIteration as error:\n        print('result',error.value)\nclass Step:\n    def __init__(self,value):\n        self.value=value\n    def __await__(self):\n        yield 'pause-'+str(self.value)\n        return self.value\nclass Suspended:\n    def __init__(self):\n        self.i=0\n    def __aiter__(self):\n        return self\n    def __anext__(self):\n        if self.i>=2:\n            raise StopAsyncIteration\n        value=self.i\n        self.i+=1\n        return Step(value)\nasync def suspended_sum():\n    total=0\n    async for value in Suspended():\n        total+=value\n    else:\n        print('suspended-exhausted')\n    return total\ncoroutine=suspended_sum()\nprint(coroutine.send(None))\nprint(coroutine.send(None))\ntry:\n    coroutine.send(None)\nexcept StopIteration as error:\n    print('suspended-result',error.value)\nclass BadIter:\n    def __aiter__(self):\n        return 1\nclass BadNext:\n    def __aiter__(self):\n        return self\n    def __anext__(self):\n        return 1\nasync def invalid(value):\n    async for item in value:\n        pass\nfor value in [BadIter(),BadNext(),1]:\n    try:\n        invalid(value).send(None)\n    except TypeError as error:\n        print(type(error).__name__)";
    assert_output_under_stress_gc_and_jit(
        source,
        b"aiter 4\nexhausted 4\nresult 5\naiter 5\nresult 2\npause-0\npause-1\nsuspended-exhausted\nsuspended-result 1\nTypeError\nTypeError\nTypeError\n",
    );
}

#[test]
fn async_with_awaits_protocol_and_preserves_unwind_semantics() {
    let source = r#"class Manager:
    def __init__(self,name,suppress=False):
        self.name=name
        self.suppress=suppress
    async def __aenter__(self):
        print('enter',self.name)
        return self.name+'-value'
    async def __aexit__(self,kind,value,traceback):
        print('exit',self.name,kind.__name__ if kind else 'None')
        return self.suppress
async def normal():
    async with Manager('outer') as outer, Manager('inner') as inner:
        print(outer,inner)
    try:
        async with Manager('propagate'):
            raise ValueError('boom')
    except ValueError:
        print('propagated')
    async with Manager('suppress',True):
        raise LookupError('hidden')
    print('suppressed')
    return 7
try:
    normal().send(None)
except StopIteration as error:
    print('normal-result',error.value)
async def leave(mode):
    for i in range(2):
        async with Manager('loop'+str(i)):
            if i==0:
                continue
            break
    async with Manager('return'):
        return mode
try:
    leave(9).send(None)
except StopIteration as error:
    print('leave-result',error.value)
class Pause:
    def __init__(self,label,value):
        self.label=label
        self.value=value
    def __await__(self):
        yield self.label
        return self.value
class SuspendedManager:
    def __aenter__(self):
        return Pause('enter-pause','entered')
    def __aexit__(self,kind,value,traceback):
        return Pause('exit-pause',False)
async def suspended():
    async with SuspendedManager() as value:
        print(value)
        return 'done'
coroutine=suspended()
print(coroutine.send(None))
print(coroutine.send(None))
try:
    coroutine.send(None)
except StopIteration as error:
    print('suspended-result',error.value)
def old_exit(self,kind,value,traceback):
    return Pause('captured-exit',False)
class Mutating:
    __aexit__=old_exit
    def __aenter__(self):
        Mutating.__aexit__=lambda self,kind,value,traceback: Pause('new-exit',False)
        return Pause('mutating-enter',self)
async def captured():
    async with Mutating():
        print('captured-body')
coroutine=captured()
print(coroutine.send(None))
print(coroutine.send(None))
try:
    coroutine.send(None)
except StopIteration:
    print('captured-done')
class TargetManager:
    async def __aenter__(self):
        return [1]
    async def __aexit__(self,kind,value,traceback):
        print('target-exit',kind.__name__)
        return False
async def target_error():
    try:
        async with TargetManager() as (first,second):
            pass
    except ValueError:
        print('target-error')
try:
    target_error().send(None)
except StopIteration:
    pass
class Reraising:
    async def __aenter__(self):
        return self
    async def __aexit__(self,kind,value,traceback):
        print('bare-exit')
        raise
async def reraising():
    try:
        async with Reraising():
            raise KeyError('same')
    except KeyError:
        print('bare-reraised')
try:
    reraising().send(None)
except StopIteration:
    pass
class EnterFails:
    async def __aenter__(self):
        print('enter-fails')
        raise ValueError('enter')
    async def __aexit__(self,kind,value,traceback):
        print('must-not-exit')
async def failed_enter():
    try:
        async with EnterFails():
            pass
    except ValueError:
        print('enter-failure-kept')
try:
    failed_enter().send(None)
except StopIteration:
    pass
class InnerFails:
    async def __aenter__(self):
        print('inner-enter-fails')
        raise ValueError('inner')
    async def __aexit__(self,kind,value,traceback):
        print('inner-must-not-exit')
async def partial_enter():
    try:
        async with Manager('partial-outer'), InnerFails():
            pass
    except ValueError:
        print('partial-failure-kept')
try:
    partial_enter().send(None)
except StopIteration:
    pass
class Truth:
    def __bool__(self):
        print('truth')
        return True
class TruthManager(Manager):
    async def __aexit__(self,kind,value,traceback):
        print('truth-exit',kind.__name__)
        return Truth()
async def truth_suppression():
    async with TruthManager('truth-manager'):
        raise TypeError('hidden')
try:
    truth_suppression().send(None)
except StopIteration:
    print('truth-suppressed')
class Meta(type):
    async def __aenter__(cls):
        print('meta-enter')
        return cls.__name__
    async def __aexit__(cls,kind,value,traceback):
        print('meta-exit',kind.__name__ if kind else 'None')
class ManagedClass(metaclass=Meta):
    pass
async def managed_class():
    async with ManagedClass as name:
        print(name)
try:
    managed_class().send(None)
except StopIteration:
    pass
class BadEnter:
    def __aenter__(self):
        return 1
    async def __aexit__(self,kind,value,traceback):
        pass
class MissingExit:
    async def __aenter__(self):
        pass
class BadExit:
    async def __aenter__(self):
        pass
    def __aexit__(self,kind,value,traceback):
        return 1
async def invalid(manager):
    async with manager:
        pass
for manager in [BadEnter(),MissingExit(),BadExit(),1]:
    try:
        invalid(manager).send(None)
    except TypeError:
        print('invalid')
"#;
    assert_output_under_stress_gc_and_jit(
        source,
        b"enter outer\nenter inner\nouter-value inner-value\nexit inner None\nexit outer None\nenter propagate\nexit propagate ValueError\npropagated\nenter suppress\nexit suppress LookupError\nsuppressed\nnormal-result 7\nenter loop0\nexit loop0 None\nenter loop1\nexit loop1 None\nenter return\nexit return None\nleave-result 9\nenter-pause\nentered\nexit-pause\nsuspended-result done\nmutating-enter\ncaptured-body\ncaptured-exit\ncaptured-done\ntarget-exit ValueError\ntarget-error\nbare-exit\nbare-reraised\nenter-fails\nenter-failure-kept\nenter partial-outer\ninner-enter-fails\nexit partial-outer ValueError\npartial-failure-kept\nenter truth-manager\ntruth-exit TypeError\ntruth\ntruth-suppressed\nmeta-enter\nManagedClass\nmeta-exit None\ninvalid\ninvalid\ninvalid\ninvalid\n",
    );
}
#[test]
fn exception_objects_and_explicit_raise() {
    assert_eq!(
        output("print(type(ValueError('x')).__name__,isinstance(ValueError(),Exception),issubclass(TypeError,BaseException),str(RuntimeError('bad')))"),
        "ValueError True True bad\n"
    );
    for (source, kind, message) in [
        ("raise ValueError('boom')", "ValueError", "boom"),
        ("raise 1", "TypeError", "exceptions must derive"),
        ("raise", "RuntimeError", "no active exception"),
        (
            "class MyError(Exception):\n    pass\nraise MyError()",
            "MyError",
            "",
        ),
    ] {
        let error = error(source);
        assert_eq!(error.kind, kind);
        assert!(error.message.contains(message));
        assert!(error.span.is_some());
        assert_eq!(error.trace[0].0, "<module>");
    }
    let program = compile(
        "def maybe(flag):\n    if flag:\n        raise ValueError('jit-safe')\n    return 1\ni=0\nwhile i<20:\n    maybe(False)\n    i+=1\nmaybe(True)",
        "raise-jit",
    )
    .unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.jit_threshold = 1;
    vm.jit_min_instructions = 0;
    let error = vm.run(&program, &mut Vec::new()).unwrap_err();
    assert_eq!(error.kind, "ValueError");
    assert!(vm.stats.jit_compile_attempts >= 1);
    assert_eq!(vm.stats.jit_compiled, 0);

    let program = compile(
        "def divide(a,b):\n    return a/b\ni=0\nwhile i<20:\n    divide(20,2)\n    i+=1\ntry:\n    divide(1,0)\nexcept ZeroDivisionError as error:\n    print(type(error).__name__)",
        "jit-exception-unwind",
    )
    .unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.jit_threshold = 1;
    vm.jit_min_instructions = 0;
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"ZeroDivisionError\n");
    assert!(vm.stats.jit_compiled >= 1);
    assert!(vm.stats.jit_runtime_errors >= 1);
}
#[test]
fn try_except_matches_unwinds_reraises_and_clears_binding() {
    assert_eq!(
        output(
            "def fail(kind):\n    scratch=0.0\n    for i in range(20):\n        scratch+=0.5\n    if kind==0:\n        int('bad')\n    if kind==1:\n        return 1//0\n    raise KeyError('key')\ntry:\n    fail(0)\nexcept TypeError:\n    print('wrong')\nexcept (ValueError, LookupError) as error:\n    print('caught',type(error).__name__,isinstance(error,Exception))\ntry:\n    print(error)\nexcept NameError:\n    print('cleared')\ntry:\n    try:\n        fail(1)\n    except ArithmeticError:\n        raise\nexcept ZeroDivisionError:\n    print('reraised')\ntry:\n    print('body')\nexcept Exception:\n    print('bad')\nelse:\n    print('else')\ntry:\n    fail(2)\nexcept:\n    print('bare')"
        ),
        "caught ValueError True\ncleared\nreraised\nbody\nelse\nbare\n"
    );
    let error = error("try:\n    int('bad')\nexcept 1:\n    pass");
    assert_eq!(error.kind, "TypeError");

    let program = compile(
        "def guarded(value):\n    try:\n        if value:\n            raise ValueError('guarded')\n        return 1\n    except ValueError:\n        return 2\ni=0\nwhile i<20:\n    guarded(False)\n    i+=1\nprint(guarded(True))",
        "try-jit",
    )
    .unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.jit_threshold = 1;
    vm.jit_min_instructions = 0;
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"2\n");
    assert!(vm.stats.jit_compile_attempts >= 1);
    assert_eq!(vm.stats.jit_compiled, 0);
}

#[test]
fn exception_context_survives_nesting_and_cleans_every_exit_path() {
    assert_eq!(
        output(
            "try:\n    raise ValueError('outer')\nexcept ValueError as outer:\n    try:\n        raise TypeError('inner')\n    except TypeError as inner:\n        pass\n    try:\n        print(inner)\n    except NameError:\n        print('inner-cleared')\n    try:\n        raise\n    except ValueError:\n        print('outer-restored')\ntry:\n    print(outer)\nexcept NameError:\n    print('outer-cleared')\ntry:\n    try:\n        raise ValueError('first')\n    except ValueError as failed:\n        raise TypeError('second')\nexcept TypeError:\n    print('handler-error')\ntry:\n    print(failed)\nexcept NameError:\n    print('failed-cleared')\nfor mode in range(2):\n    try:\n        raise LookupError('loop')\n    except LookupError as loop_error:\n        if mode==0:\n            continue\n        break\ntry:\n    print(loop_error)\nexcept NameError:\n    print('loop-cleared')\ndef leave():\n    try:\n        raise ValueError('return')\n    except ValueError as returned:\n        return 7\nprint(leave())\ntry:\n    raise\nexcept RuntimeError:\n    print('no-active')"
        ),
        "inner-cleared\nouter-restored\nouter-cleared\nhandler-error\nfailed-cleared\nloop-cleared\n7\nno-active\n"
    );
}

#[test]
fn exception_chaining_exposes_cause_context_and_suppression() {
    assert_eq!(
        output(
            "class PlainError(Exception):\n    pass\nprint(PlainError(1,'two').args,str(PlainError(1,'two')))\nclass MyError(Exception):\n    def __init__(self,message):\n        self.label=message\ntry:\n    try:\n        raise KeyError('context')\n    except KeyError:\n        raise MyError('outer') from ValueError('cause')\nexcept MyError as error:\n    print(error.label,error.args,error.__traceback__==None)\n    print(type(error.__cause__).__name__,type(error.__context__).__name__,error.__suppress_context__)\ntry:\n    try:\n        raise ValueError('implicit')\n    except ValueError:\n        raise TypeError('replacement')\nexcept TypeError as error:\n    print(error.__cause__,type(error.__context__).__name__,error.__suppress_context__)\ntry:\n    try:\n        raise LookupError('hidden')\n    except LookupError:\n        raise RuntimeError('clean') from None\nexcept RuntimeError as error:\n    print(error.__cause__,type(error.__context__).__name__,error.__suppress_context__)\nclass Manager:\n    def __enter__(self):\n        return self\n    def __exit__(self,kind,value,traceback):\n        print(type(value).__name__,traceback==None)\ntry:\n    with Manager():\n        raise ValueError('managed')\nexcept ValueError:\n    pass"
        ),
        "(1, 'two') (1, 'two')\nouter ('outer',) False\nValueError KeyError True\nNone ValueError False\nNone LookupError True\nValueError False\n"
    );
    let error = error("raise TypeError('outer') from 1");
    assert_eq!(error.kind, "TypeError");
    assert!(error.message.contains("exception causes must derive"));
}

#[test]
fn finally_runs_once_for_normal_exception_and_structural_exits() {
    assert_eq!(
        output(
            "try:\n    print('body')\nfinally:\n    print('normal-final')\ntry:\n    try:\n        raise ValueError('boom')\n    finally:\n        print('exception-final')\nexcept ValueError:\n    print('exception-kept')\ntry:\n    raise ValueError('handled')\nexcept ValueError:\n    print('handled')\nelse:\n    print('bad-else')\nfinally:\n    print('handler-final')\ndef leave(mode):\n    try:\n        if mode==0:\n            return 10\n        return 20\n    finally:\n        print('return-final',mode)\nprint(leave(0),leave(1))\ndef override():\n    try:\n        return 1\n    finally:\n        return 2\nprint('override',override())\nfor i in range(3):\n    try:\n        if i==0:\n            continue\n        break\n    finally:\n        print('loop-final',i)\ntry:\n    def fail_return():\n        try:\n            return 1\n        finally:\n            print('raising-final')\n            raise TypeError('override')\n    fail_return()\nexcept TypeError:\n    print('return-overridden')\ntry:\n    try:\n        raise ValueError('active')\n    finally:\n        try:\n            raise\n        except ValueError:\n            print('active-in-final')\nexcept ValueError:\n    print('reraised-after-final')\ntry:\n    try:\n        raise ValueError('old')\n    finally:\n        raise TypeError('new')\nexcept TypeError:\n    print('exception-overridden')"
        ),
        "body\nnormal-final\nexception-final\nexception-kept\nhandled\nhandler-final\nreturn-final 0\nreturn-final 1\n10 20\noverride 2\nloop-final 0\nloop-final 1\nraising-final\nreturn-overridden\nactive-in-final\nreraised-after-final\nexception-overridden\n"
    );
}

#[test]
fn with_calls_captured_exit_in_nested_unwind_order() {
    assert_eq!(
        output(
            "class Manager:\n    def __init__(self,name,suppress=False):\n        self.name=name\n        self.suppress=suppress\n    def __enter__(self):\n        print('enter',self.name)\n        return self.name+'-value'\n    def __exit__(self,kind,value,traceback):\n        print('exit',self.name,kind.__name__ if kind else 'None')\n        return self.suppress\nwith Manager('normal') as value:\n    print(value)\nwith Manager('outer') as outer, Manager('inner') as inner:\n    print(outer,inner)\ntry:\n    with Manager('propagate'):\n        raise ValueError('boom')\nexcept ValueError:\n    print('propagated')\nwith Manager('suppress',True):\n    raise LookupError('hidden')\nprint('suppressed')\ndef leave():\n    with Manager('return'):\n        return 7\nprint(leave())\nfor i in range(2):\n    with Manager('loop'+str(i)):\n        if i==0:\n            continue\n        break\nclass Truth:\n    def __bool__(self):\n        print('truth')\n        return True\nclass TruthManager(Manager):\n    def __exit__(self,kind,value,traceback):\n        print('truth-exit',kind.__name__)\n        return Truth()\nwith TruthManager('truth-manager'):\n    raise TypeError('hidden')\ndef old_exit(self,kind,value,traceback):\n    print('captured-old')\nclass Mutating:\n    __exit__=old_exit\n    def __enter__(self):\n        Mutating.__exit__=lambda self,kind,value,traceback: print('new')\n        return self\nwith Mutating():\n    pass\nclass TargetManager(Manager):\n    def __enter__(self):\n        return [1]\ntry:\n    with TargetManager('target') as (a,b):\n        pass\nexcept ValueError:\n    print('target-error')\nclass Meta(type):\n    def __enter__(cls):\n        print('meta-enter')\n        return cls.__name__\n    def __exit__(cls,kind,value,traceback):\n        print('meta-exit',kind.__name__ if kind else 'None')\nclass ManagedClass(metaclass=Meta):\n    pass\nwith ManagedClass as class_name:\n    print(class_name)\nclass Reraising(Manager):\n    def __exit__(self,kind,value,traceback):\n        print('bare-exit')\n        raise\ntry:\n    with Reraising('reraising'):\n        raise KeyError('same')\nexcept KeyError:\n    print('bare-reraised')\nclass RaisingExit(Manager):\n    def __exit__(self,kind,value,traceback):\n        print('raising-exit',kind.__name__ if kind else 'None')\n        raise TypeError('new')\ntry:\n    with Manager('exit-outer'):\n        with RaisingExit('exit-inner'):\n            pass\nexcept TypeError:\n    print('exit-replaced')"
        ),
        "enter normal\nnormal-value\nexit normal None\nenter outer\nenter inner\nouter-value inner-value\nexit inner None\nexit outer None\nenter propagate\nexit propagate ValueError\npropagated\nenter suppress\nexit suppress LookupError\nsuppressed\nenter return\nexit return None\n7\nenter loop0\nexit loop0 None\nenter loop1\nexit loop1 None\nenter truth-manager\ntruth-exit TypeError\ntruth\ncaptured-old\nexit target ValueError\ntarget-error\nmeta-enter\nManagedClass\nmeta-exit None\nenter reraising\nbare-exit\nbare-reraised\nenter exit-outer\nenter exit-inner\nraising-exit None\nexit exit-outer TypeError\nexit-replaced\n"
    );
    for source in [
        "class MissingEnter:\n    def __exit__(self,a,b,c):\n        pass\nwith MissingEnter():\n    pass",
        "class MissingExit:\n    def __enter__(self):\n        pass\nwith MissingExit():\n    pass",
    ] {
        assert_eq!(error(source).kind, "TypeError");
    }
}

#[test]
fn custom_iteration_consumes_only_escaping_stop_iteration() {
    assert_eq!(
        output(
            "class Counter:\n    def __init__(self,n):\n        self.i=0\n        self.n=n\n    def __iter__(self):\n        return self\n    def stop(self):\n        raise StopIteration\n    def __next__(self):\n        scratch=0.0\n        for i in range(20):\n            scratch+=0.5\n        if self.i>=self.n:\n            self.stop()\n        value=self.i\n        self.i+=1\n        return value\nfor value in Counter(4):\n    print(value)\nelse:\n    print('done')\nclass Recover:\n    def __init__(self):\n        self.first=True\n    def __iter__(self):\n        return self\n    def __next__(self):\n        if self.first:\n            self.first=False\n            try:\n                raise StopIteration\n            except StopIteration:\n                return 9\n        raise StopIteration\nfor value in Recover():\n    print('recovered',value)\nclass Failing:\n    def __iter__(self):\n        return self\n    def __next__(self):\n        raise ValueError('iteration failed')\ntry:\n    for value in Failing():\n        pass\nexcept ValueError as error:\n    print(type(error).__name__)"
        ),
        "0\n1\n2\n3\ndone\nrecovered 9\nValueError\n"
    );
    for source in [
        "class Bad:\n    def __iter__(self):\n        return 1\nfor value in Bad():\n    pass",
        "class Bad:\n    def __iter__(self):\n        return self\nfor value in Bad():\n    pass",
    ] {
        assert_eq!(error(source).kind, "TypeError");
    }
}

#[test]
fn sum_streams_iterables_and_suspends_numeric_protocols() {
    let source = r#"print(sum([1,2,3]),sum((),start=7),sum(range(5)),sum([],start=None))
print(sum([9223372036854775807,1,2]))
print(sum([1e16,1.0,-1e16]),sum([1.5,2,3.25],start=0.25))
class IntChild(int):
    def __radd__(self,other):
        print('int-child-radd')
        return 99
class FloatChild(float):
    def __radd__(self,other):
        print('float-child-radd')
        return 88.0
print(sum([1.0,IntChild(2)]))
print(sum([1.0,FloatChild(2.0)]))
print(sum([[1],[2,3]],[]))
def values():
    print('generator-start')
    yield 4
    print('generator-middle')
    yield 5
print(sum(values(),start=6))
class Number:
    def __init__(self,value): self.value=value
    def __add__(self,other):
        scratch=0.0
        for i in range(20): scratch+=0.5
        print('add',self.value,other.value)
        return Number(self.value+other.value)
    def __radd__(self,other):
        scratch=0.0
        for i in range(20): scratch+=0.5
        print('radd',other,self.value)
        return Number(other+self.value)
result=sum([Number(2),Number(3)])
print('number',result.value)
class Base:
    def __init__(self,value): self.value=value
    def __add__(self,other):
        print('base-add')
        return Number(self.value+other.value)
class Child(Base):
    def __radd__(self,other):
        print('child-radd')
        return Number(other.value+self.value)
result=sum([Child(2)],Base(5))
print('strict',result.value)
class Counter:
    def __init__(self,limit): self.i=0; self.limit=limit
    def __iter__(self):
        print('counter-iter')
        return self
    def __next__(self):
        scratch=0.0
        for j in range(20): scratch+=0.5
        if self.i>=self.limit: raise StopIteration
        value=self.i
        self.i+=1
        print('counter-next',value)
        return value
print('counter-total',sum(Counter(4),start=10))
def hot(items):
    total=0
    for i in range(30): total+=i
    return sum(items,start=total)
print('hot',hot([1,2,3]))
class Mark:
    def __iter__(self):
        print('iter-before-start-check')
        return iter([])
try:
    sum(Mark(),'')
except TypeError:
    print('string-start')
class StopAdd:
    def __radd__(self,other): raise StopIteration('from-add')
try:
    sum([StopAdd()])
except StopIteration:
    print('add-stop-escaped')
class BadNext:
    def __iter__(self): return self
    def __next__(self): raise ValueError('from-next')
try:
    sum(BadNext())
except ValueError:
    print('next-error-escaped')"#;
    assert_output_under_stress_gc_and_jit(
        source,
        b"6 7 10 None\n9223372036854775810\n1.0 7.0\n3.0\nfloat-child-radd\n88.0\n[1, 2, 3]\ngenerator-start\ngenerator-middle\n15\nradd 0 2\nadd 2 3\nnumber 5\nchild-radd\nstrict 7\ncounter-iter\ncounter-next 0\ncounter-next 1\ncounter-next 2\ncounter-next 3\ncounter-total 16\nhot 441\niter-before-start-check\nstring-start\nadd-stop-escaped\nnext-error-escaped\n",
    );

    for (source, kind) in [
        ("sum()", "TypeError"),
        ("sum([1],2,3)", "TypeError"),
        ("sum(iterable=[1])", "TypeError"),
        ("sum([1],2,start=3)", "TypeError"),
        ("sum([1],unknown=3)", "TypeError"),
        ("sum(1)", "TypeError"),
        ("sum([], '')", "TypeError"),
        ("sum(['x'])", "TypeError"),
        (
            "class Bad:\n    def __iter__(self): return 1\nsum(Bad())",
            "TypeError",
        ),
        (
            "class Bad:\n    def __iter__(self): return self\n    def __next__(self): raise ValueError('next')\nsum(Bad())",
            "ValueError",
        ),
        (
            "class Bad:\n    def __radd__(self,other): raise ValueError('add')\nsum([Bad()])",
            "ValueError",
        ),
        (
            "class Bad:\n    def __radd__(self,other): return NotImplemented\nsum([Bad()])",
            "TypeError",
        ),
    ] {
        assert_eq!(error(source).kind, kind, "{source}");
    }
}

#[test]
fn any_and_all_stream_iterables_and_suspend_truth_protocols() {
    let source = r#"print(any([]),all([]))
print(any([0,'',None,3]),all([1,'x',[0]]))
def values():
    print('generator-start')
    yield 0
    print('generator-middle')
    yield 4
    print('generator-unreached')
    yield 5
print('generator-any',any(values()))
class Truth:
    def __init__(self,name,value): self.name=name; self.value=value
    def __bool__(self):
        scratch=0.0
        for i in range(20): scratch+=0.5
        print('bool',self.name)
        return self.value
print('truth-any',any([Truth('a',False),Truth('b',True),Truth('c',True)]))
print('truth-all',all([Truth('d',True),Truth('e',False),Truth('f',True)]))
class Index:
    def __init__(self,value): self.value=value
    def __index__(self):
        print('index',self.value)
        return self.value
class Length:
    def __init__(self,name,value): self.name=name; self.value=value
    def __len__(self):
        print('len',self.name)
        return Index(self.value)
print('length-any',any([Length('zero',0),Length('two',2)]))
print('length-all',all([Length('one',1),Length('zero-again',0),Length('unreached',1)]))
class Counter:
    def __init__(self,start,limit): self.value=start; self.limit=limit
    def __iter__(self):
        print('counter-iter',self.value,self.limit)
        return self
    def __next__(self):
        scratch=0.0
        for i in range(20): scratch+=0.5
        if self.value>=self.limit: raise StopIteration
        value=self.value
        self.value+=1
        print('counter-next',value)
        return value
print('counter-any',any(Counter(0,4)))
print('counter-all',all(Counter(1,4)))
def hot(items,use_any):
    total=0
    for i in range(30): total+=i
    return any(items) if use_any else all(items)
print('hot',hot([0,0,1],True),hot([1,1,0],False))
print('large',any([False for i in range(5000)]),all([True for i in range(5000)]))
class StopTruth:
    def __bool__(self): raise StopIteration('truth-stop')
try:
    any([StopTruth()])
except StopIteration:
    print('truth-stop-escaped')
class BadNext:
    def __iter__(self): return self
    def __next__(self): raise ValueError('next-error')
try:
    all(BadNext())
except ValueError:
    print('next-error-escaped')"#;
    assert_output_under_stress_gc_and_jit(
        source,
        b"False True\nTrue True\ngenerator-start\ngenerator-middle\ngenerator-any True\nbool a\nbool b\ntruth-any True\nbool d\nbool e\ntruth-all False\nlen zero\nindex 0\nlen two\nindex 2\nlength-any True\nlen one\nindex 1\nlen zero-again\nindex 0\nlength-all False\ncounter-iter 0 4\ncounter-next 0\ncounter-next 1\ncounter-any True\ncounter-iter 1 4\ncounter-next 1\ncounter-next 2\ncounter-next 3\ncounter-all True\nhot True False\nlarge False True\ntruth-stop-escaped\nnext-error-escaped\n",
    );

    for (source, kind) in [
        ("any()", "TypeError"),
        ("all()", "TypeError"),
        ("any([],1)", "TypeError"),
        ("all(iterable=[])", "TypeError"),
        ("any(1)", "TypeError"),
        (
            "class Bad:\n    def __iter__(self): return 1\nany(Bad())",
            "TypeError",
        ),
        (
            "class Bad:\n    def __iter__(self): return self\n    def __next__(self): raise ValueError('next')\nall(Bad())",
            "ValueError",
        ),
        (
            "class Bad:\n    def __bool__(self): return 1\nany([Bad()])",
            "TypeError",
        ),
        (
            "class Bad:\n    def __len__(self): return -1\nall([Bad()])",
            "ValueError",
        ),
        (
            "class Bad:\n    def __len__(self): raise LookupError('length')\nany([Bad()])",
            "LookupError",
        ),
    ] {
        assert_eq!(error(source).kind, kind, "{source}");
    }
}

#[test]
fn min_and_max_stream_iterables_keys_and_comparisons() {
    let source = r#"print(min([3,1,2]),max([3,1,2]),min(3,1,2),max(3,1,2))
print(min([],default=9),max((),default=None))
print(min(['aaa','b','cc'],key=len),max(['aaa','b','cc'],key=len))
def values():
    print('generator-start')
    yield 4
    print('generator-middle')
    yield 1
    yield 3
print('generator',min(values()),max(values()))
class Counter:
    def __init__(self): self.value=0
    def __iter__(self):
        print('counter-iter')
        return self
    def __next__(self):
        scratch=0.0
        for i in range(20): scratch+=0.5
        if self.value>=4: raise StopIteration
        value=3-self.value
        self.value+=1
        print('counter-next',value)
        return value
print('counter-min',min(Counter()))
class Truth:
    def __init__(self,value): self.value=value
    def __bool__(self):
        print('truth',self.value)
        return self.value
class KeyValue:
    def __init__(self,value): self.value=value
    def __lt__(self,other):
        scratch=0.0
        for i in range(20): scratch+=0.5
        print('lt',self.value,other.value)
        return Truth(self.value<other.value)
    def __gt__(self,other):
        scratch=0.0
        for i in range(20): scratch+=0.5
        print('gt',self.value,other.value)
        return Truth(self.value>other.value)
class Item:
    def __init__(self,name,value): self.name=name; self.value=value
def keyed(item):
    scratch=0.0
    for i in range(20): scratch+=0.5
    print('key',item.name)
    return KeyValue(item.value)
items=[Item('first',2),Item('second',1),Item('tie',1),Item('last',3)]
print('keyed-min',min(items,key=keyed).name)
print('keyed-max',max(items,key=keyed).name)
class DefaultKey:
    def __call__(self,value):
        print('default-key-called')
        return value
print('default',min([],default='empty',key=DefaultKey()))
def hot(items,use_max):
    total=0
    for i in range(30): total+=i
    return max(items) if use_max else min(items)
print('hot',hot([4,2,3],False),hot([4,2,3],True))
print('large-key',min([[i] for i in range(5000)],key=len))
class StopKey:
    def __call__(self,value): raise StopIteration('key-stop')
try:
    min([1],key=StopKey())
except StopIteration:
    print('key-stop-escaped')
class StopCompare:
    def __lt__(self,other): raise StopIteration('compare-stop')
try:
    min([StopCompare(),StopCompare()])
except StopIteration:
    print('compare-stop-escaped')
class BadNext:
    def __iter__(self): return self
    def __next__(self): raise LookupError('next-error')
try:
    max(BadNext())
except LookupError:
    print('next-error-escaped')"#;
    assert_output_under_stress_gc_and_jit(
        source,
        b"1 3 1 3\n9 None\nb aaa\ngenerator-start\ngenerator-middle\ngenerator-start\ngenerator-middle\ngenerator 1 4\ncounter-iter\ncounter-next 3\ncounter-next 2\ncounter-next 1\ncounter-next 0\ncounter-min 0\nkey first\nkey second\nlt 1 2\ntruth True\nkey tie\nlt 1 1\ntruth False\nkey last\nlt 3 1\ntruth False\nkeyed-min second\nkey first\nkey second\ngt 1 2\ntruth False\nkey tie\ngt 1 2\ntruth False\nkey last\ngt 3 2\ntruth True\nkeyed-max last\ndefault empty\nhot 2 4\nlarge-key [0]\nkey-stop-escaped\ncompare-stop-escaped\nnext-error-escaped\n",
    );

    for (source, kind) in [
        ("min()", "TypeError"),
        ("max()", "TypeError"),
        ("min([])", "ValueError"),
        ("max(())", "ValueError"),
        ("min(1)", "TypeError"),
        ("max([1],unknown=2)", "TypeError"),
        ("min(1,2,default=0)", "TypeError"),
        ("max([1],key=2)", "TypeError"),
        ("min([1,'x'])", "TypeError"),
        (
            "class Bad:\n    def __iter__(self): return 1\nmin(Bad())",
            "TypeError",
        ),
    ] {
        assert_eq!(error(source).kind, kind, "{source}");
    }
}

#[test]
fn callable_sentinel_iterators_stream_through_all_protocol_layers() {
    let source = r#"class Counter:
    def __init__(self,start,stop):
        self.value=start
        self.stop=stop
    def __call__(self):
        scratch=0.0
        for i in range(20): scratch+=0.5
        value=self.value
        self.value+=1
        return value
stream=iter(Counter(0,4),4)
print(type(stream).__name__,iter(stream) is stream)
print(next(stream),next(stream),list(stream),next(stream,'done'))
print(tuple(iter(Counter(0,3),3)))
print(sum(iter(Counter(1,5),5)))
print(any(iter(Counter(0,3),2)),all(iter(Counter(1,4),4)))
print(min(iter(Counter(2,6),6)),max(iter(Counter(2,6),6)))
for_total=0
for value in iter(Counter(0,4),4): for_total+=value
print('for',for_total)
class Verdict:
    def __init__(self,value): self.value=value
    def __bool__(self):
        scratch=0.0
        for i in range(20): scratch+=0.5
        print('truth',self.value)
        return self.value
class Item:
    def __init__(self,value): self.value=value
    def __eq__(self,other):
        scratch=0.0
        for i in range(20): scratch+=0.5
        print('eq',self.value,other.value)
        return Verdict(self.value==other.value)
class ItemSource:
    def __init__(self): self.value=0
    def __call__(self):
        self.value+=1
        return Item(self.value)
objects=iter(ItemSource(),Item(3))
print(next(objects).value,next(objects).value,next(objects,'done'))
def make_counter(stop):
    value=0
    def pull():
        nonlocal value
        result=value
        value+=1
        return result
    return pull
print('large',sum(iter(make_counter(5000),5000)))
class Stops:
    def __init__(self): self.value=0
    def __call__(self):
        if self.value==2: raise StopIteration('callable-stop')
        value=self.value
        self.value+=1
        return value
print('callable-stop',list(iter(Stops(),99)))
direct_stop=iter(Stops(),99)
print('direct-stop',next(direct_stop),next(direct_stop))
try:
    next(direct_stop)
except StopIteration as error:
    print('normalized-stop',error.args)
print('still-stopped',next(direct_stop,'done'))
class EqualityStopsOnce:
    def __init__(self): self.first=True
    def __eq__(self,other):
        if self.first:
            self.first=False
            raise StopIteration('equality-stop')
        return False
eq_stop=iter(Counter(0,99),EqualityStopsOnce())
print('equality-stop',next(eq_stop,'default'),next(eq_stop))
class Fails:
    def __call__(self): raise LookupError('callable-error')
try:
    next(iter(Fails(),0))
except LookupError:
    print('callable-error')
class BadItem:
    def __eq__(self,other): raise ValueError('equality-error')
class BadSource:
    def __call__(self): return BadItem()
try:
    next(iter(BadSource(),0))
except ValueError:
    print('equality-error')"#;
    assert_output_under_stress_gc_and_jit(
        source,
        b"callable_iterator True\n0 1 [2, 3] done\n(0, 1, 2)\n10\nTrue True\n2 5\nfor 6\neq 3 1\ntruth False\neq 3 2\ntruth False\neq 3 3\ntruth True\n1 2 done\nlarge 12497500\ncallable-stop [0, 1]\ndirect-stop 0 1\nnormalized-stop ()\nstill-stopped done\nequality-stop default 1\ncallable-error\nequality-error\n",
    );

    for source in [
        "iter()",
        "iter(1,2)",
        "iter(lambda: 1,2,3)",
        "iter(callable=lambda: 1,sentinel=2)",
        "iter(lambda: 1)",
    ] {
        assert_eq!(error(source).kind, "TypeError", "{source}");
    }
}

#[test]
fn sequence_getitem_fallback_streams_through_all_protocol_layers() {
    let source = r#"class Sequence:
    def __init__(self,start,stop):
        self.start=start
        self.stop=stop
    def __getitem__(self,index):
        scratch=0.0
        for i in range(20): scratch+=0.5
        if index>=self.stop: raise IndexError('finished')
        return self.start+index
stream=iter(Sequence(10,4))
print(type(stream).__name__,iter(stream) is stream)
print(next(stream),next(stream),list(stream),next(stream,'done'))
print(tuple(Sequence(0,3)),sum(Sequence(1,4)))
print(any(Sequence(0,3)),all(Sequence(1,3)))
print(min(Sequence(2,4)),max(Sequence(2,4)))
total=0
for value in Sequence(0,4): total+=value
print('for',total,2 in Sequence(0,4))
class Pairs:
    def __getitem__(self,index):
        return [('a',1),('b',2)][index]
print(dict(Pairs()))
def collect(*values): print('star',values)
collect(*Sequence(0,3))
a,b,c=Sequence(0,3)
print('unpack',a,b,c)
class Static:
    __getitem__=staticmethod(lambda index:[7,8][index])
class ByClass:
    @classmethod
    def __getitem__(cls,index): return [cls.__name__,index][index]
print(list(Static()),list(ByClass()))
class Meta(type):
    def __getitem__(cls,index): return [20,21][index]
class Managed(metaclass=Meta): pass
print('meta',list(Managed))
class Dynamic:
    def __getitem__(self,index): return [10,11][index]
dynamic=iter(Dynamic())
print('dynamic',next(dynamic))
Dynamic.__getitem__=lambda self,index:[20,21][index]
print('dynamic',next(dynamic),next(dynamic,'done'))
class Retry:
    def __init__(self): self.failed=False
    def __getitem__(self,index):
        if not self.failed:
            self.failed=True
            raise ValueError('retry')
        return [30,31][index]
retry=iter(Retry())
try:
    next(retry)
except ValueError:
    print('retry-error')
print('retry',next(retry),next(retry),next(retry,'done'))
class Stops:
    def __getitem__(self,index):
        if index==2: raise StopIteration('source-stop')
        return index
stops=iter(Stops())
print('stop-values',next(stops),next(stops))
try:
    next(stops)
except StopIteration as error:
    print('normalized-stop',error.args)
print('still-stopped',next(stops,'done'))
print('large',sum(Sequence(0,5000)))"#;
    assert_output_under_stress_gc_and_jit(
        source,
        b"iterator True\n10 11 [12, 13] done\n(0, 1, 2) 10\nTrue True\n2 5\nfor 6 True\n{'a': 1, 'b': 2}\nstar (0, 1, 2)\nunpack 0 1 2\n[7, 8] ['ByClass', 1]\nmeta [20, 21]\ndynamic 10\ndynamic 21 done\nretry-error\nretry 30 31 done\nstop-values 0 1\nnormalized-stop ()\nstill-stopped done\nlarge 12497500\n",
    );

    for (source, kind) in [
        (
            "class Blocked:\n    __iter__=None\n    def __getitem__(self,index): return index\niter(Blocked())",
            "TypeError",
        ),
        (
            "class Bad:\n    __getitem__=None\nnext(iter(Bad()))",
            "TypeError",
        ),
        (
            "class Fails:\n    def __getitem__(self,index): raise LookupError('item')\nnext(iter(Fails()))",
            "LookupError",
        ),
    ] {
        assert_eq!(error(source).kind, kind, "{source}");
    }
}

#[test]
fn list_and_tuple_constructors_consume_custom_iterators() {
    let source = "class Counter:\n    def __init__(self,n):\n        self.i=0\n        self.n=n\n    def __iter__(self):\n        return self\n    def stop(self):\n        raise StopIteration\n    def __next__(self):\n        if self.i>=self.n:\n            self.stop()\n        value=str(self.i)\n        self.i+=1\n        return value\nclass Fresh:\n    def __iter__(self):\n        return Counter(2)\nprint(list(Counter(4)))\nprint(tuple(Counter(3)))\nprint(list(Fresh()))\na,b=Fresh()\nprint(a,b)\nfor count in [1,3]:\n    try:\n        a,b=Counter(count)\n    except ValueError as error:\n        print(str(error))\ndef collect(*values,marker):\n    print(values,marker)\ncollect(*Counter(3),marker='single')\ncollect(*Counter(1),*Counter(2),marker='multiple')\nclass Failing:\n    def __iter__(self):\n        return self\n    def __next__(self):\n        raise ValueError('iteration failed')\nfor operation in [0,1]:\n    try:\n        if operation==0:\n            list(Failing())\n        else:\n            collect(*Failing(),marker='failure')\n    except ValueError as error:\n        print(type(error).__name__)";
    let program = compile(source, "custom-constructor-iteration").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.gc_interval = Some(1);
    let mut output = Vec::new();
    vm.run(&program, &mut output).unwrap();
    assert_eq!(
        String::from_utf8(output).unwrap(),
        "['0', '1', '2', '3']\n('0', '1', '2')\n['0', '1']\n0 1\nnot enough values to unpack (expected 2, got 1)\ntoo many values to unpack (expected 2)\n('0', '1', '2') single\n('0', '0', '1') multiple\nValueError\nValueError\n"
    );

    for source in [
        "class Bad:\n    def __iter__(self):\n        return 1\nlist(Bad())",
        "class Bad:\n    def __iter__(self):\n        return self\ntuple(Bad())",
    ] {
        assert_eq!(error(source).kind, "TypeError");
    }
}
#[test]
fn dict_constructor_consumes_custom_pair_iterables() {
    let source = r#"class Pair:
    def __init__(self,key,value,count):
        self.key=key
        self.value=value
        self.count=count
        self.i=0
    def __iter__(self):
        print('pair-iter',self.key)
        return self
    def __next__(self):
        if self.i>=self.count:
            raise StopIteration
        if self.i==0:
            item=self.key
        else:
            item=self.value
        self.i+=1
        print('pair-next',item)
        return item
class Outer:
    def __init__(self):
        self.i=0
    def __iter__(self):
        print('outer-iter')
        return self
    def __next__(self):
        if self.i>=2:
            raise StopIteration
        print('outer-next',self.i)
        if self.i==0:
            pair=Pair('a',1,2)
        else:
            pair=Pair('b',2,2)
        self.i+=1
        return pair
print(dict(Outer(),a=9,c=3))
print(dict([Pair('x',7,2)]))
class PairFactory:
    def __iter__(self):
        print('factory-iter')
        return Pair('z',8,2)
print(dict([PairFactory()]))
class TupleOuter:
    def __init__(self):
        self.done=False
    def __iter__(self):
        return self
    def __next__(self):
        if self.done:
            raise StopIteration
        self.done=True
        return ('d',4)
print(dict(TupleOuter()))
try:
    dict([Pair('short',0,1)])
except ValueError as error:
    print(str(error))
try:
    dict([('ok',1),Pair('short2',0,1)])
except ValueError as error:
    print(str(error))
class FailingPair:
    def __iter__(self):
        return self
    def __next__(self):
        raise ValueError('pair failed')
try:
    dict([FailingPair()])
except ValueError as error:
    print(str(error))
class StopAtIter:
    def __iter__(self):
        raise StopIteration
try:
    dict(StopAtIter())
except StopIteration:
    print('iter stop propagated')"#;
    let program = compile(source, "custom-dict-iteration").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.gc_interval = Some(1);
    let mut output = Vec::new();
    vm.run(&program, &mut output).unwrap();
    assert_eq!(
        String::from_utf8(output).unwrap(),
        "outer-iter\nouter-next 0\npair-iter a\npair-iter a\npair-next a\npair-next 1\nouter-next 1\npair-iter b\npair-iter b\npair-next b\npair-next 2\n{'a': 9, 'b': 2, 'c': 3}\npair-iter x\npair-iter x\npair-next x\npair-next 7\n{'x': 7}\nfactory-iter\npair-iter z\npair-next z\npair-next 8\n{'z': 8}\n{'d': 4}\npair-iter short\npair-iter short\npair-next short\ndictionary update sequence element #0 has length 1; 2 is required\npair-iter short2\npair-iter short2\npair-next short2\ndictionary update sequence element #1 has length 1; 2 is required\npair failed\niter stop propagated\n"
    );

    assert_eq!(
        error("class Bad:\n    def __iter__(self):\n        return 1\ndict(Bad())").kind,
        "TypeError"
    );
}
#[test]
fn native_module() {
    assert_eq!(
        output(include_str!("../../../examples/fastmath.tonic")),
        "42\n"
    );
}
#[test]
fn integers_promote_without_overflow() {
    assert_eq!(
        output("x = 1152921504606846975\nprint(x+1, x*x, -x*3)\n"),
        "1152921504606846976 1329227995784915870597964051066650625 -3458764513820540925\n"
    );
}
#[test]
fn power_bitwise_shift_and_invert_cover_bigints_errors_and_jit_fallback() {
    assert_eq!(
        output(
            "print(2**10,2**-2,5|2,5^3,5&3,1<<65,-8>>2,~5)\nprint(True&True,type(True&True).__name__,True|2,True<<2)\na=3;a**=4\nb=5;b|=2\nc=5;c^=3\nd=5;d&=3\ne=1;e<<=6\nf=-8;f>>=2\nprint(a,b,c,d,e,f)\nprint(3**100)"
        ),
        "1024 0.25 7 6 1 36893488147419103232 -2 -6\nTrue bool 3 4\n81 7 6 1 64 -2\n515377520732011331036461129765621272702107522001\n"
    );
    for (source, kind) in [
        ("1 << -1", "ValueError"),
        ("1.0 | 2", "TypeError"),
        ("0 ** -1", "ZeroDivisionError"),
        ("1 << 100000000", "MemoryError"),
        ("2 ** 100000000", "MemoryError"),
    ] {
        assert_eq!(error(source).kind, kind, "{source}");
    }
    assert_eq!(
        output("print(1 ** 100000000000000000000, 0 ** 100000000000000000000, (-1) ** 100000000000000000001, 0 << 100000000000000000000)"),
        "1 0 -1 0\n"
    );

    let program = compile(
        "def transform(x):\n    return ((x**3)|2)^1\nfor i in range(20):\n    transform(i)\nprint(transform(5))",
        "power-bitwise-jit-fallback",
    )
    .unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.jit_threshold = 1;
    vm.jit_min_instructions = 0;
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"126\n");
    assert!(vm.stats.jit_compile_attempts >= 1);
    assert_eq!(vm.stats.jit_compiled, 0);
}
#[test]
fn calls_recursion_and_scopes() {
    assert_eq!(output("def f(n):\n    if n <= 1:\n        return 1\n    return n*f(n-1)\nprint(f(20))\nx=1\ndef g():\n    return x\nx=8\nprint(g())\n"),"2432902008176640000\n8\n");
}
#[test]
fn local_binding_is_whole_function() {
    assert_eq!(
        error("x=3\ndef f():\n    print(x)\n    x=4\nf()\n").kind,
        "UnboundLocalError"
    );
    assert_eq!(
        error("def f():\n    if False:\n        x=1\n    return x\nf()\n").kind,
        "UnboundLocalError"
    );
}
#[test]
fn builtin_rebinding_and_function_aliases() {
    assert_eq!(
        output("def f(x):\n    return x+10\np = print\nprint = f\np(print(2))\n"),
        "12\n"
    );
}

#[test]
fn function_annotations_evaluate_and_survive_jit_and_stress_gc() {
    let source = "def f(x: int, *args: str, y: float = 1, **kwargs: dict) -> str:\n    return str(x)\ndef bare():\n    pass\nclass Holder:\n    def method(self, value: int) -> str:\n        return str(value)\ndef make():\n    class Marker:\n        pass\n    def tagged(value: Marker) -> Marker:\n        return value\n    return tagged\ntagged=make()\nbare.__annotations__['late']=int\nprint(f.__annotations__)\nprint(bare.__annotations__)\nprint(Holder().method.__annotations__)\nprint(tagged.__annotations__['value'].__name__,tagged.__annotations__['return'].__name__)\nprint(f(7))";
    let program = compile(source, "function-annotations").unwrap();
    for mode in [ExecutionMode::Interpreter, ExecutionMode::Jit] {
        let mut vm = Vm::new().unwrap();
        vm.execution_mode = mode;
        vm.gc_interval = Some(1);
        let mut out = Vec::new();
        vm.run(&program, &mut out).unwrap();
        assert_eq!(
            out,
            b"{'x': <class 'int'>, 'args': <class 'str'>, 'y': <class 'float'>, 'kwargs': <class 'dict'>, 'return': <class 'str'>}\n{'late': <class 'int'>}\n{'value': <class 'int'>, 'return': <class 'str'>}\nMarker Marker\n7\n"
        );
    }
}

#[test]
fn variable_annotations_obey_scope_order_and_survive_jit_and_stress_gc() {
    let source = "def annotation():\n    print('annotation')\n    return int\nvalue: annotation() = 1\nmissing: str\nif False:\n    dead: float\ndef owner():\n    print('owner')\n    return {}\ndef key():\n    print('key')\n    return 0\nowner()[key()]: print('ignored')\nbox={}\nbox['item']: print('ignored') = 3\ndef build():\n    class Marker:\n        pass\n    class Holder:\n        item: Marker\n        absent: str\n    return Holder\nHolder=build()\ndef local_ok():\n    hidden: missing_name\n    return 7\ndef local_missing():\n    hidden: int\n    return hidden\nprint(value,__annotations__)\nprint(box)\nprint(Holder.__annotations__['item'].__name__,Holder.__annotations__['absent'].__name__)\nprint(local_ok())\ntry:\n    local_missing()\nexcept UnboundLocalError as error:\n    print(type(error).__name__)";
    let program = compile(source, "variable-annotations").unwrap();
    for mode in [ExecutionMode::Interpreter, ExecutionMode::Jit] {
        let mut vm = Vm::new().unwrap();
        vm.execution_mode = mode;
        vm.gc_interval = Some(1);
        let mut out = Vec::new();
        vm.run(&program, &mut out).unwrap();
        assert_eq!(
            out,
            b"annotation\nowner\nkey\n1 {'value': <class 'int'>, 'missing': <class 'str'>}\n{'item': 3}\nMarker str\n7\nUnboundLocalError\n"
        );
    }
}

#[test]
fn type_parameters_aliases_and_lexical_cells_survive_jit_and_stress_gc() {
    let source = "def identity[T: int](value: T) -> T:\n    return value\ndef reveal[T]():\n    def nested():\n        return T\n    return nested\nclass Box[T]:\n    seen=T\n    item: T\n    def reveal(self):\n        return T\nclass Child[T](Box[T]):\n    pass\ntype Plain = int\ntype Pair[T] = (T,T)\nprint(identity.__type_params__,identity.__annotations__,identity(7))\nprint(identity.__type_params__[0].__name__,identity.__type_params__[0].__bound__)\nprint(reveal()())\nprint(Box.__type_params__,Box.seen,Box.__annotations__,Box().reveal())\nprint(Child.__bases__)\nprint(Plain,Plain.__name__,Plain.__type_params__,Plain.__value__)\nprint(Pair,Pair.__name__,Pair.__type_params__,Pair.__value__)\nprint(list[int],tuple[int,str],dict[str,int],list[int]([1,2]))\nprint(Pair[int],Pair[int].__origin__,Pair[int].__args__,Pair[int].__value__)\ndef shadow[T](T):\n    return T\nprint(shadow(9),shadow.__type_params__)\ndef missing[T]():\n    print(T)\n    T=1\ntry:\n    missing()\nexcept UnboundLocalError as error:\n    print(type(error).__name__)";
    let program = compile(source, "type-parameters").unwrap();
    for mode in [ExecutionMode::Interpreter, ExecutionMode::Jit] {
        let mut vm = Vm::new().unwrap();
        vm.execution_mode = mode;
        vm.gc_interval = Some(1);
        let mut out = Vec::new();
        vm.run(&program, &mut out).unwrap();
        assert_eq!(
            out,
            b"(T,) {'value': T, 'return': T} 7\nT <class 'int'>\nT\n(T,) T {'item': T} T\n(<class 'Box'>,)\nPlain Plain () <class 'int'>\nPair Pair (T,) (T, T)\nlist[int] tuple[int, str] dict[str, int] [1, 2]\nPair[int] Pair (<class 'int'>,) (T, T)\n9 (T,)\nUnboundLocalError\n"
        );
    }
}

#[test]
fn type_parameter_defaults_survive_jit_and_stress_gc() {
    let source = "def generic[T = int, U = list[T], *Ts = *tuple[str, bool], **P = [float, dict]]():\n    pass\ndef bare[T, U = int]():\n    pass\nclass Box[T, U = int]:\n    pass\nclass Spread[T, *Ts = *tuple[int, str]]:\n    pass\ntype Alias[T, U = int] = tuple[T, U]\nprint(generic.__type_params__)\nprint(generic.__type_params__[0].__default__,generic.__type_params__[1].__default__)\nprint(generic.__type_params__[2].__default__,generic.__type_params__[3].__default__)\nprint(generic.__type_params__[2].__default__.__origin__,generic.__type_params__[2].__default__.__args__,generic.__type_params__[2].__default__.__unpacked__)\nprint(bare.__type_params__[0].__default__,bare.__type_params__[0].__default__ is bare.__type_params__[0].__default__)\nprint(Box[str].__args__)\nprint(Spread[bool].__args__)\nprint(Alias[str].__args__)";
    let program = compile(source, "type-parameter-defaults").unwrap();
    for mode in [ExecutionMode::Interpreter, ExecutionMode::Jit] {
        let mut vm = Vm::new().unwrap();
        vm.execution_mode = mode;
        vm.gc_interval = Some(1);
        let mut out = Vec::new();
        vm.run(&program, &mut out).unwrap();
        assert_eq!(
            out,
            b"(T, U, Ts, P)\n<class 'int'> list[T]\n*tuple[str, bool] [<class 'float'>, <class 'dict'>]\n<class 'tuple'> (<class 'str'>, <class 'bool'>) True\ntyping.NoDefault True\n(<class 'str'>, <class 'int'>)\n(<class 'bool'>, <class 'int'>, <class 'str'>)\n(<class 'str'>,)\n"
        );
    }
}
#[test]
fn short_circuit_and_chains() {
    assert_eq!(output("def tick(n):\n    print(n)\n    return n\nprint(3 < tick(2) < tick(1))\nprint(0 and missing, 5 or missing, not [])\nprint(tick(1) if True else missing)\n"),"2\nFalse\n0 5 True\n1\n1\n");
}
#[test]
fn loop_else_break_continue_nested() {
    assert_eq!(output("for i in range(3):\n    for j in range(2):\n        break\n    if i==1:\n        continue\n    print(i)\nelse:\n    print('done')\nwhile True:\n    break\nelse:\n    print('bad')\n"),"0\n2\ndone\n");
}
#[test]
fn loop_rebinding_does_not_change_iterator() {
    assert_eq!(
        output("xs=[1,2,3]\nfor x in xs:\n    xs=[9]\n    print(x)\n"),
        "1\n2\n3\n"
    );
}
#[test]
fn unpack_and_evaluation_order() {
    assert_eq!(output("a,b=1,2\na,b=b,a\nprint(a,b)\na,(b,c)=[1,[2,3]]\nprint(a,b,c)\na,b='é字'\nprint(a,b)\n"),"2 1\n1 2 3\né 字\n");
    assert_eq!(error("a,b=[1]\n").kind, "ValueError");
    assert_eq!(error("a,b=[1,2,3]\n").kind, "ValueError");
}
#[test]
fn indexing_and_iteration() {
    assert_eq!(output("print([10,20][-1], 'é字'[1], len('é字'))\nfor i in range(5,-1,-2):\n    print(i)\nprint(range(2,9,2)[-1])\n"),"20 字 2\n5\n3\n1\n8\n");
    assert_eq!(error("print([1][2])").kind, "IndexError");
    assert_eq!(error("range(0,1,0)").kind, "ValueError");
}
#[test]
fn list_tuple_and_unicode_slicing() {
    assert_eq!(
        output(
            "xs=[0,1,2,3,4]\nprint(xs[:],xs[1:4],xs[::-1],xs[4:0:-2])\nprint((0,1,2,3)[-3:-1])\nprint('aé字z'[::-2])\nprint(xs[-999999999999999999999999999999:],xs[:999999999999999999999999999999],xs[::-999999999999999999999999999999])"
        ),
        "[0, 1, 2, 3, 4] [1, 2, 3] [4, 3, 2, 1, 0] [4, 2]\n(1, 2)\nzé\n[0, 1, 2, 3, 4] [0, 1, 2, 3, 4] [4]\n"
    );
    assert_eq!(error("[1,2][::0]").kind, "ValueError");
    assert_eq!(error("[1,2][::1.0]").kind, "TypeError");
}
#[test]
fn negative_division() {
    assert_eq!(
        output("print(-7//3,-7%3,7//-3,7%-3,-7//-3,-7%-3)\nprint(True+True, False==0)\n"),
        "-3 2 -3 -2 2 -1\n2 True\n"
    );
}
#[test]
fn floats_and_exact_mixed_comparisons() {
    assert_eq!(output("print(1.5+2.5, -7.0//3.0, -7.0%3.0)\nprint(9007199254740993 == 9007199254740992.0, 9007199254740993 > 9007199254740992.0)\n"),"4.0 -3.0 2.0\nFalse True\n");
}

#[test]
fn identity_and_membership_cover_native_custom_and_suspending_paths() {
    let source = r#"shared=[1]
alias=shared
print(shared is alias,shared is not [1],None is None)
print(2 in [1,2,3],4 not in (1,2,3),'bc' in 'abcd','x' in {'x':1},2 in range(4))
class Truth:
    def __init__(self,value): self.value=value
    def __bool__(self):
        print('truth',self.value)
        return self.value
class Container:
    def __contains__(self,item):
        print('contains',item)
        return Truth(item==7)
print(7 in Container(),8 not in Container())
class Needle:
    def __eq__(self,item):
        print('equal',item)
        return item==2
def values():
    yield 1
    yield 2
    yield 3
print(Needle() in values())
class Iterator:
    def __init__(self): self.current=0
    def __iter__(self): return self
    def __next__(self):
        self.current+=1
        if self.current>2: raise StopIteration
        return self.current
print(3 not in Iterator())
class Meta(type):
    def __contains__(cls,item): return item==cls.answer
class TypeContainer(metaclass=Meta):
    answer=9
print(9 in TypeContainer,8 not in TypeContainer)"#;
    assert_output_under_stress_gc_and_jit(
        source,
        b"True True True\nTrue True True True True\ncontains 7\ntruth True\ncontains 8\ntruth False\nTrue True\nequal 1\nequal 2\nTrue\nTrue\nTrue True\n",
    );

    for invalid in ["1 in 2", "1 in '123'"] {
        let program = compile(invalid, "invalid-membership").unwrap();
        for mode in [ExecutionMode::Interpreter, ExecutionMode::Jit] {
            let mut vm = Vm::new().unwrap();
            vm.execution_mode = mode;
            vm.gc_interval = Some(1);
            assert_eq!(
                vm.run(&program, &mut Vec::new()).unwrap_err().kind,
                "TypeError"
            );
        }
    }
}

#[test]
fn comprehensions_isolate_scope_capture_cells_and_preserve_iteration_timing() {
    let source = r#"x=99
print([x*y for x in range(5) if x%2 for y in range(3) if y])
print(x)
print({x:x*x for x in range(5) if x%2})
print([[x*y for y in range(3)] for x in range(4)])
print([a+b for a,b in [(1,2),(3,4)]])
def capture(offset):
    values=[offset+x for x in range(3)]
    mapping={x:offset+x for x in range(3)}
    stream=(offset+x for x in range(3))
    return values,mapping,stream
values,mapping,stream=capture(10)
print(values,mapping,list(stream))
funcs=[lambda: x for x in range(3)]
print(funcs[0](),funcs[1](),funcs[2]())
class Counter:
    def __init__(self): self.value=0
    def __iter__(self): return self
    def __next__(self):
        self.value+=1
        if self.value>3: raise StopIteration
        return self.value
print([value for value in Counter()])
class Key:
    def __init__(self,value): self.value=value
    def __hash__(self): return self.value%2
    def __eq__(self,other): return self.value==other.value
mapping={Key(value):[value] for value in range(3)}
print(len(mapping),mapping[Key(1)])
class Source:
    def __iter__(self):
        print('source-iter')
        return iter([1,2,3])
def element(value):
    print('element',value)
    return value*10
stream=(element(value) for value in Source())
print('made',type(stream).__name__)
print(next(stream),list(stream))"#;
    assert_output_under_stress_gc_and_jit(
        source,
        b"[1, 2, 3, 6]\n99\n{1: 1, 3: 9}\n[[0, 0, 0], [0, 1, 2], [0, 2, 4], [0, 3, 6]]\n[3, 7]\n[10, 11, 12] {0: 10, 1: 11, 2: 12} [10, 11, 12]\n2 2 2\n[1, 2, 3]\n3 [1]\nsource-iter\nmade generator\nelement 1\nelement 2\nelement 3\n10 [20, 30]\n",
    );

    assert_eq!(
        error("[hidden for hidden in range(2)]\nprint(hidden)").kind,
        "NameError"
    );
    assert_eq!(
        error("class C:\n    values=[1]\n    result=[values for item in range(1)]").kind,
        "NameError"
    );
}

#[test]
fn async_comprehensions_suspend_scope_and_preserve_outer_iteration_timing() {
    let source = r#"import asyncio
class Source:
    def __init__(self,limit):
        self.value=0
        self.limit=limit
    def __aiter__(self):
        print('aiter',self.limit)
        return self
    async def __anext__(self):
        if self.value>=self.limit:
            raise StopAsyncIteration
        value=self.value
        self.value+=1
        return value
async def transform(value):
    return value*10
async def collect():
    values=[await transform(x) async for x in Source(4) if x%2]
    awaited=[await transform(x) for x in [2,3]]
    lambdas=[lambda value=await transform(x):value for x in [4,5]]
    mapping={x:await transform(x) async for x in Source(3)}
    nested=[x+y async for x in Source(2) for y in [10,20]]
    return values,awaited,lambdas[0](),lambdas[1](),mapping,nested
print(asyncio.run(collect()))
stream=(await transform(x) async for x in Source(3))
print(type(stream).__name__)
async def consume(stream):
    return [value async for value in stream]
print(asyncio.run(consume(stream)))"#;
    assert_output_under_stress_gc_and_jit(
        source,
        b"aiter 4\naiter 3\naiter 2\n([10, 30], [20, 30], 40, 50, {0: 0, 1: 10, 2: 20}, [10, 20, 11, 21])\naiter 3\nasync_generator\n[0, 10, 20]\n",
    );
}

#[test]
fn set_literals_comprehensions_and_hash_collisions_survive_jit_and_stress_gc() {
    let source = r#"import asyncio
class Key:
    def __init__(self,value):
        self.value=value
    def __hash__(self):
        return self.value%2
    def __eq__(self,other):
        return self.value==other.value
values={Key(1),Key(3),Key(1)}
print(len(values),Key(3) in values,Key(2) not in values)
print(values=={Key(3),Key(1)})
unique={value%3 for value in range(8)}
print(len(unique),unique=={0,1,2})
print({value for value in []})
print(type(values).__name__,len(list(values)))
class Source:
    def __init__(self):
        self.value=0
    def __aiter__(self):
        return self
    async def __anext__(self):
        if self.value>=4:
            raise StopAsyncIteration
        value=self.value
        self.value+=1
        return value
async def collect():
    return {value%2 async for value in Source()}
print(asyncio.run(collect())=={0,1})"#;
    assert_output_under_stress_gc_and_jit(
        source,
        b"2 True True\nTrue\n3 True\nset()\nset 2\nTrue\n",
    );
}

#[test]
fn basic_match_patterns_guards_and_captures_survive_jit_and_stress_gc() {
    let source = r#"def subject():
    print('subject')
    return 2
def reject(value):
    print('guard',value)
    return False
match subject():
    case 1:
        print('one')
    case 2 as guarded if reject(guarded):
        print('guarded')
    case 2 | 3 as selected:
        print('selected',selected,guarded)
match 1:
    case True:
        print('bool')
    case 1:
        print('int')
class Codes:
    hit=7
match 7:
    case Codes.hit:
        print('qualified')
match [1,2,3,4]:
    case [first,*middle,last]:
        print(first,middle,last)
match (1,[2,3]):
    case [one,[two,three]]:
        print('nested',one,two,three)
match range(3):
    case [zero,*rest]:
        print('range',zero,rest)
match 'ab':
    case [left,right]:
        print('string-sequence')
    case other:
        print('string',other)
class Numbers(list):
    pass
match Numbers([5,6]):
    case [five,six]:
        print('subclass',five,six)
match {'a':[1,2,3],'b':4}:
    case {'a':[head,*tail],**remaining}:
        print('mapping',head,tail,remaining)
match {'other':1}:
    case {'missing':value}:
        print('unexpected')
    case fallback:
        print('missing',fallback)
class Mapping(dict):
    pass
match Mapping({'x':5}):
    case {'x':mapped}:
        print('dict-subclass',mapped)
class Key:
    def __init__(self,value):
        self.value=value
    def __hash__(self):
        return 7
    def __eq__(self,other):
        return isinstance(other,Key) and self.value==other.value
class Keys:
    target=Key('target')
match {Key('target'):9,'kept':10}:
    case {Keys.target:found,**rest}:
        print('custom-key',found,rest)
class Point:
    __match_args__=('x','y')
    def __init__(self,x,y):
        self.x=x
        self.y=y
class Colored(Point):
    pass
match Colored(3,4):
    case Point(x,4):
        print('class',x)
match 7:
    case int(value):
        print('builtin-class',value)
match Point(1,2):
    case Point(missing=value):
        print('unexpected-attribute')
    case other:
        print('missing-attribute',type(other).__name__)
class Probe:
    @property
    def value(self):
        print('get-value')
        return 8
match Probe():
    case Probe(value=8):
        print('descriptor')
def choose(value):
    match value:
        case None:
            return lambda:'none'
        case 4 as kept:
            def read():
                return kept
            return read
        case other:
            return lambda:other
print(choose(None)(),choose(4)(),choose(9)())"#;
    assert_output_under_stress_gc_and_jit(
        source,
        b"subject\nguard 2\nselected 2 2\nint\nqualified\n1 [2, 3] 4\nnested 1 2 3\nrange 0 [1, 2]\nstring ab\nsubclass 5 6\nmapping 1 [2, 3] {'b': 4}\nmissing {'other': 1}\ndict-subclass 5\ncustom-key 9 {'kept': 10}\nclass 3\nbuiltin-class 7\nmissing-attribute Point\nget-value\ndescriptor\nnone 4 9\n",
    );
}

#[test]
fn class_pattern_match_args_validation_is_strict() {
    for source in [
        "class C:\n    __match_args__=['x']\nmatch C():\n    case C(value):\n        pass",
        "class C:\n    __match_args__=(1,)\nmatch C():\n    case C(value):\n        pass",
        "class C:\n    __match_args__=('x',)\nmatch C():\n    case C(first,second):\n        pass",
        "class C:\n    __match_args__=('x',)\n    x=1\nmatch C():\n    case C(first,x=second):\n        pass",
    ] {
        assert_eq!(error(source).kind, "TypeError", "{source}");
    }
    assert_eq!(
        error("class Keys:\n    first=1\n    second=True\nmatch {1:'x',2:'y'}:\n    case {Keys.first:left,Keys.second:right}:\n        pass").kind,
        "ValueError"
    );
}
#[test]
fn runtime_errors_have_spans() {
    let e = error("def f():\n    return 1//0\nf()\n");
    assert_eq!(e.kind, "ZeroDivisionError");
    assert_eq!(e.trace.len(), 2);
    assert!(e
        .render("test.tonic", "def f():\n    return 1//0\nf()\n")
        .contains("test.tonic:2:"));
}
#[test]
fn arity_name_type_import_errors() {
    for (src, kind) in [
        ("def f(x):\n    return x\nf()", "TypeError"),
        ("missing", "NameError"),
        ("1+'a'", "TypeError"),
        ("import missing", "ModuleNotFoundError"),
        ("import fastmath\nfastmath.missing", "AttributeError"),
        ("import fastmath\nfastmath.add(1)", "TypeError"),
    ] {
        assert_eq!(error(src).kind, kind);
    }
}
#[test]
fn fuel_and_recursion_limits() {
    let mut vm = Vm::new().unwrap();
    vm.limits.instructions = Some(100);
    assert_eq!(
        vm.run(
            &compile("while True:\n    pass", "x").unwrap(),
            &mut Vec::new()
        )
        .unwrap_err()
        .kind,
        "ResourceError"
    );
    vm.limits.instructions = None;
    vm.limits.frames = 16;
    assert_eq!(
        vm.run(
            &compile("def f():\n    return f()\nf()", "x").unwrap(),
            &mut Vec::new()
        )
        .unwrap_err()
        .kind,
        "RecursionError"
    );
    let mut out = Vec::new();
    vm.run(&compile("print(42)", "x").unwrap(), &mut out)
        .unwrap();
    assert_eq!(out, b"42\n");
}
#[test]
fn native_exception_cleans_local_handles() {
    let mut vm = Vm::new().unwrap();
    let p = compile("import fastmath\nfastmath.add('bad',2)", "x").unwrap();
    assert_eq!(vm.run(&p, &mut Vec::new()).unwrap_err().kind, "TypeError");
    assert_eq!(vm.active_handles(), 0);
}
#[test]
fn native_registration() {
    fn twice(ctx: &mut Context<'_>, args: &[Handle]) -> Result<Handle> {
        let n = ctx.to_i64(args[0])?;
        ctx.from_i64(n * 2)
    }
    let mut vm = Vm::new().unwrap();
    vm.register_native("demo", "twice", 1, twice).unwrap();
    let p = compile("import demo as d\nprint(d.twice(21))", "x").unwrap();
    let mut out = Vec::new();
    vm.run(&p, &mut out).unwrap();
    assert_eq!(out, b"42\n");
    assert_eq!(vm.stats.native_calls, 1);
    assert_eq!(vm.active_handles(), 0);
}

#[test]
fn stateful_native_registration_keeps_vm_owned_extension_state() {
    use std::sync::{
        atomic::{AtomicI64, Ordering},
        Arc,
    };

    let calls = Arc::new(AtomicI64::new(0));
    let state = Arc::clone(&calls);
    let mut vm = Vm::new().unwrap();
    vm.register_stateful_native(
        "stateful",
        "next",
        0,
        Arc::new(move |context, arguments| {
            assert!(arguments.is_empty());
            let _module = context.native_module("stateful")?;
            context.from_i64(state.fetch_add(1, Ordering::Relaxed) + 1)
        }),
    )
    .unwrap();
    let program = compile(
        "import stateful\nprint(stateful.next(), stateful.next())",
        "stateful-native",
    )
    .unwrap();
    let mut output = Vec::new();
    vm.run(&program, &mut output).unwrap();

    assert_eq!(String::from_utf8(output).unwrap(), "1 2\n");
    assert_eq!(calls.load(Ordering::Relaxed), 2);
}
#[test]
fn small_integer_loop_has_no_per_iteration_heap_allocations() {
    let p = compile("i=0\ntotal=0\nwhile i<10000:\n    total+=i\n    i+=1", "x").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.run(&p, &mut Vec::new()).unwrap();
    assert_eq!(vm.stats.heap_allocations, 0);
    assert_eq!(vm.stats.backedges, 10000);
}

#[test]
fn cranelift_leaf_loop_returns_and_guard_deopts_through_normal_frames() {
    let source = "def sum_to(n):\n    total=0\n    i=0\n    while i<n:\n        total+=i\n        i+=1\n    return total\ndef add(a,b):\n    return a+b\ni=0\nwhile i<7:\n    add(i,2)\n    i+=1\nprint(sum_to(100),add(1.5,2),add(1152921504606846975,1))";
    let program = compile(source, "jit-integration").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.jit_min_instructions = 0;
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"4950 3.5 1152921504606846976\n");
    assert_eq!(vm.stats.jit_compiled, 2);
    assert_eq!(vm.stats.jit_calls, 3);
    assert_eq!(vm.stats.jit_returns, 1);
    assert_eq!(vm.stats.jit_deopts, 2);
    assert_eq!(vm.stats.jit_deferred, 8);
    assert_eq!(vm.stats.jit_osr_entries, 1);
    assert!(vm.stats.jit_code_bytes > 0);
    assert!(vm.stats.jit_compile_ns > 0);
}

#[test]
fn cold_loop_stays_adaptive_below_osr_threshold() {
    let source = "def sum_to(n):\n    total=0\n    i=0\n    while i<n:\n        total+=i\n        i+=1\n    return total\nprint(sum_to(10))";
    let program = compile(source, "jit-cold-loop").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.jit_min_instructions = 0;
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"45\n");
    assert_eq!(vm.stats.jit_compiled, 0);
    assert_eq!(vm.stats.jit_osr_entries, 0);
    assert_eq!(vm.stats.jit_deferred, 1);
}

#[test]
fn cranelift_integer_mul_floor_div_and_mod_match_python_semantics() {
    let source = "def arithmetic(a,b):\n    return a*b, a//b, a%b\nprint(arithmetic(-7,3))\nprint(arithmetic(7,-3))";
    let program = compile(source, "jit-integer-ops").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.jit_threshold = 1;
    vm.jit_min_instructions = 0;
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"(-21, -3, 2)\n(-21, -3, -2)\n");
    assert_eq!(vm.stats.jit_compiled, 0);
    assert_eq!(vm.stats.jit_fallbacks, 1);

    let source = "def arithmetic(a,b):\n    return a*b + a//b + a%b\nprint(arithmetic(-7,3))\nprint(arithmetic(7,-3))";
    let program = compile(source, "jit-integer-ops-leaf").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.jit_threshold = 1;
    vm.jit_min_instructions = 0;
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"-22\n-26\n");
    assert_eq!(vm.stats.jit_compiled, 1);
    assert_eq!(vm.stats.jit_calls, 2);
    assert_eq!(vm.stats.jit_returns, 2);
}

#[test]
fn cranelift_runtime_division_survives_collecting_safepoints() {
    let source = "def divide(a,b,c):\n    first=a/b\n    return first/c\nprint(divide(20,2,4))";
    let program = compile(source, "jit-runtime-helper").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.jit_threshold = 1;
    vm.jit_min_instructions = 0;
    vm.gc_interval = Some(1);
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"2.5\n");
    assert_eq!(vm.stats.jit_compiled, 1);
    assert_eq!(vm.stats.jit_returns, 1);
    assert_eq!(vm.stats.jit_helper_calls, 2);
    assert_eq!(vm.stats.jit_safepoints, 2);
    assert!(vm.stats.jit_gc_collections > 0);
}

#[test]
fn cranelift_float_loop_stays_unboxed_through_poll_safepoints() {
    let source = "def accumulate(value,step,n):\n    i=0\n    while i<n:\n        value+=step\n        i+=1\n    return value\nprint(accumulate(0.0,0.5,5000))";
    let program = compile(source, "jit-float-loop").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.jit_osr_threshold = 2;
    vm.jit_min_instructions = 0;
    vm.gc_interval = Some(1);
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"2500.0\n");
    assert_eq!(vm.stats.jit_compiled, 1);
    assert_eq!(vm.stats.jit_osr_entries, 1);
    assert_eq!(vm.stats.jit_deopts, 0);
    assert!(vm.stats.jit_helper_calls >= 4900);
    assert!(vm.stats.heap_allocations < 100);
}

#[test]
fn cranelift_keeps_profiled_float_leaf_unboxed_until_return() {
    let source = "def fused(a,b):\n    x=a+b\n    x=x*b\n    return x-b\ndef loop(n):\n    i=0\n    x=1.0\n    while i<n:\n        x=fused(x,1.000001)\n        i+=1\n    return x\nprint(loop(5000))";
    let program = compile(source, "jit-direct-float").unwrap();

    let mut generic = Vm::new().unwrap();
    generic.execution_mode = ExecutionMode::Jit;
    generic.jit_direct_call_inlining = false;
    generic.gc_interval = Some(1);
    let mut generic_out = Vec::new();
    generic.run(&program, &mut generic_out).unwrap();

    let mut direct = Vm::new().unwrap();
    direct.execution_mode = ExecutionMode::Jit;
    direct.gc_interval = Some(1);
    let mut direct_out = Vec::new();
    direct.run(&program, &mut direct_out).unwrap();

    assert_eq!(direct_out, generic_out);
    assert_eq!(direct.stats.jit_direct_call_sites, 1);
    assert!(direct.stats.jit_direct_calls >= 4_900);
    assert_eq!(direct.stats.jit_deopts, 0);
    assert_eq!(direct.stats.jit_side_exits, 0);
    assert!(generic.stats.heap_allocations >= direct.stats.heap_allocations + 9_500);
    assert!(direct.stats.jit_gc_collections > 0);
}

#[test]
fn cranelift_float_leaf_guard_deopts_before_mixed_type_side_effects() {
    let source = "def fused(a,b):\n    x=a+b\n    x=x*b\n    return x-b\ndef apply(x,step,n):\n    i=0\n    while i<n:\n        x=fused(x,step)\n        i+=1\n    return x\nprint(apply(1.0,1.000001,5000))\nprint(apply(1,2,10))";
    let program = compile(source, "jit-direct-float-guard").unwrap();

    let mut interpreter = Vm::new().unwrap();
    let mut expected = Vec::new();
    interpreter.run(&program, &mut expected).unwrap();

    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.gc_interval = Some(1);
    let mut actual = Vec::new();
    vm.run(&program, &mut actual).unwrap();

    assert_eq!(actual, expected);
    assert!(actual.ends_with(b"3070\n"));
    assert_eq!(vm.stats.jit_direct_call_sites, 1);
    assert!(vm.stats.jit_direct_calls >= 4_900);
    assert!(vm.stats.jit_deopts >= 1);
}

#[test]
fn cranelift_float_leaf_matches_ieee_edge_results_under_stress_gc() {
    let source = "def mul(a,b):\n    return a*b\ndef sub(a,b):\n    return a-b\ndef mul_loop(x,step,n):\n    i=0\n    while i<n:\n        x=mul(x,step)\n        i+=1\n    return x\ndef sub_loop(x,step,n):\n    i=0\n    while i<n:\n        x=sub(x,step)\n        i+=1\n    return x\ninf=1e308*1e308\nprint(mul_loop(inf,2.0,500))\nprint(sub_loop(inf,inf,500))\nprint(mul_loop(-0.0,1.0,500))";
    let program = compile(source, "jit-direct-float-ieee").unwrap();

    let mut interpreter = Vm::new().unwrap();
    let mut expected = Vec::new();
    interpreter.run(&program, &mut expected).unwrap();

    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.gc_interval = Some(1);
    let mut actual = Vec::new();
    vm.run(&program, &mut actual).unwrap();

    assert_eq!(actual, expected);
    assert_eq!(actual, b"inf\nnan\n-0.0\n");
    assert_eq!(vm.stats.jit_direct_call_sites, 2);
    assert!(vm.stats.jit_direct_calls >= 1_300);
    assert_eq!(vm.stats.jit_deopts, 0);
    assert!(vm.stats.jit_gc_collections > 0);
}

#[test]
fn cranelift_inlines_profiled_exact_callee_integer_leaf() {
    let source = "def add(a,b):\n    return a+b\ndef loop(n):\n    i=0\n    s=0\n    while i<n:\n        s=add(s,1)\n        i+=1\n    return s\nprint(loop(5000))";
    let program = compile(source, "jit-direct-call").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.gc_interval = Some(1);
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"5000\n");
    assert_eq!(vm.stats.jit_direct_call_sites, 1);
    assert_eq!(vm.stats.jit_side_exits, 0);
    assert_eq!(vm.stats.jit_deopts, 0);
    assert_eq!(vm.stats.calls, 5_002);
    assert_eq!(vm.stats.jit_direct_calls, 4_937);
    assert!(vm.stats.gc_collections > 0);
    assert!(vm.stats.instructions < 2_000);
}

#[test]
fn cranelift_inlines_keyword_and_default_bound_integer_leaf() {
    let source = "def add(a,/,b=1,*,bias=0):\n    return a+b+bias\ndef loop(n):\n    i=0\n    s=0\n    while i<n:\n        s=add(s,bias=0)\n        i+=1\n    return s\nprint(loop(5000))";
    let program = compile(source, "jit-direct-call-binding").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.gc_interval = Some(1);
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"5000\n");
    assert_eq!(vm.stats.jit_direct_call_sites, 1);
    assert_eq!(vm.stats.jit_side_exits, 0);
    assert_eq!(vm.stats.jit_deopts, 0);
    assert_eq!(vm.stats.calls, 5_002);
    assert_eq!(vm.stats.jit_direct_calls, 4_937);
    assert!(vm.stats.gc_collections > 0);
    assert!(vm.stats.instructions < 2_000);
}

#[test]
fn cranelift_fuses_plain_bound_method_lookup_and_leaf_call() {
    let source = "class Counter:\n    def add(self,a,/,b=1):\n        return a+b\ndef loop(counter,n):\n    i=0\n    s=0\n    while i<n:\n        s=counter.add(s,b=1)\n        i+=1\n    return s\ncounter=Counter()\nprint(loop(counter,5000))\ndef replacement(a,/,b=1):\n    return a+b+b\ncounter.add=replacement\nprint(loop(counter,10))";
    let program = compile(source, "jit-direct-method").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.gc_interval = Some(1);
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"5000\n20\n");
    assert_eq!(vm.stats.jit_direct_method_sites, 1);
    assert_eq!(vm.stats.jit_side_exits, 0);
    assert!(vm.stats.jit_direct_calls >= 4_900);
    assert!(vm.stats.jit_deopts >= 1);
    assert!(vm.stats.gc_collections > 0);
}

#[test]
fn cranelift_direct_method_guard_observes_class_rebinding() {
    let source = "class Counter:\n    def add(self,a,/,b=1):\n        return a+b\ndef replacement(self,a,/,b=1):\n    return a+b+b\ndef loop(counter,n):\n    i=0\n    s=0\n    while i<n:\n        s=counter.add(s,b=1)\n        i+=1\n    return s\ncounter=Counter()\nprint(loop(counter,5000))\nCounter.add=replacement\nprint(loop(counter,10))";
    let program = compile(source, "jit-direct-method-rebind").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"5000\n20\n");
    assert_eq!(vm.stats.jit_direct_method_sites, 1);
    assert!(vm.stats.jit_deopts >= 1);
}

#[test]
fn cranelift_does_not_bypass_custom_getattribute_for_method_calls() {
    let source = "class Counter:\n    def add(self,value):\n        return value+1\n    def __getattribute__(self,name):\n        if name=='add':\n            return lambda value:value+2\n        return object.__getattribute__(self,name)\ndef loop(counter,n):\n    i=0\n    total=0\n    while i<n:\n        total=counter.add(total)\n        i+=1\n    return total\nprint(loop(Counter(),5000))";
    let program = compile(source, "jit-custom-getattribute").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.gc_interval = Some(1);
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"10000\n");
    assert_eq!(vm.stats.jit_direct_method_sites, 0);
    assert!(vm.stats.gc_collections > 0);
}

#[test]
fn cranelift_direct_method_binds_receiver_without_hot_bound_method_allocations() {
    let source = "class Token:\n    def identity(self):\n        return self\ndef loop(token,n):\n    i=0\n    result=None\n    while i<n:\n        result=token.identity()\n        i+=1\n    return result\ntoken=Token()\nprint(isinstance(loop(token,5000),Token))";
    let program = compile(source, "jit-direct-method-receiver").unwrap();

    let mut generic = Vm::new().unwrap();
    generic.execution_mode = ExecutionMode::Jit;
    generic.jit_direct_call_inlining = false;
    let mut generic_out = Vec::new();
    generic.run(&program, &mut generic_out).unwrap();

    let mut direct = Vm::new().unwrap();
    direct.execution_mode = ExecutionMode::Jit;
    let mut direct_out = Vec::new();
    direct.run(&program, &mut direct_out).unwrap();

    assert_eq!(generic_out, b"True\n");
    assert_eq!(direct_out, generic_out);
    assert_eq!(direct.stats.jit_direct_method_sites, 1);
    assert!(direct.stats.jit_direct_calls >= 4_900);
    assert!(generic.stats.heap_allocations >= direct.stats.heap_allocations + 4_800);
}

#[test]
fn cranelift_fuses_staticmethod_without_binding_receiver() {
    let source = "class Math:\n    @staticmethod\n    def add(a,/,b=1):\n        return a+b\ndef loop(math,n):\n    i=0\n    s=0\n    while i<n:\n        s=math.add(s,b=1)\n        i+=1\n    return s\nprint(loop(Math(),5000))";
    let program = compile(source, "jit-direct-staticmethod").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.gc_interval = Some(1);
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"5000\n");
    assert_eq!(vm.stats.jit_direct_method_sites, 1);
    assert_eq!(vm.stats.jit_deopts, 0);
    assert!(vm.stats.jit_direct_calls >= 4_900);
    assert!(vm.stats.jit_helper_calls < 50);
}

#[test]
fn cranelift_method_entry_cache_guards_owner_changes() {
    let source = "class Counter:\n    def add(self,a,b):\n        return a+b\ndef loop(first,second,n):\n    i=0\n    total=0\n    while i<n:\n        if i%2==0:\n            owner=first\n        else:\n            owner=second\n        total=owner.add(total,1)\n        i+=1\n    return total\nprint(loop(Counter(),Counter(),5000))";
    let program = compile(source, "jit-method-owner-guard").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"5000\n");
    assert_eq!(vm.stats.jit_direct_method_sites, 1);
    assert!(vm.stats.jit_deopts >= 1);
}

#[test]
fn cranelift_staticmethod_guard_includes_binding_kind() {
    let source = "class Math:\n    @staticmethod\n    def add(a,/,b=1):\n        return a+b\ndef loop(math,n):\n    i=0\n    s=0\n    while i<n:\n        s=math.add(s,b=1)\n        i+=1\n    return s\nmath=Math()\nraw=Math.add\nprint(loop(math,5000))\nMath.add=raw\nloop(math,1)";
    let program = compile(source, "jit-direct-staticmethod-kind").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    let mut out = Vec::new();
    let error = vm.run(&program, &mut out).unwrap_err();
    assert_eq!(out, b"5000\n");
    assert_eq!(error.kind, "TypeError");
    assert_eq!(vm.stats.jit_direct_method_sites, 1);
    assert!(vm.stats.jit_deopts >= 1);
}

#[test]
fn cranelift_fuses_classmethod_with_dynamic_subclass_receiver() {
    let source = "class Base:\n    @classmethod\n    def identity(cls):\n        return cls\nclass Sub(Base):\n    marker=1\ndef loop(value,n):\n    i=0\n    result=None\n    while i<n:\n        result=value.identity()\n        i+=1\n    return result\nprint(loop(Sub(),5000).__name__)";
    let program = compile(source, "jit-direct-classmethod").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.gc_interval = Some(1);
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"Sub\n");
    assert_eq!(vm.stats.jit_direct_method_sites, 1);
    assert_eq!(vm.stats.jit_deopts, 0);
    assert!(vm.stats.jit_direct_calls >= 4_900);
}

#[test]
fn cranelift_classmethod_guard_includes_binding_kind() {
    let source = "class Base:\n    @classmethod\n    def identity(cls):\n        return cls\ndef loop(value,n):\n    i=0\n    result=None\n    while i<n:\n        result=value.identity()\n        i+=1\n    return result\nvalue=Base()\nraw=Base.identity.__func__\nprint(loop(value,5000).__name__)\nBase.identity=raw\nprint(isinstance(loop(value,1),Base))";
    let program = compile(source, "jit-direct-classmethod-kind").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"Base\nTrue\n");
    assert_eq!(vm.stats.jit_direct_method_sites, 1);
    assert!(vm.stats.jit_deopts >= 1);
}

#[test]
fn cranelift_fuses_class_level_function_and_classmethod_access() {
    let source = "class Math:\n    def add(a,/,b=1):\n        return a+b\nclass Base:\n    @classmethod\n    def identity(cls):\n        return cls\nclass Sub(Base):\n    marker=1\ndef sum_loop(n):\n    i=0\n    s=0\n    while i<n:\n        s=Math.add(s,b=1)\n        i+=1\n    return s\ndef class_loop(n):\n    i=0\n    result=None\n    while i<n:\n        result=Sub.identity()\n        i+=1\n    return result\nprint(sum_loop(5000))\nprint(class_loop(5000).__name__)";
    let program = compile(source, "jit-direct-class-access").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.gc_interval = Some(1);
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"5000\nSub\n");
    assert_eq!(vm.stats.jit_direct_method_sites, 2);
    assert_eq!(vm.stats.jit_deopts, 0);
    assert!(vm.stats.jit_direct_calls >= 9_800);
}

#[test]
fn cranelift_resumes_custom_descriptor_then_inlines_returned_leaf() {
    let source = "def add(a,/,b=1):\n    return a+b\nclass Forward:\n    def __get__(self,obj,owner):\n        return add\nclass Math:\n    op=Forward()\ndef loop(math,n):\n    i=0\n    total=0\n    while i<n:\n        total=math.op(total,b=1)\n        i+=1\n    return total\nprint(loop(Math(),5000))";
    let program = compile(source, "jit-custom-descriptor").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.gc_interval = Some(1);
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"5000\n");
    assert_eq!(vm.stats.jit_direct_method_sites, 0);
    assert_eq!(vm.stats.jit_deopts, 0);
    assert!(vm.stats.jit_side_exits >= 4_900);
    assert!(vm.stats.jit_resumes >= 4_900);
    assert!(vm.stats.jit_direct_calls >= 4_900);
}

#[test]
fn cranelift_custom_descriptor_result_change_deopts_at_call() {
    let source = "def add(a,b):\n    return a+b\ndef other(a,b):\n    return a+b+b\ntarget=add\nclass Forward:\n    def __get__(self,obj,owner):\n        return target\nclass Math:\n    op=Forward()\ndef loop(math,n):\n    i=0\n    total=0\n    while i<n:\n        total=math.op(total,1)\n        i+=1\n    return total\nmath=Math()\nprint(loop(math,5000))\ntarget=other\nprint(loop(math,1))";
    let program = compile(source, "jit-custom-descriptor-rebind").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"5000\n2\n");
    assert_eq!(vm.stats.jit_direct_method_sites, 0);
    assert!(vm.stats.jit_direct_calls >= 4_900);
    assert!(vm.stats.jit_deopts >= 1);
}

#[test]
fn cranelift_inlines_flat_expanded_call_under_stress_gc() {
    let source = "def add(a,b):\n    return a+b\ndef loop(values,n):\n    i=0\n    total=0\n    while i<n:\n        total=add(total,*values)\n        i+=1\n    return total\nprint(loop([1],5000))";
    let program = compile(source, "jit-expanded-call").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.gc_interval = Some(1);
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"5000\n");
    assert!(vm.stats.jit_compiled >= 1);
    assert_eq!(vm.stats.jit_deopts, 0);
    assert_eq!(vm.stats.jit_direct_call_sites, 1);
    assert!(vm.stats.jit_direct_calls >= 4_900);
    assert_eq!(vm.stats.jit_side_exits, 0);
    assert_eq!(vm.stats.jit_resumes, 0);
}

#[test]
fn cranelift_expanded_sequence_guard_observes_items_and_length_changes() {
    let source = "def add(a,b,c):\n    return a+b+c\ndef loop(values,n):\n    i=0\n    total=0\n    while i<n:\n        total=add(total,*values)\n        i+=1\n    return total\nvalues=[1,0]\nprint(loop(values,5000))\nvalues[0]=2\nprint(loop(values,5000))";
    let program = compile(source, "jit-expanded-item-mutation").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.gc_interval = Some(1);
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"5000\n10000\n");
    assert_eq!(vm.stats.jit_direct_call_sites, 1);
    assert!(vm.stats.jit_direct_calls >= 4_800);
    assert_eq!(vm.stats.jit_deopts, 0);

    let source = "def add(a,b):\n    return a+b\ndef loop(values,n):\n    i=0\n    total=0\n    while i<n:\n        total=add(total,*values)\n        i+=1\n    return total\nprint(loop([1],5000))\nprint(loop([1,2],5000))";
    let program = compile(source, "jit-expanded-length-guard").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    let error = vm.run(&program, &mut Vec::new()).unwrap_err();
    assert_eq!(error.kind, "TypeError");
    assert!(vm.stats.jit_deopts >= 1);
}

#[test]
fn cranelift_defers_non_loop_expansion_until_profile_is_ready() {
    let source = "def add(a,b):\n    return a+b\ndef invoke(values):\n    return add(1,*values)\ni=0\ntotal=0\nwhile i<20:\n    total+=invoke([1])\n    i+=1\nprint(total)";
    let program = compile(source, "jit-expanded-entry-profile").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.jit_min_instructions = 0;
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"40\n");
    assert_eq!(vm.stats.jit_direct_call_sites, 1);
    assert!(vm.stats.jit_direct_calls >= 10);
    assert_eq!(vm.stats.jit_side_exits, 0);
}

#[test]
fn cranelift_binds_named_and_default_slots_in_expanded_direct_call() {
    let source = "def add(a,/,b=1,*,bias=0):\n    return a+b+bias\ndef loop(values,n):\n    i=0\n    total=0\n    while i<n:\n        total=add(total,*values,bias=0)\n        i+=1\n    return total\nprint(loop([1],5000))";
    let program = compile(source, "jit-expanded-named").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.gc_interval = Some(1);
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"5000\n");
    assert_eq!(vm.stats.jit_direct_call_sites, 1);
    assert!(vm.stats.jit_direct_calls >= 4_900);
    assert_eq!(vm.stats.jit_side_exits, 0);
    assert_eq!(vm.stats.jit_deopts, 0);
}

#[test]
fn cranelift_binds_mapping_values_in_expanded_direct_call() {
    let source = "def add(a,/,b=1,*,bias=0):\n    return a+b+bias\ndef loop(mapping,n):\n    i=0\n    total=0\n    while i<n:\n        total=add(total,**mapping)\n        i+=1\n    return total\nmapping={'b':1,'bias':0}\nprint(loop(mapping,5000))\nmapping['bias']=1\nprint(loop(mapping,5000))";
    let program = compile(source, "jit-expanded-mapping").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.gc_interval = Some(1);
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"5000\n10000\n");
    assert_eq!(vm.stats.jit_direct_call_sites, 1);
    assert!(vm.stats.jit_direct_calls >= 9_800);
    assert_eq!(vm.stats.jit_side_exits, 0);
    assert_eq!(vm.stats.jit_deopts, 0);
}

#[test]
fn cranelift_mapping_guard_deopts_when_key_set_changes() {
    let source = "def add(a,/,b=1,*,bias=0):\n    return a+b+bias\ndef loop(mapping,n):\n    i=0\n    total=0\n    while i<n:\n        total=add(total,b=1,**mapping)\n        i+=1\n    return total\nprint(loop({'bias':0},5000))\nprint(loop({'bias':0,'other':1},5000))";
    let program = compile(source, "jit-expanded-mapping-keys").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    let error = vm.run(&program, &mut Vec::new()).unwrap_err();
    assert_eq!(error.kind, "TypeError");
    assert!(vm.stats.jit_deopts >= 1);
}

#[test]
fn cranelift_expanded_segment_waits_for_matching_nested_builder() {
    let source = "def pack(*args,**kw):\n    return 0\ndef add(a,b,**kw):\n    return a+b\ndef loop(one,inner,extra,n):\n    i=0\n    total=0\n    while i<n:\n        total=add(total,*one,x=pack(*inner,y=3),**extra)\n        i+=1\n    return total\nprint(loop([1],[2],{'z':4},5000))";
    let program = compile(source, "jit-expanded-nested").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.gc_interval = Some(1);
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"5000\n");
    assert!(vm.stats.jit_compiled >= 1);
    assert_eq!(vm.stats.jit_deopts, 0);
    assert!(vm.stats.jit_side_exits >= 4_900);
    assert!(vm.stats.jit_side_exits < 5_100);
    assert!(vm.stats.jit_resumes >= 4_900);
}

#[test]
fn cranelift_omits_unobserved_and_materializes_observed_variadics() {
    let source = "def add(a,b,*rest,**kw):\n    return a+b\ndef loop(n):\n    i=0\n    total=0\n    while i<n:\n        total=add(total,1)\n        i+=1\n    return total\nprint(loop(5000))";
    let program = compile(source, "jit-unused-variadic").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.gc_interval = Some(1);
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"5000\n");
    assert_eq!(vm.stats.jit_direct_call_sites, 1);
    assert_eq!(vm.stats.jit_deopts, 0);
    assert!(vm.stats.jit_direct_calls >= 4_900);

    let source = "def reveal(a,*rest,**kw):\n    return rest\ndef loop(n):\n    i=0\n    result=None\n    while i<n:\n        result=reveal(1,2,3)\n        i+=1\n    return result\nprint(loop(100))";
    let program = compile(source, "jit-observed-variadic").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"(2, 3)\n");
    assert_eq!(vm.stats.jit_direct_call_sites, 1);
    assert!(vm.stats.jit_direct_calls >= 20);
    assert_eq!(vm.stats.jit_side_exits, 0);
    assert_eq!(vm.stats.jit_deopts, 0);
}

#[test]
fn cranelift_materializes_observed_variadic_parameters_in_direct_calls() {
    let source = "def collect_args(a,*args,b=0):\n    return args\ndef collect_kw(a=0,/,*,b=0,**kw):\n    return kw\ndef loop_args(n):\n    i=0\n    result=None\n    while i<n:\n        result=collect_args(0,1,2,b=9)\n        i+=1\n    return result\ndef loop_kw(n):\n    i=0\n    result=None\n    while i<n:\n        result=collect_kw(b=9,a=1,y=2)\n        i+=1\n    return result\nprint(loop_args(5000))\nprint(loop_kw(5000))";
    let program = compile(source, "jit-materialized-variadics").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.gc_interval = Some(1);
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"(1, 2)\n{'a': 1, 'y': 2}\n");
    assert_eq!(vm.stats.jit_direct_call_sites, 2);
    assert!(vm.stats.jit_direct_calls >= 9_800);
    assert_eq!(vm.stats.jit_side_exits, 0);
    assert_eq!(vm.stats.jit_deopts, 0);
    assert!(vm.stats.jit_gc_collections >= 9_800);
}

#[test]
fn cranelift_direct_call_guards_callee_and_argument_tags() {
    let source = "def add(a,b):\n    return a+b\ndef other(a,b):\n    return a+b+b\ndef loop(n):\n    i=0\n    s=0\n    while i<n:\n        s=add(s,1)\n        i+=1\n    return s\nprint(loop(200))\nadd=other\nprint(loop(10))";
    let program = compile(source, "jit-direct-call-rebind").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"200\n20\n");
    assert_eq!(vm.stats.jit_direct_call_sites, 1);
    assert_eq!(vm.stats.jit_deopts, 1);
    assert_eq!(vm.stats.call_cache_misses, 1);

    let source = "def add(a,b):\n    return a+b\ndef apply(a,b):\n    return add(a,b)\ni=0\nwhile i<10:\n    apply(i,1)\n    i+=1\nprint(apply(1.5,2.5))";
    let program = compile(source, "jit-direct-call-types").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.jit_min_instructions = 0;
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"4.0\n");
    assert_eq!(vm.stats.jit_direct_call_sites, 1);
    assert!(vm.stats.jit_deopts >= 1);
}

#[test]
fn cranelift_runtime_error_preserves_kind_and_source_span() {
    let source = "def divide(a,b):\n    return a/b\nprint(divide(1,0))";
    let program = compile(source, "jit-runtime-error").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.jit_threshold = 1;
    vm.jit_min_instructions = 0;
    let error = vm.run(&program, &mut Vec::new()).unwrap_err();
    assert_eq!(error.kind, "ZeroDivisionError");
    assert_eq!(error.span, Some(program.program().code[1].spans[2]));
    assert_eq!(vm.stats.jit_helper_calls, 1);
    assert_eq!(vm.stats.jit_runtime_errors, 1);
}

#[test]
fn cranelift_recursive_calls_resume_through_vm_frames_and_rebinding() {
    let source = "def recur(n):\n    if n<1:\n        return 0\n    return target(n-1)\ntarget=recur\ni=0\nwhile i<8:\n    recur(2)\n    i+=1\ndef replacement(n):\n    return 40+n\ntarget=replacement\nprint(recur(2))";
    let program = compile(source, "jit-recursive-resume").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.jit_threshold = 1;
    vm.jit_min_instructions = 0;
    vm.gc_interval = Some(1);
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"41\n");
    assert!(vm.stats.jit_compiled >= 2);
    assert!(vm.stats.jit_side_exits > 0);
    assert_eq!(vm.stats.jit_side_exits, vm.stats.jit_resumes);
    assert_eq!(vm.stats.jit_helper_calls, 0);
}

#[test]
fn cranelift_global_helper_preserves_name_error_location() {
    let source = "def read_missing():\n    return missing\nprint(read_missing())";
    let program = compile(source, "jit-global-error").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.jit_threshold = 1;
    vm.jit_min_instructions = 0;
    let error = vm.run(&program, &mut Vec::new()).unwrap_err();
    assert_eq!(error.kind, "NameError");
    assert_eq!(error.span, Some(program.program().code[1].spans[0]));
    assert_eq!(vm.stats.jit_runtime_errors, 1);
}

#[test]
fn unstable_jit_site_despecializes_after_bounded_guard_failures() {
    let source = "def add(a,b):\n    return a+b\ni=0\nwhile i<20:\n    add(1.5,i)\n    i+=1";
    let program = compile(source, "jit-despecialize").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.jit_min_instructions = 0;
    vm.run(&program, &mut Vec::new()).unwrap();
    assert_eq!(vm.stats.jit_compiled, 1);
    assert_eq!(vm.stats.jit_calls, 8);
    assert_eq!(vm.stats.jit_deopts, 8);
    assert_eq!(vm.stats.jit_despecialized, 1);
    assert_eq!(vm.stats.jit_deferred, 7);
    assert_eq!(vm.stats.jit_fallbacks, 0);
}

#[test]
fn tiny_leaf_function_stays_in_profitable_adaptive_tier() {
    let source =
        "def add(a,b):\n    return a+b\ni=0\ns=0\nwhile i<20:\n    s=add(s,1)\n    i+=1\nprint(s)";
    let program = compile(source, "jit-profitability").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"20\n");
    assert_eq!(vm.stats.jit_compiled, 0);
    assert_eq!(vm.stats.jit_unprofitable, 1);
    assert_eq!(vm.stats.jit_fallbacks, 0);
    assert_eq!(vm.stats.call_quickened, 1);
}

#[test]
fn jit_code_budget_rejects_native_code_and_preserves_interpreter_execution() {
    let source = "def add(a,b):\n    x=a+b\n    return x\ni=0\ns=0\nwhile i<20:\n    s=add(s,1)\n    i+=1\nprint(s)";
    let program = compile(source, "jit-code-budget").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.jit_threshold = 1;
    vm.jit_min_instructions = 0;
    vm.jit_max_code_bytes = 0;
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"20\n");
    assert_eq!(vm.stats.jit_compile_attempts, 1);
    assert_eq!(vm.stats.jit_compiled, 0);
    assert_eq!(vm.stats.jit_code_budget_rejections, 1);
    assert_eq!(vm.stats.jit_fallbacks, 1);
    assert_eq!(vm.stats.jit_calls, 0);
}

#[test]
fn adaptive_integer_binary_sites_quicken_and_despecialize() {
    let source = "def add(a,b):\n    return a+b\ni=0\nwhile i<10:\n    add(i,2)\n    i+=1\nprint(add(1.5,2))\ni=0\nwhile i<9:\n    add(i,3)\n    i+=1\nprint(add(4,5))";
    let program = compile(source, "adaptive-binary").unwrap();
    let mut vm = Vm::new().unwrap();
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"3.5\n9\n");
    assert_eq!(vm.stats.quickened, 4);
    assert_eq!(vm.stats.quickened_misses, 1);
}

#[test]
fn persistent_callable_cannot_alias_code_in_another_execution() {
    use std::sync::Mutex;
    use tonic_runtime::PersistentHandle;
    static SAVED: Mutex<Option<PersistentHandle>> = Mutex::new(None);
    fn save(ctx: &mut Context<'_>, args: &[Handle]) -> Result<Handle> {
        *SAVED.lock().unwrap() = Some(ctx.persist(args[0])?);
        ctx.none()
    }
    fn load(ctx: &mut Context<'_>, _: &[Handle]) -> Result<Handle> {
        let saved = SAVED.lock().unwrap().take().unwrap();
        let local = ctx.borrow_persistent(&saved)?;
        ctx.release_persistent(&saved)?;
        Ok(local)
    }
    let mut vm = Vm::new().unwrap();
    vm.register_native("cache", "save", 1, save).unwrap();
    vm.register_native("cache", "load", 0, load).unwrap();
    let first = compile(
        "import cache\ndef f():\n    return 42\ncache.save(f)",
        "first",
    )
    .unwrap();
    vm.run(&first, &mut Vec::new()).unwrap();
    let second = compile("import cache\ncache.load()()", "second").unwrap();
    assert_eq!(
        vm.run(&second, &mut Vec::new()).unwrap_err().kind,
        "RuntimeError"
    );
    assert_eq!(vm.active_handles(), 0);
}

#[test]
fn augmented_list_add_preserves_aliases_and_self_extension() {
    assert_eq!(
        output("a=[1]\nb=a\na += (2,3)\na += a\nprint(a,b)\na += range(2)\nprint(b)"),
        "[1, 2, 3, 1, 2, 3] [1, 2, 3, 1, 2, 3]\n[1, 2, 3, 1, 2, 3, 0, 1]\n"
    );
}

#[test]
fn cyclic_container_repr_and_trace_edges() {
    assert_eq!(output("a=[]\na += (a,)\nprint(a)"), "[[...]]\n");
    let mut vm = Vm::new().unwrap();
    vm.run(&compile("a=[]\na += (a,)", "x").unwrap(), &mut Vec::new())
        .unwrap();
    assert!(vm.root_and_edge_counts().1 >= 2);
}

#[test]
fn source_modules_isolate_versioned_globals_cache_once_and_support_cycles() {
    let program = compile_modules(
        "__main__",
        &[
            ModuleSource::new(
                "__main__",
                "main.tonic",
                "import alpha\nimport alpha\nimport beta\nprint(alpha.value,alpha.from_beta,beta.seen,hasattr(alpha,'print'))\nalpha.value='changed'\nprint(alpha.read())",
            ),
            ModuleSource::new(
                "alpha",
                "alpha.tonic",
                "print('load-alpha')\nvalue='alpha-start'\nimport beta\nfrom_beta=beta.value\nvalue='alpha-done'\ndef read(): return value",
            ),
            ModuleSource::new(
                "beta",
                "beta.tonic",
                "print('load-beta')\nimport alpha\nseen=alpha.value\nvalue='beta-done'",
            ),
        ],
    )
    .unwrap();
    let mut vm = Vm::new().unwrap();
    vm.gc_interval = Some(1);
    let mut output = Vec::new();
    vm.run(&program, &mut output).unwrap();
    assert_eq!(
        output,
        b"load-alpha\nload-beta\nalpha-done beta-done alpha-start False\nchanged\n"
    );
    assert!(vm
        .module_version("alpha")
        .is_some_and(|version| version >= 4));
    assert!(vm
        .module_version("beta")
        .is_some_and(|version| version >= 3));
}

#[test]
fn source_packages_support_dotted_and_from_imports() {
    let program = compile_modules(
        "__main__",
        &[
            ModuleSource::new(
                "__main__",
                "main.tonic",
                "from package import child as first\nfrom package import answer\nimport package.child as leaf\nimport package.child\nprint(answer,first.value,leaf.value,package.child.value,first.Exported.__module__,package.__name__,first.__name__,first.__file__)",
            ),
            ModuleSource::new(
                "package",
                "package/__init__.tonic",
                "print('load-package')\nanswer=40",
            ),
            ModuleSource::new(
                "package.child",
                "package/child.tonic",
                "print('load-child')\nvalue=2\nclass Exported:\n    pass",
            ),
        ],
    )
    .unwrap();
    for mode in [ExecutionMode::Interpreter, ExecutionMode::Jit] {
        let mut vm = Vm::new().unwrap();
        vm.execution_mode = mode;
        vm.gc_interval = Some(1);
        let mut output = Vec::new();
        vm.run(&program, &mut output).unwrap();
        assert_eq!(
            output,
            b"load-package\nload-child\n40 2 2 2 package.child package package.child package/child.tonic\n"
        );
        assert!(vm
            .module_version("package")
            .is_some_and(|version| version >= 2));
    }
}

#[test]
fn failed_source_import_rolls_back_partial_globals_and_retries() {
    let program = compile_modules(
        "__main__",
        &[
            ModuleSource::new(
                "__main__",
                "main.tonic",
                "import control\ntry:\n    import flaky\nexcept ValueError:\n    print(control.attempts,hasattr(control.partial,'value'))\nimport flaky\nprint(control.attempts,flaky.value,control.partial.value)",
            ),
            ModuleSource::new(
                "control",
                "control.tonic",
                "attempts=0\npartial=None",
            ),
            ModuleSource::new(
                "flaky",
                "flaky.tonic",
                "import control\ncontrol.attempts+=1\nimport flaky\ncontrol.partial=flaky\nvalue='partial'\nif control.attempts==1:\n    raise ValueError('first')\nvalue='done'",
            ),
        ],
    )
    .unwrap();
    for mode in [ExecutionMode::Interpreter, ExecutionMode::Jit] {
        let mut vm = Vm::new().unwrap();
        vm.execution_mode = mode;
        vm.gc_interval = Some(1);
        let mut output = Vec::new();
        vm.run(&program, &mut output).unwrap();
        assert_eq!(output, b"1 False\n2 done done\n");
    }
}

#[test]
fn jit_compiles_imported_functions_and_reads_mutated_module_globals() {
    let program = compile_modules(
        "__main__",
        &[
            ModuleSource::new(
                "__main__",
                "main.tonic",
                "import worker\ni=0\nwhile i<20:\n    result=worker.compute(1)\n    i+=1\nprint(result)\nworker.base=100\ni=0\nwhile i<20:\n    result=worker.compute(1)\n    i+=1\nprint(result)",
            ),
            ModuleSource::new(
                "worker",
                "worker.tonic",
                "base=40\ndef compute(x):\n    x=x+base\n    x=x+1\n    x=x+1\n    x=x+1\n    x=x+1\n    x=x+1\n    return x",
            ),
        ],
    )
    .unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.gc_interval = Some(1);
    let mut output = Vec::new();
    vm.run(&program, &mut output).unwrap();
    assert_eq!(output, b"46\n106\n");
    assert!(vm.stats.jit_compiled >= 1, "{:?}", vm.stats);
    assert!(vm.stats.jit_calls >= 1, "{:?}", vm.stats);
}
