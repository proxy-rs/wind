use std::path::PathBuf;

use clap::{ArgAction, Parser, Subcommand};

#[derive(Parser)]
#[command(about, long_about = None)]
pub struct Cli {
	/// Sets a custom config file
	#[arg(short, visible_short_alias = 'f', long, value_name = "FILE")]
	pub config: Option<PathBuf>,

	/// Show current version
	#[arg(short = 'v', visible_short_alias = 'V', long, action = ArgAction::SetTrue)]
	pub version: bool,

	#[command(subcommand)]
	pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
	/// does testing things
	Test {
		/// lists test values
		#[arg(short, long)]
		list: bool,
	},
}
