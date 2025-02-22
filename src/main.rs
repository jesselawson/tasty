// Copyright (c) 2025 Jesse Lawson <jesse@lawsonry.com>
// GNU General Public License v3.0+ (see LICENSE or https://www.gnu.org/licenses/gpl-3.0.txt)

use anyhow::Result;
use clap::Parser;
use colored::Colorize;
use tasty::{Args, run_tests};

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let result = run_tests(&args).await?;

    if result.success {
        println!("\n{}", "All tests passed!".green().bold());
    } else {
        println!("\n{}", "Some tests failed".red().bold());
        std::process::exit(1);
    }

    Ok(())
}
