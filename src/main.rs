use anyhow::{Result, bail};
use clap::Parser;
use std::{
    io::{self, IsTerminal},
    path::PathBuf,
};

mod app;
mod comments;
mod diff;
mod diff_view;
mod file_tree;
mod filters;
mod git;
mod search;
mod settings;
mod storage;

#[derive(Parser, Debug)]
#[command(version, about = "Fast syntax-aware worktree diff review")]
struct Cli {
    /// Directory inside the Git worktree to review
    #[arg(default_value = ".")]
    dir: PathBuf,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let repo = git::find_repo(&cli.dir)?;
    if !io::stdout().is_terminal() {
        bail!("Luminatti needs an interactive terminal");
    }
    app::run(repo)
}
