//! `weasel`, the build tool of the WaSL language.

mod call;
mod error;
mod init;
mod manifest;
mod project;

use std::fs;
use std::path::Path;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use error::{Error, Result};
use project::{BUILD_DIR, Project};

#[derive(Parser)]
#[command(name = "weasel", version, about = "Build tool for the WaSL language")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a new project in a new directory
    Init {
        /// Project name (kebab-case), also used as the package name
        name: String,
        /// Package namespace
        #[arg(long, default_value = "local")]
        namespace: String,
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
    /// Call a function exported by a component of the project
    Call {
        /// Function to call: `interface::function`, or `function` if unambiguous
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
        Command::Init { name, namespace } => init::init(&name, &namespace),
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
        Command::Call { function, args } => {
            if let Some(result) = call::call(&Project::load()?, &function, &args)? {
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
