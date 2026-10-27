use clap::{Parser, Subcommand};
use std::fs;

#[derive(Parser, Debug)]
#[command(name = "n2np-trace")]
#[command(about = "n2np trace")]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Static {
        #[arg(long)]
        target: String,
        #[arg(long)]
        out: Option<String>,
    },
}

fn main() {
    env_logger::init();
    let args = Args::parse();
    match args.command {
        Commands::Static { target, out } => {
            println!("[n2np-trace] static");
            match n2np_pe::parse_pe_imports(&target) {
                Ok(profile) => {
                    let out_path = out.unwrap_or_else(|| format!("profiles/{}.json", profile.target.name));
                    let json = serde_json::to_string_pretty(&profile).expect("serialize");
                    fs::write(&out_path, json).expect("write");
                    println!("[n2np-trace] wrote {}", out_path);
                    for imp in &profile.imports {
                        println!("  {}!{}", imp.dll, imp.name);
                    }
                }
                Err(e) => {
                    eprintln!("error: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }
}