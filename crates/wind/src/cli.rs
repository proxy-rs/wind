use std::path::PathBuf;

use clap::{ArgAction, Parser, Subcommand};

#[derive(Parser)]
#[command(about, long_about = None)]
pub struct Cli {
	/// Set a custom config
	#[arg(short, visible_short_alias = 'f', long, value_name = "FILE/BASE64-TEXT")]
	pub config: Option<String>,

	/// Set configuration directory
	#[arg(short = 'C', visible_short_alias = 'd', long, value_name = "PATH")]
	pub config_dir: Option<PathBuf>,

	/// Set working directory
	#[arg(short = 'D', long, value_name = "PATH")]
	pub work_dir: Option<PathBuf>,

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
