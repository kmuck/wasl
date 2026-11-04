//! `weasel`, the build tool of the WaSL language.

mod call;
mod compose;
mod deps;
mod error;
mod init;
mod manifest;
mod project;

use std::fs;
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
    /// Remove the build directory
    Clean,
}

fn main() -> ExitCode {
    match run(Cli::parse().command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn run(command: Command) -> Result<()> {
    match command {
        Command::Init { package } => init::init(&package),
        Command::Build { world } => build(&world),
        Command::Check => {
            let project = Project::load()?;
            project.analyze()?;
            eprintln!("No errors in `{}`", project.package.name);
            Ok(())
        }
        Command::Wit => {
            let project = Project::load()?;
            let wit = project
                .analyze()?
                .wit()
                .map_err(|d| Error::diagnostic(&d, &project.sources))?;
            print!("{wit}");
            Ok(())
        }
        Command::Call {
            world,
            function,
            args,
        } => {
            if let Some(result) = call::call(&Project::load()?, &world, &function, &args)? {
                println!("{result}");
            }
            Ok(())
        }
        Command::Clean => {
            if Path::new(BUILD_DIR).exists() {
                fs::remove_dir_all(BUILD_DIR)
                    .map_err(|e| Error::new(format!("cannot remove `{BUILD_DIR}`: {e}")))?;
                eprintln!("Removed `{BUILD_DIR}/`");
            }
            Ok(())
        }
    }
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
