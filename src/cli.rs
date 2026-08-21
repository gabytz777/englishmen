use crate::config;
use crate::installer;
use crate::interpreter::Interpreter;
use crate::lexer::tokenize;
use crate::parser::parse;
use crate::repl;
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
        other if other.ends_with(".em") => cmd_run_file(other),
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
    em <file.em>
    em new [name]
    em repair
    em
    em --help
    em --version
"#
    );
}

fn cmd_run_file(path: &str) -> ExitCode {
    let path = Path::new(path);
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: Could not read file '{}': {e}", path.display());
            return ExitCode::FAILURE;
        }
    };

    let filename = path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string());

    let tokens = match tokenize(&source) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{}", e.render(&filename, &source));
            return ExitCode::FAILURE;
        }
    };

    let program = match parse(tokens) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{}", e.render(&filename, &source));
            return ExitCode::FAILURE;
        }
    };

    let mut interpreter = Interpreter::new();
    if let Err(e) = interpreter.run(&program) {
        eprintln!("{}", e.render(&filename, &source));
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
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
