use std::{
    collections::{HashSet, VecDeque},
    env, fs,
    io::{self, Read},
    path::{Path, PathBuf},
    process::ExitCode,
};
use tonic_core::diagnostic::Diagnostic;
use tonic_runtime::{ExecutionMode, Vm};
const HELP:&str="Tonic 0.1.0 — bootstrap Python-syntax runtime\n\nUsage: tonic [--check | --dump-bytecode] [--stats] [--jit] [--fuel N] FILE\n       tonic [options] -c SOURCE\n       tonic [options] -\n\n  --check          Parse, compile and verify without executing\n  --dump-bytecode  Show verified register bytecode without executing\n  --stats          Write interpreter and JIT counters to stderr\n  --jit            Promote supported hot functions with Cranelift\n  --fuel N         Stop after N VM instructions (disables JIT execution)\n  --gc-every N     Collect after N allocations (1 for stress testing)\n  --no-gc          Disable automatic collection\n  -                Read UTF-8 source from stdin\n  -h, --help       Show help\n  -V, --version    Show version\n\nUnsupported functions use the generic interpreter. CPython bridge and interactive REPL are not implemented yet.\n";
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err((code, text)) => {
            eprintln!("{text}");
            ExitCode::from(code)
        }
    }
}
fn run() -> std::result::Result<(), (u8, String)> {
    let mut args = env::args().skip(1);
    let mut input = None;
    let mut check = false;
    let mut dump = false;
    let mut stats = false;
    let mut jit = false;
    let mut fuel = None;
    let mut gc_interval = Some(1024);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                print!("{HELP}");
                return Ok(());
            }
            "--version" | "-V" => {
                println!("tonic {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--check" => check = true,
            "--dump-bytecode" => dump = true,
            "--stats" => stats = true,
            "--jit" => jit = true,
            "--no-gc" => gc_interval = None,
            "--gc-every" => {
                let n = args
                    .next()
                    .ok_or_else(|| (2, "--gc-every needs a positive integer".into()))?;
                let interval = n
                    .parse::<u64>()
                    .map_err(|_| (2, "invalid --gc-every value".into()))?;
                if interval == 0 {
                    return Err((2, "--gc-every must be positive".into()));
                }
                gc_interval = Some(interval);
            }
            "--fuel" => {
                let n = args
                    .next()
                    .ok_or_else(|| (2, "--fuel needs an integer".into()))?;
                fuel = Some(
                    n.parse::<u64>()
                        .map_err(|_| (2, "invalid --fuel value".into()))?,
                );
            }
            "-c" => {
                if input.is_some() {
                    return Err((2, "only one source input is allowed".into()));
                }
                let source = args
                    .next()
                    .ok_or_else(|| (2, "-c needs source text".into()))?;
                input = Some(("<command>".to_owned(), Some(source)));
            }
            _ => {
                if arg.starts_with('-') && arg != "-" {
                    return Err((2, format!("unknown option: {arg}")));
                }
                if input.is_some() {
                    return Err((2, "only one source input is allowed".into()));
                }
                input = Some((arg, None));
            }
        }
    }
    let Some((filename, source)) = input else {
        return Err((2, HELP.into()));
    };
    let source = if let Some(s) = source {
        s
    } else if filename == "-" {
        let mut s = String::new();
        io::stdin()
            .read_to_string(&mut s)
            .map_err(|e| (2, e.to_string()))?;
        s
    } else {
        fs::read_to_string(&filename).map_err(|e| (2, format!("{filename}: {e}")))?
    };
    let render = |e: Diagnostic| (1, render_diagnostic(&e, &filename, &source));
    let program = if filename != "-" && filename != "<command>" {
        compile_file_graph(&filename, source.clone()).map_err(render)?
    } else {
        tonic_compiler::compile(&source, &filename).map_err(render)?
    };
    if dump {
        print!("{}", program.program().disassemble());
        return Ok(());
    }
    if check {
        return Ok(());
    }
    let mut vm = Vm::new().map_err(render)?;
    vm.limits.instructions = fuel;
    vm.gc_interval = gc_interval;
    vm.execution_mode = if jit {
        ExecutionMode::Jit
    } else {
        ExecutionMode::Interpreter
    };
    vm.run(&program, &mut io::stdout().lock()).map_err(render)?;
    if stats {
        eprintln!("{:?}", vm.stats);
    }
    Ok(())
}

fn render_diagnostic(error: &Diagnostic, fallback_filename: &str, fallback_source: &str) -> String {
    let filename = error.filename.as_deref().unwrap_or(fallback_filename);
    if filename == fallback_filename {
        return error.render(filename, fallback_source);
    }
    match fs::read_to_string(filename) {
        Ok(source) => error.render(filename, &source),
        Err(_) => error.render(filename, ""),
    }
}

fn compile_file_graph(
    filename: &str,
    source: String,
) -> tonic_core::diagnostic::Result<tonic_core::bytecode::VerifiedProgram> {
    let entry = Path::new(filename);
    let root = entry.parent().unwrap_or_else(|| Path::new("."));
    let mut sources = vec![tonic_compiler::ModuleSource::new(
        "__main__", filename, source,
    )];
    let mut queued = HashSet::new();
    let mut queue = VecDeque::new();
    for import in tonic_compiler::discover_imports(&sources[0].source, filename)? {
        if queued.insert(import.clone()) {
            queue.push_back(import);
        }
    }
    while let Some(name) = queue.pop_front() {
        let Some(path) = resolve_module(root, &name) else {
            continue;
        };
        let module_source = fs::read_to_string(&path).map_err(|error| {
            Diagnostic::new("ImportError", format!("{}: {error}", path.display()))
        })?;
        let module_filename = path.to_string_lossy().into_owned();
        for import in tonic_compiler::discover_imports(&module_source, &module_filename)? {
            if queued.insert(import.clone()) {
                queue.push_back(import);
            }
        }
        sources.push(tonic_compiler::ModuleSource::new(
            name,
            module_filename,
            module_source,
        ));
    }
    tonic_compiler::compile_modules("__main__", &sources)
}

fn resolve_module(root: &Path, name: &str) -> Option<PathBuf> {
    let relative = name.replace('.', "/");
    [
        root.join(format!("{relative}.tonic")),
        root.join(format!("{relative}.py")),
        root.join(&relative).join("__init__.tonic"),
        root.join(relative).join("__init__.py"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}
