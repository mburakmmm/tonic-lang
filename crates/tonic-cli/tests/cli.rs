use std::process::Command;
fn tonic(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_tonic"))
        .args(args)
        .output()
        .unwrap()
}
#[test]
fn execute_file() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/fib.tonic");
    let out = tonic(&[path]);
    assert!(out.status.success());
    assert_eq!(out.stdout, b"102334155\n");
}
#[test]
fn command_check_dump() {
    let out = tonic(&["-c", "print(42)"]);
    assert_eq!(out.stdout, b"42\n");
    assert!(tonic(&["--check", "-c", "print(1//0)"]).status.success());
    let out = tonic(&["--dump-bytecode", "-c", "print(42)"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("Call"));
}
#[test]
fn error_exit_codes() {
    assert_eq!(tonic(&["--unknown"]).status.code(), Some(2));
    let out = tonic(&["-c", "print(1//0)"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("<command>:1:"));
    assert_eq!(
        tonic(&["--fuel", "10", "-c", "while True: pass"])
            .status
            .code(),
        Some(1)
    );
}
#[test]
fn stdin() {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new(env!("CARGO_BIN_EXE_tonic"))
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"print(6*7)\n")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    assert_eq!(out.stdout, b"42\n");
}
#[test]
fn collection_options() {
    for interval in ["0", "-1", "garbage"] {
        assert_eq!(
            tonic(&["--gc-every", interval, "-c", "pass"]).status.code(),
            Some(2)
        );
    }
    let source = "i=0\nx=0.0\nwhile i<100:\n    x+=0.5\n    i+=1\nprint(x)";
    let stress = tonic(&["--gc-every", "1", "--stats", "-c", source]);
    let disabled = tonic(&["--no-gc", "--stats", "-c", source]);
    assert!(stress.status.success() && disabled.status.success());
    assert_eq!(stress.stdout, b"50.0\n");
    assert_eq!(stress.stdout, disabled.stdout);
    assert!(!String::from_utf8_lossy(&stress.stderr).contains(", gc_collections: 0,"));
    assert!(String::from_utf8_lossy(&disabled.stderr).contains(", gc_collections: 0,"));
}
#[test]
fn jit_executes_leaf_loops_and_deoptimizes_to_the_interpreter() {
    let loop_source = "def sum_to(n):\n    total=0\n    i=0\n    while i<n:\n        total+=i\n        i+=1\n    return total\nprint(sum_to(100))";
    let native = tonic(&["--jit", "--stats", "-c", loop_source]);
    assert!(native.status.success());
    assert_eq!(native.stdout, b"4950\n");
    let stats = String::from_utf8_lossy(&native.stderr);
    assert!(stats.contains("jit_compiled: 1"));
    assert!(stats.contains("jit_calls: 1"));
    assert!(stats.contains("jit_returns: 1"));

    let deopt = tonic(&[
        "--jit",
        "--stats",
        "-c",
        "def add(a,b):\n    c=a+b\n    c=c+0\n    c=c+0\n    return c\ni=0\nwhile i<15:\n    x=add(1.5,2)\n    i+=1\nprint(x)",
    ]);
    assert!(deopt.status.success());
    assert_eq!(deopt.stdout, b"3.5\n");
    assert!(String::from_utf8_lossy(&deopt.stderr).contains("jit_deopts: 8"));
    assert!(String::from_utf8_lossy(&deopt.stderr).contains("jit_despecialized: 1"));

    let fallback = tonic(&[
        "--jit",
        "--stats",
        "-c",
        "def divide(a,b):\n    c=a+0\n    c=c+0\n    c=c+0\n    return c/b\ni=0\nwhile i<8:\n    x=divide(9,3)\n    i+=1\nprint(x)",
    ]);
    assert!(fallback.status.success());
    assert_eq!(fallback.stdout, b"3.0\n");
    let stats = String::from_utf8_lossy(&fallback.stderr);
    assert!(stats.contains("jit_compiled: 1"));
    assert!(stats.contains("jit_helper_calls: 1"));
    assert!(stats.contains("jit_fallbacks: 0"));

    let fuel = tonic(&["--jit", "--fuel", "1000000", "--stats", "-c", loop_source]);
    assert!(fuel.status.success());
    assert_eq!(fuel.stdout, b"4950\n");
    assert!(String::from_utf8_lossy(&fuel.stderr).contains("jit_calls: 0"));
}
