mod filter;
mod local_llm;
mod ls;
mod read;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "rtk",
    version,
    about = "Rust Token Killer - Minimize LLM token consumption",
    long_about = "A high-performance CLI proxy designed to filter and summarize system outputs before they reach your LLM context."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Verbosity level (-v, -vv, -vvv)
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,
}

#[derive(Subcommand)]
enum Commands {
    /// List directory contents in ultra-dense, token-optimized format
    Ls {
        /// Path to list (defaults to current directory)
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Maximum depth to traverse
        #[arg(short, long, default_value = "10")]
        depth: usize,

        /// Show hidden files (except .git, node_modules, etc.)
        #[arg(short = 'a', long)]
        all: bool,

        /// Output format: tree, flat, json
        #[arg(short, long, default_value = "tree")]
        format: ls::OutputFormat,
    },

    /// Read file with intelligent filtering (strips comments, docstrings, whitespace)
    Read {
        /// File to read
        file: PathBuf,

        /// Filter level: none, minimal, aggressive
        #[arg(short, long, default_value = "minimal")]
        level: filter::FilterLevel,

        /// Maximum lines to output (smart truncation keeps signatures)
        #[arg(short, long)]
        max_lines: Option<usize>,

        /// Show line numbers
        #[arg(short = 'n', long)]
        line_numbers: bool,
    },

    /// Generate AI-powered 2-line technical summary using local LLM
    Smart {
        /// File to summarize
        file: PathBuf,

        /// Model to use (default: Llama-3.2-1B)
        #[arg(short, long, default_value = "meta-llama/Llama-3.2-1B-Instruct")]
        model: String,

        /// Force re-download of model
        #[arg(long)]
        force_download: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Ls {
            path,
            depth,
            all,
            format,
        } => {
            ls::run(&path, depth, all, format, cli.verbose)?;
        }
        Commands::Read {
            file,
            level,
            max_lines,
            line_numbers,
        } => {
            read::run(&file, level, max_lines, line_numbers, cli.verbose)?;
        }
        Commands::Smart {
            file,
            model,
            force_download,
        } => {
            local_llm::run(&file, &model, force_download, cli.verbose)?;
        }
    }

    Ok(())
}
