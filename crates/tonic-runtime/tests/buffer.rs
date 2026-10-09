use tonic_compiler::compile;
use tonic_runtime::{ExecutionMode, Vm};

#[test]
fn f64_backing_allocation_stays_stable_when_gc_moves_its_owner() {
    let mut vm = Vm::new().unwrap();
    let (persistent, data_before, shape_before, strides_before) = {
        let mut context = vm.context().unwrap();
        let buffer = context
            .from_f64_buffer(&[1.0, 2.0, 3.0, 4.0], &[2, 2], false)
            .unwrap();
        let persistent = context.persist(buffer).unwrap();
        let view = context.f64_buffer(buffer).unwrap();
        assert_eq!(view.shape(), &[2, 2]);
        assert_eq!(view.strides(), &[16, 8]);
        assert!(!view.is_writable());
        (
            persistent,
            view.as_slice().as_ptr() as usize,
            view.shape().as_ptr() as usize,
            view.strides().as_ptr() as usize,
        )
    };
    let collection = vm.collect_garbage().unwrap();
    assert!(collection.moved > 0);
    let mut context = vm.context().unwrap();
    let buffer = context.borrow_persistent(&persistent).unwrap();
    {
        let view = context.f64_buffer(buffer).unwrap();
        assert_eq!(view.as_slice(), &[1.0, 2.0, 3.0, 4.0]);
        assert_eq!(view.as_slice().as_ptr() as usize, data_before);
        assert_eq!(view.shape().as_ptr() as usize, shape_before);
        assert_eq!(view.strides().as_ptr() as usize, strides_before);
    }
    context.release_persistent(&persistent).unwrap();
}

#[test]
fn shape_product_and_sequence_elements_are_validated() {
    let mut vm = Vm::new().unwrap();
    let mut context = vm.context().unwrap();
    assert_eq!(
        context
            .from_f64_buffer(&[1.0, 2.0], &[3], false)
            .unwrap_err()
            .kind,
        "BufferError"
    );
    drop(context);
    let program = compile("import fastmath\nfastmath.array([1.0,'bad'])", "buffer").unwrap();
    assert_eq!(
        vm.run(&program, &mut Vec::new()).unwrap_err().kind,
        "TypeError"
    );
    assert_eq!(vm.active_handles(), 0);
}

#[test]
fn fastmath_sum_reads_one_typed_buffer_without_element_boxing_or_copying() {
    let program = compile(
        "import fastmath\nvalues=fastmath.array([1.0,2.0,3.0,4.0])\nprint(len(values),fastmath.sum(values))",
        "buffer",
    )
    .unwrap();
    let mut vm = Vm::new().unwrap();
    vm.gc_interval = Some(1);
    let mut output = Vec::new();
    vm.run(&program, &mut output).unwrap();
    assert_eq!(output, b"4 10.0\n");
    assert_eq!(vm.stats.buffer_copies, 1);
    assert_eq!(vm.stats.buffer_exports, 1);
    assert_eq!(vm.stats.native_calls, 2);
    assert!(vm.stats.gc_collections > 0);
    assert_eq!(vm.active_handles(), 0);
}

#[test]
fn fastmath_buffer_has_a_stable_runtime_type_for_annotations() {
    let program = compile(
        concat!(
            "import fastmath\n",
            "values=fastmath.array([1.0,2.0])\n",
            "alias=fastmath.Buffer[float,1,False]\n",
            "print(alias.__origin__ is fastmath.Buffer,",
            "alias.__args__==(float,1,False))\n",
            "print(type(values) is fastmath.Buffer)\n",
            "print(isinstance(values,fastmath.Buffer))\n",
            "def total(values:fastmath.Buffer)->float:\n",
            "    return fastmath.sum(values)\n",
            "print(total(values))\n",
        ),
        "buffer-type",
    )
    .unwrap();
    let mut vm = Vm::new().unwrap();
    vm.gc_interval = Some(1);
    let mut output = Vec::new();
    vm.run(&program, &mut output).unwrap();
    assert_eq!(output, b"True True\nTrue\nTrue\n3.0\n");
    assert!(vm.stats.gc_collections > 0);
    assert_eq!(vm.active_handles(), 0);
}

#[test]
fn buffer_annotation_guards_first_call_jit_without_enforcing_the_hint() {
    let program = compile(
        concat!(
            "import fastmath\n",
            "sum_buffer=fastmath.sum\n",
            "def total(values:fastmath.Buffer[float,1,False])->float:\n",
            "    return sum_buffer(values)\n",
            "def rank_two(values:fastmath.Buffer[float,2,False])->float:\n",
            "    return sum_buffer(values)\n",
            "def need_writable(values:fastmath.Buffer[float,...,True])->float:\n",
            "    return sum_buffer(values)\n",
            "values=fastmath.array([1.0,2.0,3.0])\n",
            "print(total(values))\n",
            "print(total([4.0,5.0]))\n",
            "print(rank_two(values),need_writable(values))\n",
        ),
        "buffer-annotation-jit",
    )
    .unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.gc_interval = Some(1);
    let mut output = Vec::new();
    vm.run(&program, &mut output).unwrap();
    assert_eq!(output, b"6.0\n9.0\n6.0 6.0\n");
    assert_eq!(vm.stats.jit_annotation_candidates, 3);
    assert_eq!(vm.stats.jit_annotation_compiled, 1);
    assert_eq!(vm.stats.jit_compiled, 1);
    assert_eq!(vm.stats.jit_annotation_guard_misses, 3);
    assert!(vm.stats.jit_side_exits >= 1);
    assert!(vm.stats.jit_resumes >= 1);
    assert!(vm.stats.gc_collections > 0);
}

#[test]
fn annotated_rank_one_buffer_loop_loads_f64_values_in_native_code() {
    let program = compile(
        concat!(
            "import fastmath\n",
            "def total(values:fastmath.Buffer[float,1,False], count:int)->float:\n",
            "    index=0\n",
            "    result=0.0\n",
            "    while index<count:\n",
            "        result=result+values[index]\n",
            "        index+=1\n",
            "    return result\n",
            "values=fastmath.array([1.0,2.0,3.0,4.0])\n",
            "print(values[0],values[-1])\n",
            "print(total(values,4),total(values,2))\n",
            "try:\n",
            "    total(values,5)\n",
            "except IndexError:\n",
            "    print('loop-bounds')\n",
        ),
        "buffer-native-load",
    )
    .unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.gc_interval = Some(1);
    let mut output = Vec::new();
    vm.run(&program, &mut output).unwrap();
    assert_eq!(output, b"1.0 4.0\n10.0 3.0\nloop-bounds\n");
    assert_eq!(vm.stats.jit_annotation_candidates, 1);
    assert_eq!(vm.stats.jit_annotation_compiled, 1);
    assert_eq!(vm.stats.jit_compiled, 1);
    assert_eq!(vm.stats.jit_side_exits, 0);
    assert_eq!(vm.stats.jit_f64_buffer_parameters, 1);
    assert_eq!(vm.stats.jit_f64_buffer_item_sites, 1);
    assert_eq!(vm.stats.jit_f64_buffer_bounds_elided_sites, 1);
    assert!(vm.stats.jit_deopts >= 1);
    assert!(vm.stats.jit_helper_calls >= 4);
    assert!(vm.stats.gc_collections > 0);
}

#[test]
fn native_buffer_load_preserves_negative_and_out_of_bounds_index_semantics() {
    let program = compile(
        concat!(
            "import fastmath\n",
            "def load(values:fastmath.Buffer[float,1,False], index:int)->float:\n",
            "    return values[index]\n",
            "values=fastmath.array([1.0,2.0,3.0])\n",
            "print(load(values,-1))\n",
            "try:\n",
            "    load(values,3)\n",
            "except IndexError:\n",
            "    print('bounds')\n",
        ),
        "buffer-native-bounds",
    )
    .unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.gc_interval = Some(1);
    let mut output = Vec::new();
    vm.run(&program, &mut output).unwrap();
    assert_eq!(output, b"3.0\nbounds\n");
    assert_eq!(vm.stats.jit_annotation_compiled, 1);
    assert_eq!(vm.stats.jit_f64_buffer_item_sites, 1);
    assert!(vm.stats.jit_deopts >= 1);
    assert!(vm.stats.gc_collections > 0);
}
