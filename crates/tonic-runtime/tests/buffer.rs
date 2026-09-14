use tonic_compiler::compile;
use tonic_runtime::Vm;

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
