//! `weasel`, the build tool of the WaSL language.

mod call;
mod compose;
mod deps;
mod error;
mod host;
mod init;
mod manifest;
mod project;
mod run;
mod serve;

use std::fs;
use std::net::SocketAddr;
use std::path::Path;
use std::process::ExitCode;

use clap::{ArgAction, Args, Parser, Subcommand};

use error::{Error, Result};
use project::{BUILD_DIR, Project};

/// The command line has one spelling per option (§ 14.1): long options only, and no
/// `help` command, so `--help` and `--version` replace clap's `-h`, `-V` and `weasel help`.
#[derive(Parser)]
#[command(
    name = "weasel",
    version,
    about = "Build tool for the WaSL language",
    disable_help_flag = true,
    disable_version_flag = true,
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
    #[command(flatten)]
    help: Help,
    /// Print version
    #[arg(long, action = ArgAction::Version)]
    version: (),
}

/// `--help`, the only spelling of help, on `weasel` and on every command.
#[derive(Args)]
struct Help {
    /// Print help
    #[arg(long, action = ArgAction::Help, global = true)]
    help: (),
}

#[derive(Subcommand)]
enum Command {
    /// Create a new project in a new directory
    Init {
        /// Package of the project, `namespace:name` or `namespace:name@version`
        /// (kebab-case); the directory is named after the package name
        package: String,
    },
    /// Compile a world into a component (build/<world>.wasm)
    Build {
        /// World to build
        world: String,
    },
    /// Check the project for errors without producing components
    Check,
    /// Print the WIT of the package
    Wit,
    /// Call a function exported by a world of the project
    Call {
        /// World whose component runs the function
        world: String,
        /// Function to call: `interface::function`
        function: String,
        /// Arguments, one per parameter
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Run a world as a command, through its export of `wasi:cli/run`
    Run {
        /// World to run, then the arguments of the program, given unchanged (even
        /// `--help`)
        #[arg(
            value_names = ["WORLD", "ARGUMENTS"],
            required = true,
            trailing_var_arg = true,
            allow_hyphen_values = true
        )]
        command: Vec<String>,
    },
    /// Serve a world over HTTP, through its export of `wasi:http/handler`
    Serve {
        /// World to serve
        world: String,
        /// Address to listen on
        #[arg(long, value_name = "HOST:PORT", default_value = "127.0.0.1:8080")]
        addr: SocketAddr,
    },
    /// Remove the build directory
    Clean,
}

fn main() -> ExitCode {
    match run(Cli::parse().command) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

/// Runs a command, and returns the exit code of `weasel`: that of the program for
/// `weasel run` (§ 14.2), success otherwise.
fn run(command: Command) -> Result<ExitCode> {
    match command {
        Command::Init { package } => init::init(&package)?,
        Command::Build { world } => build(&world)?,
        Command::Check => {
            let project = Project::load()?;
            project.analyze()?;
            eprintln!("No errors in `{}`", project.package.name);
        }
        Command::Wit => {
            let project = Project::load()?;
            let wit = project
                .analyze()?
                .wit()
                .map_err(|d| Error::diagnostic(&d, &project.sources))?;
            print!("{wit}");
        }
        Command::Call {
            world,
            function,
            args,
        } => {
            if let Some(result) = call::call(&Project::load()?, &world, &function, &args)? {
                println!("{result}");
            }
        }
        Command::Run { command } => {
            let (world, args) = command.split_first().expect("a world");
            return run::run(&Project::load()?, world, args);
        }
        Command::Serve { world, addr } => serve::serve(&Project::load()?, &world, addr)?,
        Command::Clean => {
            if Path::new(BUILD_DIR).exists() {
                fs::remove_dir_all(BUILD_DIR)
                    .map_err(|e| Error::new(format!("cannot remove `{BUILD_DIR}`: {e}")))?;
                eprintln!("Removed `{BUILD_DIR}/`");
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn build(world: &str) -> Result<()> {
    let project = Project::load()?;
    let analysis = project.analyze()?;
    let bytes = project.build(&analysis, world)?;
    fs::create_dir_all(BUILD_DIR)
        .map_err(|e| Error::new(format!("cannot create `{BUILD_DIR}`: {e}")))?;
    let path = format!("{BUILD_DIR}/{world}.wasm");
    fs::write(&path, bytes).map_err(|e| Error::new(format!("cannot write `{path}`: {e}")))?;
    eprintln!("Built `{path}`");
    Ok(())
}
