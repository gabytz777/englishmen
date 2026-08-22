use crate::ast::Stmt;
use crate::bytecode::{self, Chunk};
use crate::compiler;
use crate::config;
use crate::error::EmError;
use crate::installer;
use crate::interpreter::Interpreter;
use crate::lexer::tokenize;
use crate::parser::parse;
use crate::repl;
use crate::vm::Vm;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn run() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() {
        if !installer::is_installed() {
            return cmd_install();
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
        "repair" => cmd_repair(),
        "install" => cmd_install(),
        "build" => cmd_build(&args[1..]),
        "run" => match args.get(1) {
            Some(file) => cmd_run_path(file, &args[2..]),
            None => {
                eprintln!("Error: 'em run' needs a file to run\n");
                print_help();
                ExitCode::FAILURE
            }
        },
        other if other.ends_with(".em") || other.ends_with(".emb") => {
            cmd_run_path(other, &args[1..])
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
    em <file.em>              Compile and run a program
    em <file.emb>             Run compiled Englishmen bytecode
    em run <file>             Same as the two above
    em build [file.em]        Compile to bytecode (defaults to the project main)
    em new [name]             Create a new project
    em repair                 Check and repair the installation
    em                        Start the REPL
    em --help
    em --version

Run options:
    --interpret               Use the tree-walking interpreter instead of the
                              compiler and VM

Build options:
    -o, --out <file.emb>      Where to write the compiled program
    --emit asm                Print the disassembly instead of writing a file
    --run                     Run the program after compiling it
"#
    );
}

/// Run a `.em` source file or a pre-compiled `.emb` bytecode file.
fn cmd_run_path(path: &str, rest: &[String]) -> ExitCode {
    let interpret = rest.iter().any(|a| a == "--interpret");
    if let Some(bad) = rest
        .iter()
        .find(|a| a.starts_with('-') && *a != "--interpret")
    {
        eprintln!("Error: Unknown option '{bad}'\n");
        print_help();
        return ExitCode::FAILURE;
    }

    let path = Path::new(path);
    if path.extension().and_then(|e| e.to_str()) == Some("emb") {
        if interpret {
            eprintln!("Error: '--interpret' only applies to '.em' source files");
            return ExitCode::FAILURE;
        }
        return cmd_run_bytecode(path);
    }
    cmd_run_file(path, interpret)
}

/// Compile the file and run it on the VM, or walk the AST directly when the
/// interpreter backend was asked for.
fn cmd_run_file(path: &Path, interpret: bool) -> ExitCode {
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
    ExitCode::SUCCESS
}

fn cmd_run_bytecode(path: &Path) -> ExitCode {
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
    ExitCode::SUCCESS
}

/// `em build [file.em] [-o out.emb] [--emit asm] [--run]`
fn cmd_build(args: &[String]) -> ExitCode {
    let mut file: Option<String> = None;
    let mut out: Option<PathBuf> = None;
    let mut emit_asm = false;
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
                Some("bytecode") => i += 2,
                Some(other) => {
                    eprintln!(
                        "Error: Unknown emit format '{other}' (expected 'asm' or 'bytecode')"
                    );
                    return ExitCode::FAILURE;
                }
                None => {
                    eprintln!("Error: '--emit' needs a format ('asm' or 'bytecode')");
                    return ExitCode::FAILURE;
                }
            },
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

    let source_path = match file {
        Some(f) => PathBuf::from(f),
        None => match project_main() {
            Some(p) => p,
            None => {
                eprintln!("Error: No source file given and no project found here.");
                eprintln!();
                eprintln!("Run 'em build <file.em>', or run 'em build' in a project directory");
                eprintln!("containing an 'englishmen.emc' with a 'main' entry.");
                return ExitCode::FAILURE;
            }
        },
    };

    let (source, filename) = match read_source(&source_path) {
        Some(pair) => pair,
        None => return ExitCode::FAILURE,
    };
    let program = match front_end(&source, &filename) {
        Ok(p) => p,
        Err(()) => return ExitCode::FAILURE,
    };
    let chunk = match compiler::compile(&program) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}", e.render(&filename, &source));
            return ExitCode::FAILURE;
        }
    };

    if emit_asm {
        print!("{}", bytecode::disassemble(&chunk, &filename));
        return ExitCode::SUCCESS;
    }

    let out_path = out.unwrap_or_else(|| source_path.with_extension("emb"));
    let bytes = bytecode::serialize(&chunk);
    if let Err(e) = std::fs::write(&out_path, &bytes) {
        eprintln!("Error: Could not write '{}': {e}", out_path.display());
        return ExitCode::FAILURE;
    }

    println!(
        "Compiled {} -> {} ({} instructions, {} bytes)",
        source_path.display(),
        out_path.display(),
        count_instructions(&chunk),
        bytes.len()
    );

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

/// The `main` entry of an `englishmen.emc` in the current directory, if any.
fn project_main() -> Option<PathBuf> {
    let dir = std::env::current_dir().ok()?;
    let config = config::EmConfig::load(&dir.join("englishmen.emc")).ok()?;
    let path = dir.join(config.main_file().unwrap_or("main.em"));
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

fn cmd_repair() -> ExitCode {
    match installer::run_repair(true) {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: Repair failed: {e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_install() -> ExitCode {
    let create_projects = confirm_projects_dir();
    match installer::run_installer(create_projects) {
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
    let main_path = project_dir.join("main.em");

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
