use clap::Parser;

#[derive(Parser, Debug)]
pub struct Args {
    #[clap(short, long, default_value_t = 0, action = clap::ArgAction::Count, help = "Output verbosity level")]
    pub verbose: u8,
    #[clap(short, long, help = "Path to configuration file")]
    pub config_file: Option<String>,
}
