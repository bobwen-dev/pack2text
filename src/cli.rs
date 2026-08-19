use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "pack2text",
    version,
    about = "Pack text files into multipart container"
)]
pub struct Cli {
    #[arg(help = "Directories or files to pack")]
    pub directories: Vec<PathBuf>,

    #[arg(short, long, help = "Output file path")]
    pub output: Option<PathBuf>,

    #[arg(short, long, help = "Output to clipboard instead of file")]
    pub clipboard: bool,

    #[arg(short = 'f', long, help = "Overwrite existing output file")]
    pub force: bool,

    #[arg(
        long,
        help = "Context-menu mode: selected items, shared parent root, auto-rename output"
    )]
    pub menu: bool,

    #[arg(long, help = "Context-menu current directory (Explorer %V)")]
    pub menu_dir: Option<PathBuf>,

    #[arg(long, help = "Unpack container file to directory (verification)")]
    pub unpack: Option<PathBuf>,

    #[arg(long, help = "Output directory for --unpack")]
    pub unpack_dir: Option<PathBuf>,

    #[arg(long, help = "Install Windows Explorer context menu entries")]
    pub install_menu: bool,

    #[arg(long, help = "Uninstall Windows Explorer context menu entries")]
    pub uninstall_menu: bool,

    #[arg(
        long,
        action = clap::ArgAction::Append,
        help = "Include files matching glob pattern (repeatable, any match wins)"
    )]
    pub include: Option<Vec<String>>,

    #[arg(
        long,
        action = clap::ArgAction::Append,
        help = "Exclude files matching glob pattern (repeatable, any match wins)"
    )]
    pub exclude: Option<Vec<String>>,

    #[arg(short, long, help = "Verbose output")]
    pub verbose: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn include_does_not_swallow_positional_args() {
        let cli = Cli::parse_from(["pack2text", "a", "--include", "*.rs", "b"]);
        assert_eq!(
            cli.directories,
            vec![PathBuf::from("a"), PathBuf::from("b")]
        );
        assert_eq!(cli.include, Some(vec![String::from("*.rs")]));
    }

    #[test]
    fn include_repeatable() {
        let cli = Cli::parse_from(["pack2text", "a", "--include", "*.rs", "--include", "*.py"]);
        assert_eq!(
            cli.include,
            Some(vec![String::from("*.rs"), String::from("*.py")])
        );
    }

    #[test]
    fn exclude_repeatable() {
        let cli = Cli::parse_from(["pack2text", "a", "--exclude", "*.exe", "--exclude", "*.dll"]);
        assert_eq!(
            cli.exclude,
            Some(vec![String::from("*.exe"), String::from("*.dll")])
        );
    }
}
