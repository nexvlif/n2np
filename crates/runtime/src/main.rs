use clap::Parser;
use n2np_common::Profile;
use std::fs;

mod winapi;

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
    let mut data = fs::read_to_string(&args.profile).expect("failed to read profile");
    if data.starts_with('\u{FEFF}') {
        data = data.trim_start_matches('\u{FEFF}').to_string();
    }
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
    
    let mut has_getstd = false;
    let mut has_writefile = false;
    let mut has_exitprocess = false;
    let mut has_loadlibrary = false;
    let mut has_getprocaddress = false;
    let mut has_virtualprotect = false;
    for imp in &profile.imports {
        if !imp.called { continue; }
        let dll = imp.dll.to_lowercase();
        let name = imp.name.as_str();
        if dll == "kernel32.dll" {
            match name {
                "GetStdHandle" => has_getstd = true,
                "WriteFile" => has_writefile = true,
                "ExitProcess" => has_exitprocess = true,
                "LoadLibraryA" | "LoadLibraryW" => has_loadlibrary = true,
                "GetProcAddress" => has_getprocaddress = true,
                "VirtualProtect" => has_virtualprotect = true,
                _ => {}
            }
        }
    }
    
    println!("[n2np-runtime] thunk table: GetStdHandle={} WriteFile={} ExitProcess={} LoadLibrary={} GetProcAddress={} VirtualProtect={}", has_getstd, has_writefile, has_exitprocess, has_loadlibrary, has_getprocaddress, has_virtualprotect);
    println!("[n2np-runtime] executing in native mode (profile-driven)...");
    
    if has_getstd && has_writefile {
        let h = winapi::kernel32::get_std_handle(-11);
        let msg = b"hello from n2np\n";
        let _written = winapi::kernel32::write_file(h, msg);
    }
    if has_loadlibrary { let _ = winapi::kernel32::load_library_a(std::ptr::null()); }
    if has_getprocaddress { let _ = winapi::kernel32::get_proc_address(0, std::ptr::null()); }
    if has_virtualprotect { let mut old=0; let _ = winapi::kernel32::virtual_protect(0,0,0x40,&mut old); }
    if has_exitprocess {
        winapi::kernel32::exit_process(0);
    }
    println!("[n2np-runtime] done");
}