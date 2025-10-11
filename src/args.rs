use clap::Parser;

#[derive(Parser, Debug)]
pub struct Args {
    #[clap(short, long, default_value_t = false, help = "Verbose output")]
    pub verbose: bool,
    #[clap(short, long, help = "Path to configuration file")]
    pub config_file: Option<String>,
}
