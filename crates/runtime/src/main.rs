use clap::Parser;
use n2np_common::Profile;
use std::fs;

#[derive(Parser, Debug)]
#[command(name = "n2np-runtime")]
#[command(about = "n2np runtime - run profiled Windows apps natively on Linux", long_about = None)]
struct Args {
    #[arg(long)]
    profile: String,
    #[arg(long)]
    target: Option<String>,
}

fn main() {
    env_logger::init();
    let args = Args::parse();
    let data = fs::read_to_string(&args.profile).expect("failed to read profile");
    let profile: Profile = serde_json::from_str(&data).expect("failed to parse profile");
    println!("[n2np-runtime] loaded profile: {}", profile.target.name);
    println!("[n2np-runtime] imports: {}", profile.imports.len());
    for imp in &profile.imports {
        if imp.called {
            println!("  [call] {}!{}", imp.dll, imp.name);
        }
    }
    if let Some(t) = args.target {
        println!("[n2np-runtime] target: {}", t);
    }
    println!("[n2np-runtime] (stub) native mapping not implemented yet");
}