use tonic_runtime::Vm;
#[test]
fn locals_are_invalid_after_scope_even_when_slot_reused() {
    let mut vm = Vm::new().unwrap();
    let old = {
        let mut ctx = vm.context().unwrap();
        ctx.from_i64(42).unwrap()
    };
    assert_eq!(vm.active_handles(), 0);
    let mut ctx = vm.context().unwrap();
    let new = ctx.from_i64(9).unwrap();
    assert_eq!(ctx.to_i64(new).unwrap(), 9);
    assert_eq!(ctx.to_i64(old).unwrap_err().kind, "HandleError");
}
#[test]
fn foreign_runtime_handles_are_rejected() {
    let mut a = Vm::new().unwrap();
    let mut b = Vm::new().unwrap();
    let mut ca = a.context().unwrap();
    let mut cb = b.context().unwrap();
    let h = ca.from_i64(42).unwrap();
    let _ = cb.from_i64(99).unwrap();
    assert_eq!(cb.to_i64(h).unwrap_err().kind, "HandleError");
}
#[test]
fn persistent_survives_local_scope_and_validates_release() {
    let mut vm = Vm::new().unwrap();
    let p = {
        let mut ctx = vm.context().unwrap();
        let local = ctx.from_str("kept").unwrap();
        ctx.persist(local).unwrap()
    };
    assert_eq!(vm.active_handles(), 1);
    {
        let mut ctx = vm.context().unwrap();
        let local = ctx.borrow_persistent(&p).unwrap();
        assert_eq!(ctx.as_str(local).unwrap(), "kept");
        ctx.release_persistent(&p).unwrap();
        assert_eq!(ctx.as_str(local).unwrap(), "kept");
        assert_eq!(ctx.borrow_persistent(&p).unwrap_err().kind, "HandleError");
        assert_eq!(ctx.release_persistent(&p).unwrap_err().kind, "HandleError");
    }
    assert_eq!(vm.active_handles(), 0);
}
#[test]
fn wrong_runtime_release_does_not_consume_token() {
    let mut a = Vm::new().unwrap();
    let mut b = Vm::new().unwrap();
    let p = {
        let mut c = a.context().unwrap();
        let h = c.from_i64(9).unwrap();
        c.persist(h).unwrap()
    };
    assert!(b.context().unwrap().release_persistent(&p).is_err());
    a.context().unwrap().release_persistent(&p).unwrap();
}
#[test]
fn primitive_conversions_and_heap_growth() {
    let mut vm = Vm::new().unwrap();
    let mut ctx = vm.context().unwrap();
    let s = ctx.from_str("first").unwrap();
    for _ in 0..10000 {
        ctx.from_str("growth").unwrap();
    }
    assert_eq!(ctx.as_str(s).unwrap(), "first");
    let n = ctx.from_i64(i64::MAX).unwrap();
    assert_eq!(ctx.to_i64(n).unwrap(), i64::MAX);
    let n = ctx.from_f64(2.5).unwrap();
    assert_eq!(ctx.to_f64(n).unwrap(), 2.5);
    assert!(ctx.to_i64(n).is_err());
}
