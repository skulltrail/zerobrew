use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "zb")]
#[command(about = "Zerobrew - A fast Homebrew-compatible package installer")]
#[command(version)]
pub struct Cli {
    #[arg(long, env = "ZEROBREW_ROOT")]
    pub root: Option<PathBuf>,

    #[arg(long, env = "ZEROBREW_PREFIX")]
    pub prefix: Option<PathBuf>,

    #[arg(long, default_value = "48")]
    pub concurrency: usize,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Install formulas
    Install {
        #[arg(required = true, num_args = 1..)]
        formulas: Vec<String>,
        #[arg(long)]
        no_link: bool,
        /// Install as cask (macOS application)
        #[arg(long)]
        cask: bool,
    },
    /// Install packages from a Brewfile manifest
    Bundle {
        #[arg(long, short = 'f', value_name = "FILE", default_value = "Brewfile")]
        file: PathBuf,
        #[arg(long)]
        no_link: bool,
    },
    /// Uninstall a formula or cask
    Uninstall {
        formula: Option<String>,
        /// Uninstall a cask
        #[arg(long)]
        cask: bool,
    },
    /// Migrate from Homebrew
    Migrate {
        #[arg(long, short = 'y')]
        yes: bool,
        #[arg(long)]
        force: bool,
    },
    /// List installed formulas and casks
    List {
        /// List only casks
        #[arg(long)]
        cask: bool,
    },
    /// Show info about an installed formula or cask
    Info {
        formula: String,
        /// Show info for a cask
        #[arg(long)]
        cask: bool,
    },
    /// Garbage collect unreferenced store entries
    Gc,
    /// Reset zerobrew (delete all data)
    Reset {
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// Initialize zerobrew directories
    Init,
    /// Generate shell completions
    Completion {
        #[arg(value_enum)]
        shell: clap_complete::shells::Shell,
    },
    /// Run a formula without linking
    #[command(disable_help_flag = true)]
    Run {
        formula: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Show outdated formulas and casks
    Outdated {
        /// Check only casks
        #[arg(long)]
        cask: bool,
    },
    /// Upgrade outdated formulas and casks
    Upgrade {
        /// Specific formulas to upgrade (upgrades all if empty)
        formulas: Vec<String>,
        /// Upgrade only casks
        #[arg(long)]
        cask: bool,
    },
    /// Update formula metadata from Homebrew API
    Update,
    /// Remove old versions and cleanup cache
    Cleanup {
        /// Only show what would be cleaned up
        #[arg(long, short = 'n')]
        dry_run: bool,
        /// Scrub the cache, removing downloads for even the latest versions
        #[arg(long, short = 's')]
        scrub: bool,
    },
    /// Check system for potential problems
    Doctor,
}
