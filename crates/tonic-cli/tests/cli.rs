use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
fn tonic(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_tonic"))
        .args(args)
        .output()
        .unwrap()
}
fn temp_project() -> std::path::PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "tonic-module-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
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

#[test]
fn file_execution_resolves_tonic_and_python_source_modules() {
    let project = temp_project();
    std::fs::write(
        project.join("main.tonic"),
        "import alpha\nimport alpha\nprint(alpha.answer(),alpha.from_beta)",
    )
    .unwrap();
    std::fs::write(
        project.join("alpha.tonic"),
        "print('load-alpha')\nvalue=40\nimport beta\nfrom_beta=beta.seen\ndef answer(): return value+2",
    )
    .unwrap();
    std::fs::write(
        project.join("beta.py"),
        "print('load-beta')\nimport alpha\nseen=alpha.value",
    )
    .unwrap();
    let main = project.join("main.tonic");
    let out = tonic(&[main.to_str().unwrap()]);
    std::fs::remove_dir_all(project).unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, b"load-alpha\nload-beta\n42 40\n");
}

#[test]
fn file_execution_resolves_packages_dotted_and_from_imports() {
    let project = temp_project();
    std::fs::create_dir(project.join("package")).unwrap();
    std::fs::write(
        project.join("main.tonic"),
        "from package import child as first\nfrom package import answer\nimport package.child as leaf\nimport package.child\nprint(answer,first.value,leaf.value,package.child.value)",
    )
    .unwrap();
    std::fs::write(
        project.join("package/__init__.tonic"),
        "print('load-package')\nanswer=40",
    )
    .unwrap();
    std::fs::write(
        project.join("package/child.py"),
        "print('load-child')\nvalue=2",
    )
    .unwrap();
    let main = project.join("main.tonic");
    let out = tonic(&["--jit", main.to_str().unwrap()]);
    std::fs::remove_dir_all(project).unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, b"load-package\nload-child\n40 2 2 2\n");
}

#[test]
fn imported_module_errors_render_the_imported_filename() {
    let project = temp_project();
    let main = project.join("main.tonic");
    let dependency = project.join("dependency.tonic");
    std::fs::write(&main, "import dependency").unwrap();
    std::fs::write(&dependency, "value=1//0").unwrap();
    let out = tonic(&[main.to_str().unwrap()]);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    std::fs::remove_dir_all(project).unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr.contains(&format!("{}:1:", dependency.display())),
        "{stderr}"
    );
}
