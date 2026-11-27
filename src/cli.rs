use crate::appimage;
use crate::ast::Stmt;
use crate::bundle;
use crate::bytecode::{self, Chunk};
use crate::compiler;
use crate::config;
use crate::console;
use crate::error::{EmError, Span};
use crate::installer;
use crate::interpreter::Interpreter;
use crate::lexer::{tokenize, InterpPart, SpannedToken, Token};
use crate::parser::parse;
use crate::repl;
use crate::updater;
use crate::vm::Vm;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn run() -> ExitCode {
    // A standalone build carries its program inside this executable. When one
    // is present it *is* the program, so it runs ahead of any command.
    if let Some(program) = bundle::embedded_program() {
        return finish(run_embedded(program));
    }

    let args: Vec<String> = std::env::args().skip(1).collect();

    // The automatic update check belongs to everyday use (REPL, running and
    // building programs). Update-related commands check by themselves, and
    // help output stays clean.
    let auto_check_due = !args
        .first()
        .is_some_and(|arg| matches!(arg.as_str(), "update" | "install" | "uninstall" | "repair" | "--help" | "-h" | "--version" | "-v"));
    if auto_check_due {
        updater::auto_check();
    }

    if args.is_empty() {
        if !installer::is_installed() {
            return cmd_install(&[]);
        }
        repl::run_repl();
        return ExitCode::SUCCESS;
    }

    match args[0].as_str() {
        "--help" | "-h" => {
            print_help();
            ExitCode::SUCCESS
        }
        "--version" | "-v" => {
            println!("Englishmen {VERSION}");
            ExitCode::SUCCESS
        }
        "new" => cmd_new(args.get(1).map(|s| s.as_str())),
        "uninstall" => cmd_uninstall(&args[1..]),
        "update" => cmd_update(&args[1..]),
        "repair" => cmd_repair(),
        "install" => cmd_install(&args[1..]),
        "build" => cmd_build(&args[1..]),
        "check" => cmd_check(&args[1..]),
        "fmt" => cmd_fmt(&args[1..]),
        "eval" => cmd_eval(&args[1..]),
        "doc" => cmd_doc(&args[1..]),
        "run" => match args.get(1) {
            Some(file) => finish(cmd_run_path(file, &args[2..])),
            None => {
                eprintln!("Error: 'em run' needs a file to run\n");
                print_help();
                ExitCode::FAILURE
            }
        },
        other if other.ends_with(".em") => legacy_extension_notice(Path::new(other)),
        other if other.ends_with(".enm") || other.ends_with(".emb") => {
            finish(cmd_run_path(other, &args[1..]))
        }
        other => {
            eprintln!("Error: Unknown command or file '{other}'\n");
            print_help();
            ExitCode::FAILURE
        }
    }
}

fn print_help() {
    println!(
        r#"Englishmen

Usage:
    em <file.enm>             Compile and run a program
    em <file.emb>             Run compiled Englishmen bytecode
    em run <file>             Same as the two above
    em build [file.enm]       Compile to bytecode (defaults to the project main)
    em check <file.enm>       Check syntax and compilation; write nothing
    em fmt <file.enm>         Format source (--check verifies without writing)
    em eval "<source>"        Compile and run a one-line program
    em doc                    Print the language quick reference
    em new [name]             Create a new project
    em install [options]      Install or upgrade Englishmen
    em uninstall [options]    Remove Englishmen from this machine
    em update [options]       Check for and install Englishmen updates
    em repair                 Check and repair the installation
    em                        Start the REPL
    em --help
    em --version

Install options:
    -y, --yes                 Accept defaults without prompting
    --no-projects             Do not create a projects folder
    --optional                Install optional packages (appimagetool, ~15 MB)
    --no-optional             Skip optional packages

Uninstall options:
    -y, --yes                 Remove without asking for confirmation
    --purge                   Also delete the projects folder
                              (projects are kept by default)

Update options:
    --check                   Check only; never downloads or installs
    --notifications on|off    Enable or disable automatic update notices

Check options:
    --help                    Show check-specific help

Format options:
    --check                   Report whether formatting would change the file;
                              exit nonzero when it would
    --help                    Show format-specific help

Eval options:
    --help                    Show eval-specific help

Run options:
    --interpret               Use the tree-walking interpreter instead of the
                              compiler and VM
    --time                    Print elapsed wall-clock time to stderr

Build options:
    -o, --out <file>          Where to write the compiled program
    --exe                     Build a standalone executable that runs without
                              Englishmen installed
    --appimage                Package the build as a portable Linux AppImage
                              (implies --exe; requires appimagetool)
    --emit asm                Print the disassembly instead of writing a file
    --emit ast                Print the parsed AST instead of writing a file
    --stats                   Print code statistics instead of writing a file
    --run                     Run the program after compiling it
"#
    );
}

/// Finish a program run, holding the window open if it was double-clicked.
/// Output and errors are already printed by this point, so the pause happens
/// whether the program succeeded or failed — a failure is the case where
/// reading the message matters most.
fn finish(code: ExitCode) -> ExitCode {
    console::pause_if_window_will_close();
    code
}

/// `.em` was the old source extension. Point people at the new one instead of
/// silently running a file that the current tooling no longer recognises.
fn legacy_extension_notice(path: &Path) -> ExitCode {
    eprintln!("Warning: Englishmen has switched from '.em' to '.enm' source files.");
    eprintln!(
        "         Rename '{}' to '{}' and try again.",
        path.display(),
        path.with_extension("enm").display()
    );
    ExitCode::FAILURE
}

/// Run the program bundled into this executable by `em build --exe`.
fn run_embedded(program: Result<Chunk, EmError>) -> ExitCode {
    let name = std::env::current_exe()
        .ok()
        .as_deref()
        .map(display_name)
        .unwrap_or_else(|| "program".to_string());

    let chunk = match program {
        Ok(chunk) => chunk,
        Err(e) => {
            eprintln!("{}", e.render(&name, ""));
            return ExitCode::FAILURE;
        }
    };

    let mut vm = Vm::new();
    if let Err(e) = vm.run(chunk) {
        eprintln!("{}", e.render(&name, ""));
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// Run an `.enm` source file or a pre-compiled `.emb` bytecode file.
fn cmd_run_path(path: &str, rest: &[String]) -> ExitCode {
    let interpret = rest.iter().any(|a| a == "--interpret");
    let time = rest.iter().any(|a| a == "--time");
    if let Some(bad) = rest
        .iter()
        .find(|a| a.starts_with('-') && *a != "--interpret" && *a != "--time")
    {
        eprintln!("Error: Unknown option '{bad}'\n");
        print_help();
        return ExitCode::FAILURE;
    }

    let path = Path::new(path);
    if path.extension().and_then(|e| e.to_str()) == Some("em") {
        return legacy_extension_notice(path);
    }
    if path.extension().and_then(|e| e.to_str()) == Some("emb") {
        if interpret {
            eprintln!("Error: '--interpret' only applies to '.enm' source files");
            return ExitCode::FAILURE;
        }
        return cmd_run_bytecode(path, time);
    }
    cmd_run_file(path, interpret, time)
}

/// Print wall-clock timing only after a run has succeeded.
fn report_elapsed(start: Instant) {
    eprintln!(
        "Elapsed time: {:.3} ms (wall clock)",
        start.elapsed().as_secs_f64() * 1000.0
    );
}

/// Compile the file and run it on the VM, or walk the AST directly when the
/// interpreter backend was asked for.
fn cmd_run_file(path: &Path, interpret: bool, time: bool) -> ExitCode {
    let started = Instant::now();
    let (source, filename) = match read_source(path) {
        Some(pair) => pair,
        None => return ExitCode::FAILURE,
    };

    let program = match front_end(&source, &filename) {
        Ok(p) => p,
        Err(()) => return ExitCode::FAILURE,
    };

    if interpret {
        let mut interpreter = Interpreter::new();
        if let Err(e) = interpreter.run(&program) {
            eprintln!("{}", e.render(&filename, &source));
            return ExitCode::FAILURE;
        }
        if time {
            report_elapsed(started);
        }
        return ExitCode::SUCCESS;
    }

    let chunk = match compiler::compile(&program) {
        Ok(chunk) => chunk,
        Err(e) => {
            eprintln!("{}", e.render(&filename, &source));
            return ExitCode::FAILURE;
        }
    };

    let mut vm = Vm::new();
    if let Err(e) = vm.run(chunk) {
        eprintln!("{}", e.render(&filename, &source));
        return ExitCode::FAILURE;
    }
    if time {
        report_elapsed(started);
    }
    ExitCode::SUCCESS
}

fn cmd_run_bytecode(path: &Path, time: bool) -> ExitCode {
    let started = Instant::now();
    let filename = display_name(path);
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Error: Could not read file '{}': {e}", path.display());
            return ExitCode::FAILURE;
        }
    };
    let chunk = match bytecode::deserialize(&bytes) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}", e.render(&filename, ""));
            return ExitCode::FAILURE;
        }
    };
    let mut vm = Vm::new();
    if let Err(e) = vm.run(chunk) {
        // The source is not shipped with the bytecode, so errors carry the
        // recorded line and column but no source excerpt.
        eprintln!("{}", e.render(&filename, ""));
        return ExitCode::FAILURE;
    }
    if time {
        report_elapsed(started);
    }
    ExitCode::SUCCESS
}

/// `em build [file.enm] [-o out] [--emit asm|ast] [--stats] [--exe] [--appimage] [--run]`
fn cmd_build(args: &[String]) -> ExitCode {
    let mut file: Option<String> = None;
    let mut out: Option<PathBuf> = None;
    let mut emit_asm = false;
    let mut emit_ast = false;
    let mut stats = false;
    let mut standalone = false;
    let mut appimage = false;
    let mut run_after = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--out" => match args.get(i + 1) {
                Some(value) => {
                    out = Some(PathBuf::from(value));
                    i += 2;
                }
                None => {
                    eprintln!("Error: '{}' needs a file name", args[i]);
                    return ExitCode::FAILURE;
                }
            },
            "--emit" => match args.get(i + 1).map(|s| s.as_str()) {
                Some("asm") => {
                    emit_asm = true;
                    i += 2;
                }
                Some("ast") => {
                    emit_ast = true;
                    i += 2;
                }
                Some("bytecode") => i += 2,
                Some(other) => {
                    eprintln!(
                        "Error: Unknown emit format '{other}' (expected 'asm', 'ast' or 'bytecode')"
                    );
                    return ExitCode::FAILURE;
                }
                None => {
                    eprintln!("Error: '--emit' needs a format ('asm', 'ast' or 'bytecode')");
                    return ExitCode::FAILURE;
                }
            },
            "--stats" => {
                stats = true;
                i += 1;
            }
            "--exe" | "--standalone" => {
                standalone = true;
                i += 1;
            }
            "--appimage" => {
                appimage = true;
                i += 1;
            }
            "--run" => {
                run_after = true;
                i += 1;
            }
            other if other.starts_with('-') => {
                eprintln!("Error: Unknown build option '{other}'\n");
                print_help();
                return ExitCode::FAILURE;
            }
            other => {
                if file.is_some() {
                    eprintln!("Error: 'em build' takes at most one source file");
                    return ExitCode::FAILURE;
                }
                file = Some(other.to_string());
                i += 1;
            }
        }
    }

    if appimage {
        if !cfg!(target_os = "linux") {
            eprintln!("Error: '--appimage' builds Linux AppImages and is only available on Linux");
            return ExitCode::FAILURE;
        }
        if emit_asm || emit_ast {
            eprintln!(
                "Error: '--appimage' cannot be combined with '--emit asm' or '--emit ast'"
            );
            return ExitCode::FAILURE;
        }
        // An AppImage wraps the standalone binary, so ask for one.
        standalone = true;
    }

    let source_path = match file {
        Some(f) => PathBuf::from(f),
        None => match project_main() {
            Some(p) => p,
            None => {
                eprintln!("Error: No source file given and no project found here.");
                eprintln!();
                eprintln!("Run 'em build <file.enm>', or run 'em build' in a project directory");
                eprintln!("containing an 'englishmen.emc' with a 'main' entry.");
                return ExitCode::FAILURE;
            }
        },
    };

    if source_path.extension().and_then(|e| e.to_str()) == Some("em") {
        return legacy_extension_notice(&source_path);
    }

    let (source, filename) = match read_source(&source_path) {
        Some(pair) => pair,
        None => return ExitCode::FAILURE,
    };
    let program = match front_end(&source, &filename) {
        Ok(p) => p,
        Err(()) => return ExitCode::FAILURE,
    };
    if emit_ast {
        // Debug formatting is intentionally the whole output: it is the
        // unmodified AST, and nothing at all is written to disk.
        println!("{program:#?}");
        return ExitCode::SUCCESS;
    }
    let chunk = match compiler::compile(&program) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}", e.render(&filename, &source));
            return ExitCode::FAILURE;
        }
    };

    if stats {
        println!("Statistics for {filename}");
        println!("  constants: {}", count_constants(&chunk));
        println!("  instructions: {}", count_instructions(&chunk));
        println!("  functions: {}", count_functions(&chunk));
        return ExitCode::SUCCESS;
    }

    if emit_asm {
        print!("{}", bytecode::disassemble(&chunk, &filename));
        return ExitCode::SUCCESS;
    }

    let instructions = count_instructions(&chunk);
    let out_path = out.unwrap_or_else(|| {
        if appimage {
            appimage::default_output_path(&source_path)
        } else if standalone {
            bundle::default_output_path(&source_path)
        } else {
            source_path.with_extension("emb")
        }
    });

    if appimage {
        if out_path == source_path {
            eprintln!(
                "Error: The AppImage build would overwrite '{}'",
                source_path.display()
            );
            eprintln!("Pass '-o <name>.AppImage' to choose a different output file.");
            return ExitCode::FAILURE;
        }
        let stem = source_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "program".to_string());
        let size = match appimage::create(&chunk, &stem, &out_path) {
            Ok(size) => size,
            Err(e) => {
                eprintln!("{}", e.render(&filename, &source));
                return ExitCode::FAILURE;
            }
        };
        println!(
            "Compiled {} -> {} ({instructions} instructions, {size} bytes, portable Linux AppImage)",
            source_path.display(),
            out_path.display(),
        );
    } else if standalone {
        // Refuse to write over the source: `em build --exe main` with a
        // missing extension would otherwise destroy the program.
        if out_path == source_path {
            eprintln!(
                "Error: The standalone build would overwrite '{}'",
                source_path.display()
            );
            eprintln!("Pass '-o <name>' to choose a different output file.");
            return ExitCode::FAILURE;
        }
        let size = match bundle::create(&chunk, &out_path) {
            Ok(size) => size,
            Err(e) => {
                eprintln!("{}", e.render(&filename, &source));
                return ExitCode::FAILURE;
            }
        };
        println!(
            "Compiled {} -> {} ({instructions} instructions, {size} bytes, runs on its own)",
            source_path.display(),
            out_path.display(),
        );
    } else {
        let bytes = bytecode::serialize(&chunk);
        if let Err(e) = std::fs::write(&out_path, &bytes) {
            eprintln!("Error: Could not write '{}': {e}", out_path.display());
            return ExitCode::FAILURE;
        }
        println!(
            "Compiled {} -> {} ({instructions} instructions, {} bytes)",
            source_path.display(),
            out_path.display(),
            bytes.len()
        );
    }

    if run_after {
        let mut vm = Vm::new();
        if let Err(e) = vm.run(chunk) {
            eprintln!("{}", e.render(&filename, &source));
            return ExitCode::FAILURE;
        }
    }

    ExitCode::SUCCESS
}

/// Instructions in a chunk and in every function nested inside it.
fn count_instructions(chunk: &Chunk) -> usize {
    chunk.code.len()
        + chunk
            .functions
            .iter()
            .map(|f| count_instructions(&f.chunk))
            .sum::<usize>()
}

/// Constants in a chunk and in every nested function, including names.
fn count_constants(chunk: &Chunk) -> usize {
    chunk.constants.len()
        + chunk
            .functions
            .iter()
            .map(|f| count_constants(&f.chunk))
            .sum::<usize>()
}

/// Function prototypes, counting nested functions too.
fn count_functions(chunk: &Chunk) -> usize {
    chunk.functions.len()
        + chunk
            .functions
            .iter()
            .map(|f| count_functions(&f.chunk))
            .sum::<usize>()
}

/// The `main` entry of an `englishmen.emc` in the current directory, if any.
fn project_main() -> Option<PathBuf> {
    let dir = std::env::current_dir().ok()?;
    let config = config::EmConfig::load(&dir.join("englishmen.emc")).ok()?;
    let path = dir.join(config.main_file().unwrap_or("main.enm"));
    path.exists().then_some(path)
}

fn read_source(path: &Path) -> Option<(String, String)> {
    match std::fs::read_to_string(path) {
        Ok(source) => Some((source, display_name(path))),
        Err(e) => {
            eprintln!("Error: Could not read file '{}': {e}", path.display());
            None
        }
    }
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

/// Lex and parse, reporting any error against the source.
fn front_end(source: &str, filename: &str) -> Result<Vec<Stmt>, ()> {
    let report = |e: EmError| eprintln!("{}", e.render(filename, source));
    let tokens = tokenize(source).map_err(report)?;
    parse(tokens).map_err(report)
}

// ---------------------------------------------------------------------------
// em check / em fmt / em eval / em doc
// ---------------------------------------------------------------------------

fn print_check_help() {
    println!(
        r#"Englishmen syntax checker

    em check <file.enm>       Lex, parse and compile the file without writing
    em check --help           Show this help

'em check' reports syntax and compilation errors but never creates or changes
any files. On success it prints a short summary of the checked program.
"#
    );
}

/// `em check <file.enm>` — validate a source file without producing output.
fn cmd_check(args: &[String]) -> ExitCode {
    if args.iter().any(|a| matches!(a.as_str(), "--help" | "-h")) {
        print_check_help();
        return ExitCode::SUCCESS;
    }

    let mut file: Option<&str> = None;
    for arg in args {
        if arg.starts_with('-') {
            eprintln!("Error: Unknown check option '{arg}'\n");
            print_check_help();
            return ExitCode::FAILURE;
        }
        if file.is_some() {
            eprintln!("Error: 'em check' takes at most one source file");
            return ExitCode::FAILURE;
        }
        file = Some(arg);
    }
    let Some(file) = file else {
        eprintln!("Error: 'em check' needs a file to check\n");
        print_check_help();
        return ExitCode::FAILURE;
    };

    let path = Path::new(file);
    if path.extension().and_then(|e| e.to_str()) == Some("em") {
        return legacy_extension_notice(path);
    }
    let (source, filename) = match read_source(path) {
        Some(pair) => pair,
        None => return ExitCode::FAILURE,
    };
    let program = match front_end(&source, &filename) {
        Ok(p) => p,
        Err(()) => return ExitCode::FAILURE,
    };
    let chunk = match compiler::compile(&program) {
        Ok(chunk) => chunk,
        Err(e) => {
            eprintln!("{}", e.render(&filename, &source));
            return ExitCode::FAILURE;
        }
    };
    println!(
        "Checked {filename}: OK ({} statements, {} instructions)",
        program.len(),
        count_instructions(&chunk)
    );
    ExitCode::SUCCESS
}

fn print_fmt_help() {
    println!(
        r#"Englishmen source formatter

    em fmt <file.enm>         Format the file in place
    em fmt <file.enm> --check Report whether formatting would change the file
    em fmt --help             Show this help

Formatting is deterministic: blocks are indented with four spaces, operators
are spaced consistently, and comments are preserved. With --check nothing is
written; the command exits nonzero when the file is not formatted.
"#
    );
}

/// `em fmt <file.enm> [--check]` — deterministic source formatting.
fn cmd_fmt(args: &[String]) -> ExitCode {
    if args.iter().any(|a| matches!(a.as_str(), "--help" | "-h")) {
        print_fmt_help();
        return ExitCode::SUCCESS;
    }

    let mut check_only = false;
    let mut file: Option<&str> = None;
    for arg in args {
        match arg.as_str() {
            "--check" => check_only = true,
            other if other.starts_with('-') => {
                eprintln!("Error: Unknown format option '{other}'\n");
                print_fmt_help();
                return ExitCode::FAILURE;
            }
            other => {
                if file.is_some() {
                    eprintln!("Error: 'em fmt' takes at most one source file");
                    return ExitCode::FAILURE;
                }
                file = Some(other);
            }
        }
    }
    let Some(file) = file else {
        eprintln!("Error: 'em fmt' needs a file to format\n");
        print_fmt_help();
        return ExitCode::FAILURE;
    };

    let path = Path::new(file);
    if path.extension().and_then(|e| e.to_str()) == Some("em") {
        return legacy_extension_notice(path);
    }
    let (source, filename) = match read_source(path) {
        Some(pair) => pair,
        None => return ExitCode::FAILURE,
    };
    // Never "format" broken source: a half-written program must stay as the
    // user wrote it so the diagnostic points at their text, not our rewrite.
    if front_end(&source, &filename).is_err() {
        return ExitCode::FAILURE;
    }
    let formatted = match format_source(&source) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("{}", e.render(&filename, &source));
            return ExitCode::FAILURE;
        }
    };

    if formatted == source {
        println!("{filename} is already formatted");
        return ExitCode::SUCCESS;
    }
    if check_only {
        println!("{filename} is not formatted");
        return ExitCode::FAILURE;
    }
    if let Err(e) = std::fs::write(path, &formatted) {
        eprintln!("Error: Could not write '{}': {e}", path.display());
        return ExitCode::FAILURE;
    }
    println!("Formatted {filename}");
    ExitCode::SUCCESS
}

fn print_eval_help() {
    println!(
        r#"Englishmen one-liner runner

    em eval "<source>"       Compile and run a single line of source
    em eval --help           Show this help

The source is compiled and executed exactly like a '.enm' file; errors are
reported against the name '<eval>'. Extra arguments are joined with spaces.
"#
    );
}

/// `em eval "<source>"` — compile and run source supplied on the command line.
fn cmd_eval(args: &[String]) -> ExitCode {
    if args.iter().any(|a| matches!(a.as_str(), "--help" | "-h")) {
        print_eval_help();
        return ExitCode::SUCCESS;
    }
    if args.is_empty() {
        eprintln!("Error: 'em eval' needs source to run\n");
        print_eval_help();
        return ExitCode::FAILURE;
    }
    let source = args.join(" ");
    let filename = "<eval>";
    let program = match front_end(&source, filename) {
        Ok(p) => p,
        Err(()) => return ExitCode::FAILURE,
    };
    let chunk = match compiler::compile(&program) {
        Ok(chunk) => chunk,
        Err(e) => {
            eprintln!("{}", e.render(filename, &source));
            return ExitCode::FAILURE;
        }
    };
    let mut vm = Vm::new();
    if let Err(e) = vm.run(chunk) {
        eprintln!("{}", e.render(filename, &source));
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// A built-in language reference, kept in the binary so `em doc` works
/// without an installation, a project, or a network connection.
const LANGUAGE_DOC: &str = r#"Englishmen quick reference

Keywords
    if / else / end          Conditional branches
    function ... end         Function definition
    return [value]           Return from a function
    repeat <count> ... end   Counted loop
    for <name> in <list> ... end
                             Loop over the items of an array
    import <module>          Load a standard module
    and / or / not           Boolean operators
    true / false / null      Literal values

Statement forms
    name = expression                     Assignment creates a variable
    object.property = expression          Dot notation / objects
    array[index] = expression             Indexed assignment
    function add(a, b)                    Function definition
        return a + b
    end
    if age >= 18                          Condition (else if / else supported)
        say("Adult")
    end
    repeat 3                              Counted loop
        say("Hello")
    end
    for name in people                    Iterate an array
        say(name)
    end

Builtins
    say(value, ...)          Print values to stdout, separated by spaces
    value.length()           Length of a string or array
    array.add(value)         Append an item
    array.remove(value)      Remove the first matching item
    "a{n}b"                  String interpolation

Standard modules (import <name> first)
    math    sqrt, abs, floor, ceil, round, pow, min, max, pi, e
    time    now, sleep, year, month, day, hour, minute, second
    qrc     make(text, path) — write a QR code PNG
    cowsay  say(message) — ASCII-art speech

Comments
    $- single-line comment -$
    $- multi-line
       comment -$

Numbers, strings, booleans, arrays and objects are the value types. Operators:
+ - * / %, == != > < >= <=, and/or/not. Run 'em --help' for CLI commands.
"#;

fn cmd_doc(args: &[String]) -> ExitCode {
    for arg in args {
        if matches!(arg.as_str(), "--help" | "-h") {
            println!("    em doc                    Print the language quick reference");
            return ExitCode::SUCCESS;
        }
        eprintln!("Error: Unknown doc option '{arg}'\n");
        return ExitCode::FAILURE;
    }
    println!("{}", LANGUAGE_DOC.trim_end());
    ExitCode::SUCCESS
}

// ---------------------------------------------------------------------------
// Deterministic source formatter used by `em fmt`.
// ---------------------------------------------------------------------------

/// A comment lifted out of the source so the lexer can tokenise the rest.
/// Comments are later put back on their own line (or at the end of a line).
struct FormatComment {
    line: usize,
    col: usize,
    text: String,
}

/// One logical line: physical newlines inside brackets do not split it.
struct FormatLine {
    tokens: Vec<Token>,
    line: usize,
    first_col: usize,
    last_line: usize,
    last_col: usize,
}

/// Replace comments with spaces, preserving every position and newline so
/// lexer spans still refer to the original source. Strings are left alone:
/// `$-` inside a literal is text, not a comment.
fn strip_comments(source: &str) -> (String, Vec<FormatComment>) {
    let chars: Vec<char> = source.chars().collect();
    let mut clean = String::with_capacity(source.len());
    let mut comments = Vec::new();
    let mut i = 0;
    let mut line = 1;
    let mut col = 1;
    let mut in_string = false;

    while i < chars.len() {
        let c = chars[i];
        if in_string {
            clean.push(c);
            if c == '\\' && i + 1 < chars.len() {
                i += 1;
                let escaped = chars[i];
                clean.push(escaped);
                if escaped == '\n' {
                    line += 1;
                    col = 1;
                } else {
                    col += 1;
                }
                i += 1;
                col += 1;
                continue;
            }
            if c == '"' {
                in_string = false;
            }
            if c == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
            i += 1;
            continue;
        }
        if c == '"' {
            in_string = true;
            clean.push(c);
            i += 1;
            col += 1;
            continue;
        }
        if c == '$' && chars.get(i + 1) == Some(&'-') {
            let start_line = line;
            let start_col = col;
            let mut text = String::from("$-");
            clean.push(' ');
            clean.push(' ');
            i += 2;
            col += 2;
            while i < chars.len() {
                let ch = chars[i];
                text.push(ch);
                clean.push(if ch == '\n' { '\n' } else { ' ' });
                if ch == '-' && chars.get(i + 1) == Some(&'$') {
                    text.push('$');
                    clean.push(' ');
                    i += 2;
                    col += 2;
                    break;
                }
                if ch == '\n' {
                    line += 1;
                    col = 1;
                } else {
                    col += 1;
                }
                i += 1;
            }
            comments.push(FormatComment {
                line: start_line,
                col: start_col,
                text,
            });
            continue;
        }
        if c == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
        clean.push(c);
        i += 1;
    }
    (clean, comments)
}

/// Escape a string so the formatter's output lexes back to the same literal.
fn escape_string(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            other => out.push(other),
        }
    }
    out
}

/// Print one interpolation part without introducing statement formatting.
fn render_interpolation(parts: &[InterpPart]) -> String {
    let mut out = String::from("\"");
    for part in parts {
        match part {
            InterpPart::Literal(text) => out.push_str(&escape_string(text)),
            InterpPart::Expr(tokens) => {
                let expr: Vec<Token> = tokens
                    .iter()
                    .filter(|t| !matches!(t.token, Token::Newline | Token::Eof))
                    .map(|t| t.token.clone())
                    .collect();
                out.push('{');
                out.push_str(&render_tokens(&expr));
                out.push('}');
            }
        }
    }
    out.push('"');
    out
}

fn token_text(token: &Token) -> String {
    match token {
        Token::Number(n) => format!("{n}"),
        Token::String(s) => format!("\"{}\"", escape_string(s)),
        Token::Interpolated(parts) => render_interpolation(parts),
        Token::Identifier(name) => name.clone(),
        Token::True => "true".to_string(),
        Token::False => "false".to_string(),
        Token::Null => "null".to_string(),
        Token::If => "if".to_string(),
        Token::Else => "else".to_string(),
        Token::End => "end".to_string(),
        Token::Function => "function".to_string(),
        Token::Return => "return".to_string(),
        Token::Repeat => "repeat".to_string(),
        Token::For => "for".to_string(),
        Token::In => "in".to_string(),
        Token::Import => "import".to_string(),
        Token::And => "and".to_string(),
        Token::Or => "or".to_string(),
        Token::Not => "not".to_string(),
        Token::Plus => "+".to_string(),
        Token::Minus => "-".to_string(),
        Token::Star => "*".to_string(),
        Token::Slash => "/".to_string(),
        Token::Percent => "%".to_string(),
        Token::Assign => "=".to_string(),
        Token::Eq => "==".to_string(),
        Token::NotEq => "!=".to_string(),
        Token::Gt => ">".to_string(),
        Token::Lt => "<".to_string(),
        Token::GtEq => ">=".to_string(),
        Token::LtEq => "<=".to_string(),
        Token::LParen => "(".to_string(),
        Token::RParen => ")".to_string(),
        Token::LBracket => "[".to_string(),
        Token::RBracket => "]".to_string(),
        Token::LBrace => "{".to_string(),
        Token::RBrace => "}".to_string(),
        Token::Dot => ".".to_string(),
        Token::Comma => ",".to_string(),
        Token::Colon => ":".to_string(),
        Token::Newline | Token::Eof => String::new(),
    }
}

/// A `-` is unary when it starts an expression (after an operator, a keyword
/// introducing one, or an opening delimiter), not when it follows a value.
fn prefix_position(previous: Option<&Token>) -> bool {
    match previous {
        None => true,
        Some(
            Token::Plus
            | Token::Minus
            | Token::Star
            | Token::Slash
            | Token::Percent
            | Token::Assign
            | Token::Eq
            | Token::NotEq
            | Token::Gt
            | Token::Lt
            | Token::GtEq
            | Token::LtEq
            | Token::And
            | Token::Or
            | Token::Not
            | Token::If
            | Token::Return
            | Token::Repeat
            | Token::For
            | Token::In
            | Token::LParen
            | Token::LBracket
            | Token::LBrace
            | Token::Comma
            | Token::Colon,
        ) => true,
        Some(_) => false,
    }
}

fn needs_space(tokens: &[Token], i: usize) -> bool {
    if i == 0 {
        return false;
    }
    let token = &tokens[i];
    let prev = &tokens[i - 1];
    let before = tokens.get(i.wrapping_sub(2));

    // Closers and postfix punctuation hug the token before them.
    if matches!(
        token,
        Token::Comma | Token::RParen | Token::RBracket | Token::RBrace | Token::Dot | Token::Colon
    ) {
        return false;
    }
    // An opening delimiter never has a space after it.
    if matches!(prev, Token::LParen | Token::LBracket | Token::LBrace | Token::Dot) {
        return false;
    }
    // A prefix minus binds to its operand: `-1`, not `- 1`.
    if matches!(prev, Token::Minus) && prefix_position(before) {
        return false;
    }
    // Calls and indexing hug the name/value they follow; `if (...)` does not.
    if matches!(token, Token::LParen) {
        return !matches!(
            prev,
            Token::Identifier(_)
                | Token::Number(_)
                | Token::String(_)
                | Token::Interpolated(_)
                | Token::True
                | Token::False
                | Token::Null
                | Token::RParen
                | Token::RBracket
        );
    }
    true
}

/// Render a token sequence with normalised spacing. Newlines are not
/// significant inside an expression, so they are dropped here.
fn render_tokens(tokens: &[Token]) -> String {
    let mut out = String::new();
    for (i, token) in tokens.iter().enumerate() {
        if matches!(token, Token::Newline | Token::Eof) {
            continue;
        }
        if needs_space(tokens, i) {
            out.push(' ');
        }
        out.push_str(&token_text(token));
    }
    out
}

fn push_format_line(
    groups: &mut Vec<FormatLine>,
    current: &mut Vec<Token>,
    first: Option<Span>,
    last: Option<Span>,
) {
    if let (Some(first), Some(last)) = (first, last) {
        groups.push(FormatLine {
            tokens: std::mem::take(current),
            line: first.line,
            first_col: first.col,
            last_line: last.line,
            last_col: last.col,
        });
    }
}

fn comment_before(comment: &FormatComment, line: &FormatLine) -> bool {
    (comment.line, comment.col) < (line.line, line.first_col)
}

fn comment_trailing(comment: &FormatComment, line: &FormatLine) -> bool {
    comment.line == line.last_line && comment.col > line.last_col
}

/// Turn lexer tokens plus the saved comments into canonical source.
fn render_format(tokens: &[SpannedToken], comments: &[FormatComment]) -> String {
    let mut groups: Vec<FormatLine> = Vec::new();
    let mut current: Vec<Token> = Vec::new();
    let mut first: Option<Span> = None;
    let mut last: Option<Span> = None;
    let mut depth = 0usize;

    for spanned in tokens {
        match &spanned.token {
            Token::Eof => break,
            Token::Newline => {
                // Arrays and object literals may span lines; bring them
                // together on one canonical line instead.
                if depth == 0 {
                    push_format_line(&mut groups, &mut current, first, last);
                    first = None;
                    last = None;
                }
                continue;
            }
            Token::LParen | Token::LBracket | Token::LBrace => depth += 1,
            Token::RParen | Token::RBracket | Token::RBrace => {
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
        if first.is_none() {
            first = Some(spanned.span);
        }
        last = Some(spanned.span);
        current.push(spanned.token.clone());
    }
    push_format_line(&mut groups, &mut current, first, last);

    let mut out = String::new();
    let mut comment_index = 0;
    let mut indent = 0usize;
    let mut pending_else = false;

    for line in &groups {
        while let Some(comment) = comments.get(comment_index) {
            if !comment_before(comment, line) {
                break;
            }
            out.push_str(&comment.text);
            out.push('\n');
            comment_index += 1;
        }

        let first_token = &line.tokens[0];
        let ends_block = matches!(first_token, Token::End);
        let is_else_if = matches!(first_token, Token::Else)
            && matches!(line.tokens.get(1), Some(Token::If));
        let plain_else = matches!(first_token, Token::Else) && !is_else_if;
        let continuation_if = pending_else && matches!(first_token, Token::If);
        let mut level = indent;
        if ends_block || plain_else {
            level = level.saturating_sub(1);
        }
        if level > 0 {
            out.push_str(&"    ".repeat(level));
        }
        out.push_str(&render_tokens(&line.tokens));
        out.push('\n');

        while let Some(comment) = comments.get(comment_index) {
            if !comment_trailing(comment, line) {
                break;
            }
            out.push(' ');
            out.push_str(&comment.text);
            out.push('\n');
            comment_index += 1;
        }

        if ends_block {
            indent = indent.saturating_sub(1);
        } else if is_else_if || continuation_if {
            // A single `end` closes the whole chain, so an `else if` line
            // keeps the indent of the `if` that started it.
        } else if plain_else {
            indent = level + 1;
        } else if matches!(
            first_token,
            Token::If | Token::Function | Token::Repeat | Token::For
        ) {
            indent += 1;
        }
        pending_else = plain_else;
    }

    for comment in &comments[comment_index..] {
        out.push_str(&comment.text);
        out.push('\n');
    }
    out
}

/// Lex and render source in the canonical format. The caller parses first so
/// malformed source gets the normal diagnostic instead of a rewrite.
fn format_source(source: &str) -> Result<String, EmError> {
    let (clean, comments) = strip_comments(source);
    let tokens = tokenize(&clean)?;
    Ok(render_format(&tokens, &comments))
}

/// `em uninstall [-y|--yes] [--purge]`
fn cmd_uninstall(args: &[String]) -> ExitCode {
    let mut assume_yes = false;
    let mut purge = false;

    for arg in args {
        match arg.as_str() {
            "-y" | "--yes" => assume_yes = true,
            "--purge" => purge = true,
            "--keep-projects" => purge = false,
            "--help" | "-h" => {
                print_help();
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("Error: Unknown uninstall option '{other}'\n");
                print_help();
                return ExitCode::FAILURE;
            }
        }
    }

    match installer::run_uninstall(assume_yes, purge) {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: Uninstall failed: {e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_repair() -> ExitCode {
    match installer::run_repair(true) {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: Repair failed: {e}");
            ExitCode::FAILURE
        }
    }
}

fn print_update_help() {
    println!(
        r#"Englishmen update commands

    em update                 Check for and install the latest version
    em update --check         Check for updates without installing anything
    em update --notifications on|off
                              Turn the automatic startup notification on or
                              off (default: on)

Notes:
    'em update --check' always contacts GitHub, even when an automatic check
    ran recently or notifications are turned off. It never downloads or
    installs anything. Automatic checks run at most once every 24 hours and
    also never install anything — they only print a notice when a newer
    version exists.
"#
    );
}

/// `em update [--check] [--notifications on|off]`
fn cmd_update(args: &[String]) -> ExitCode {
    let mut check_only = false;
    let mut notifications: Option<bool> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--check" => {
                check_only = true;
                i += 1;
            }
            "--notifications" => match args.get(i + 1).map(|s| s.as_str()) {
                Some("on") => {
                    notifications = Some(true);
                    i += 2;
                }
                Some("off") => {
                    notifications = Some(false);
                    i += 2;
                }
                Some(other) => {
                    eprintln!("Error: Unknown notifications setting '{other}' (expected 'on' or 'off')");
                    return ExitCode::FAILURE;
                }
                None => {
                    eprintln!("Error: '--notifications' needs a value ('on' or 'off')");
                    return ExitCode::FAILURE;
                }
            },
            "--help" | "-h" => {
                print_update_help();
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("Error: Unknown update option '{other}'\n");
                print_update_help();
                return ExitCode::FAILURE;
            }
        }
    }

    if notifications.is_some() && check_only {
        eprintln!("Error: '--notifications' cannot be combined with '--check'");
        return ExitCode::FAILURE;
    }

    if let Some(enabled) = notifications {
        return updater::set_notifications(enabled);
    }
    if check_only {
        return updater::run_manual_check();
    }
    updater::run_install()
}

/// `em install [-y|--yes] [--no-projects] [--optional|--no-optional]`
///
/// The flags exist so install scripts can run the installer without a terminal.
/// Without `--yes`, the projects-folder question is still asked interactively.
fn cmd_install(args: &[String]) -> ExitCode {
    let mut assume_yes = false;
    let mut create_projects: Option<bool> = None;
    let mut optional: Option<bool> = None;

    for arg in args {
        match arg.as_str() {
            "-y" | "--yes" => assume_yes = true,
            "--no-projects" => create_projects = Some(false),
            "--projects" => create_projects = Some(true),
            "--optional" => optional = Some(true),
            "--no-optional" => optional = Some(false),
            "--help" | "-h" => {
                print_help();
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("Error: Unknown install option '{other}'\n");
                print_help();
                return ExitCode::FAILURE;
            }
        }
    }

    let create_projects = create_projects.unwrap_or_else(|| {
        if assume_yes {
            true
        } else {
            confirm_projects_dir()
        }
    });

    match installer::run_installer(create_projects, !assume_yes, optional) {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: Installation failed: {e}");
            ExitCode::FAILURE
        }
    }
}

fn confirm_projects_dir() -> bool {
    use std::io::{self, Write};
    println!("Would you like Englishmen to create a projects folder?");
    println!("Location: {}", installer::default_projects_dir().display());
    print!("[Y/n] ");
    let _ = io::stdout().flush();
    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_err() {
        return true;
    }
    let input = input.trim().to_lowercase();
    input.is_empty() || input == "y" || input == "yes"
}

fn cmd_new(name: Option<&str>) -> ExitCode {
    let project_name = name.unwrap_or("MyProject").to_string();
    let base_dir = resolve_projects_base();
    let project_dir = base_dir.join(&project_name);

    if project_dir.exists() {
        eprintln!(
            "Error: A project already exists at '{}'.\nChoose a different name or remove the existing directory.",
            project_dir.display()
        );
        return ExitCode::FAILURE;
    }

    if let Err(e) = std::fs::create_dir_all(&project_dir) {
        eprintln!("Error: Could not create project directory: {e}");
        return ExitCode::FAILURE;
    }

    let emc_path = project_dir.join("englishmen.emc");
    let main_path = project_dir.join("main.enm");

    let emc_contents = config::default_emc(&project_name);
    let main_contents = "$- Welcome to Englishmen! -$\n\nsay(\"Hello, world!\")\n";

    if let Err(e) = std::fs::write(&emc_path, emc_contents) {
        eprintln!("Error: Could not write '{}': {e}", emc_path.display());
        return ExitCode::FAILURE;
    }
    if let Err(e) = std::fs::write(&main_path, main_contents) {
        eprintln!("Error: Could not write '{}': {e}", main_path.display());
        return ExitCode::FAILURE;
    }

    println!(
        "Created new Englishmen project '{project_name}' at:\n    {}\n",
        project_dir.display()
    );
    println!("Run it with:\n    em {}", main_path.display());

    ExitCode::SUCCESS
}

fn resolve_projects_base() -> PathBuf {
    if installer::is_installed() {
        if let Ok(config) = config::EmConfig::load(&installer::config_file()) {
            if let Some(section) = config.sections.get("installer") {
                if let Some(dir) = section.get("projects_dir") {
                    if !dir.is_empty() {
                        let path = PathBuf::from(dir);
                        if path.exists() {
                            return path;
                        }
                    }
                }
            }
        }
    }

    let default_dir = installer::default_projects_dir();
    if default_dir.exists() {
        return default_dir;
    }

    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatter_normalises_spacing_and_indents_blocks() {
        let source = "if  true\nsay(  1+2  )\nend\n";
        assert_eq!(
            format_source(source).unwrap(),
            "if true\n    say(1 + 2)\nend\n"
        );
    }

    #[test]
    fn formatter_keeps_else_if_at_the_if_indent() {
        let source = "if false\n\tsay(1)\nelse   if true\n    say(2)\nelse\n say(3)\nend\n";
        assert_eq!(
            format_source(source).unwrap(),
            "if false\n    say(1)\nelse if true\n    say(2)\nelse\n    say(3)\nend\n"
        );
    }

    #[test]
    fn formatter_preserves_comments() {
        let source = "$- heading -$\nx=1 $- trailing -$\n";
        assert_eq!(
            format_source(source).unwrap(),
            "$- heading -$\nx = 1 $- trailing -$\n"
        );
    }

    #[test]
    fn formatter_preserves_string_and_interpolation_text() {
        let source = "say(  \"a{b}c\"  )\nsay(\"$- literal -$\")\n";
        let formatted = format_source(source).unwrap();
        assert_eq!(
            formatted,
            "say(\"a{b}c\")\nsay(\"$- literal -$\")\n"
        );
        assert_eq!(format_source(&formatted).unwrap(), formatted);
    }

    #[test]
    fn formatter_collapses_multiline_literals_and_is_idempotent() {
        let source = "xs = [\n  1,\n  2\n]\nperson = {\n name: \"Alex\",\n age: 18\n}\n";
        let expected = "xs = [1, 2]\nperson = {name: \"Alex\", age: 18}\n";
        let formatted = format_source(source).unwrap();
        assert_eq!(formatted, expected);
        assert_eq!(format_source(&formatted).unwrap(), expected);
    }

    #[test]
    fn formatter_normalises_unary_minus_and_calls() {
        let source = "x = -  1\nsay( -x )\nfunction f( a,b )\n return a-b\nend\n";
        assert_eq!(
            format_source(source).unwrap(),
            "x = -1\nsay(-x)\nfunction f(a, b)\n    return a - b\nend\n"
        );
    }
}
