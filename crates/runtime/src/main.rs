use clap::Parser;
use n2np_common::Profile;
use std::fs;
use std::path::PathBuf;

#[macro_use]
mod macros;

mod error;
mod gui;
mod handles;
#[cfg(target_os = "linux")]
mod loader;
mod memory;
mod modules;
mod prefix;
mod trace;
mod utf;
mod winapi;

#[derive(Parser, Debug)]
#[command(name = "n2np-runtime")]
#[command(about = "n2np runtime - run profiled Windows apps natively on Linux")]
#[command(version)]
struct Args {
    #[arg(long)]
    profile: Option<String>,
    #[arg(long)]
    target: Option<String>,
    #[arg(long)]
    run: bool,
    #[arg(long)]
    trace: bool,
    #[arg(long)]
    trace_out: Option<String>,
    #[arg(long = "dll-path")]
    dll_paths: Vec<String>,
    #[arg(long)]
    prefix: Option<String>,
    #[arg(long = "dll-override")]
    dll_overrides: Vec<String>,
    #[arg(long)]
    list_apis: bool,
}

fn load_profile(path: &str) -> Result<Profile, String> {
    let mut data =
        fs::read_to_string(path).map_err(|err| format!("failed to read profile: {err}"))?;
    if data.starts_with('\u{FEFF}') {
        data = data.trim_start_matches('\u{FEFF}').to_string();
    }
    serde_json::from_str(&data).map_err(|err| format!("failed to parse profile: {err}"))
}

fn print_apis() {
    let apis = winapi::supported_apis();
    let mut current = "";
    for (dll, name) in &apis {
        if *dll != current {
            if !current.is_empty() {
                println!();
            }
            println!("{dll}:");
            current = *dll;
        }
        println!("  {name}");
    }
    println!();
    println!("{} APIs implemented", apis.len());
}

fn describe_profile(profile: &Profile) {
    println!("[n2np-runtime] loaded profile: {}", profile.target.name);
    println!("[n2np-runtime] imports: {}", profile.imports.len());
    let mut missing = 0usize;
    for import in &profile.imports {
        if !import.called {
            continue;
        }
        let supported = match import.ordinal {
            Some(ordinal) if import.name.is_empty() => {
                winapi::resolve_ordinal(&import.dll, ordinal).is_some()
            }
            _ => winapi::resolve(&import.dll, &import.name).is_some(),
        };
        if supported {
            println!("  [ready] {}!{}", import.dll, import.name);
        } else {
            missing += 1;
            println!("  [missing] {}!{}", import.dll, import.name);
        }
    }
    if missing == 0 {
        println!("[n2np-runtime] all called imports have implementations");
    } else {
        println!("[n2np-runtime] {missing} called imports are not implemented yet");
    }
}

fn main() {
    env_logger::init();
    let args = Args::parse();

    if args.list_apis {
        print_apis();
        return;
    }

    let profile = match &args.profile {
        Some(path) => match load_profile(path) {
            Ok(profile) => Some(profile),
            Err(err) => {
                eprintln!("error: {err}");
                std::process::exit(2);
            }
        },
        None => None,
    };

    if args.run {
        run_pe(&args, profile);
        return;
    }

    match profile {
        Some(profile) => describe_profile(&profile),
        None => {
            eprintln!("error: --profile is required unless --run or --list-apis is used");
            std::process::exit(2);
        }
    }
}

fn run_pe(args: &Args, profile: Option<Profile>) {
    let target = match &args.target {
        Some(target) => target,
        None => {
            eprintln!("error: --target is required with --run");
            std::process::exit(2);
        }
    };

    if let Some(profile) = &profile {
        describe_profile(profile);
    }

    let tracing =
        args.trace || std::env::var_os("N2NP_TRACE").is_some() || args.trace_out.is_some();
    if tracing {
        trace::enable(true);
    }
    if let Some(out) = &args.trace_out {
        trace::set_output(target, out, profile.clone());
        println!("[n2np-runtime] runtime trace will be written to {out}");
    }

    let root = args
        .prefix
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(prefix::default_root);
    let root = prefix::set_root(root);
    let prefix = prefix::Prefix::new(&root);
    if let Err(err) = prefix.ensure() {
        eprintln!(
            "warning: failed to initialize prefix at {}: {err}",
            root.display()
        );
    }
    println!("[n2np-runtime] prefix: {}", root.display());

    let overrides = match prefix::collect_overrides(&args.dll_overrides) {
        Ok(overrides) => overrides,
        Err(err) => {
            eprintln!("error: {err}");
            std::process::exit(2);
        }
    };
    for entry in &overrides {
        println!(
            "[n2np-runtime] DLL override: {} -> {}",
            entry.name,
            entry.path.display()
        );
    }
    prefix::set_overrides(overrides);

    #[cfg(target_os = "linux")]
    {
        println!("[n2np-runtime] loading PE image: {target}");
        match loader::run(target, &args.dll_paths) {
            Ok(code) => {
                if let Err(err) = trace::flush_output() {
                    eprintln!("warning: failed to write runtime profile: {err}");
                }
                println!("[n2np-runtime] process returned {code}");
            }
            Err(err) => {
                eprintln!("error: {err}");
                std::process::exit(1);
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (&args.dll_paths, profile);
        eprintln!("error: native PE execution is only available on Linux/Wayland");
        std::process::exit(1);
    }
}
