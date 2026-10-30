use n2np_common::{Import, Module, Profile};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone)]
pub struct ApiCall {
    pub dll: &'static str,
    pub name: &'static str,
    pub count: u64,
}

#[derive(Clone)]
struct TraceOutput {
    target: String,
    path: String,
    template: Option<Profile>,
}

static ENABLED: AtomicBool = AtomicBool::new(false);
static CALLS: OnceLock<Mutex<Vec<ApiCall>>> = OnceLock::new();
static OUTPUT: OnceLock<Mutex<Option<TraceOutput>>> = OnceLock::new();

fn calls() -> &'static Mutex<Vec<ApiCall>> {
    CALLS.get_or_init(|| Mutex::new(Vec::new()))
}

fn output() -> &'static Mutex<Option<TraceOutput>> {
    OUTPUT.get_or_init(|| Mutex::new(None))
}

pub fn enable(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

pub fn record(dll: &'static str, name: &'static str) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let mut calls = calls().lock().unwrap();
    if let Some(call) = calls
        .iter_mut()
        .find(|call| call.dll == dll && call.name == name)
    {
        call.count += 1;
    } else {
        calls.push(ApiCall {
            dll,
            name,
            count: 1,
        });
    }
}

pub fn snapshot() -> Vec<ApiCall> {
    calls().lock().unwrap().clone()
}

pub fn reset() {
    calls().lock().unwrap().clear();
}

pub fn set_output(target: &str, path: &str, template: Option<Profile>) {
    *output().lock().unwrap() = Some(TraceOutput {
        target: target.to_string(),
        path: path.to_string(),
        template,
    });
}

pub fn flush_output() -> std::io::Result<Option<String>> {
    let output = output().lock().unwrap().clone();
    match output {
        Some(output) => {
            write_profile(output.template.as_ref(), &output.target, &output.path)?;
            Ok(Some(output.path))
        }
        None => Ok(None),
    }
}

pub fn write_profile(
    template: Option<&Profile>,
    target_path: &str,
    out_path: &str,
) -> std::io::Result<Profile> {
    let name = std::path::Path::new(target_path)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("unknown");

    let mut profile = template.cloned().unwrap_or_else(|| Profile::new(name));
    profile.target.name = name.to_string();
    profile.target.path = Some(target_path.to_string());
    profile.target.arch = "x86_64".to_string();
    profile.notes = Some("generated from a runtime trace".to_string());

    for call in snapshot() {
        if !profile.modules.iter().any(|module| module.name == call.dll) {
            profile.modules.push(Module {
                name: call.dll.to_string(),
                required: true,
            });
        }
        if let Some(import) = profile
            .imports
            .iter_mut()
            .find(|import| import.dll == call.dll && import.name == call.name)
        {
            import.called = true;
        } else {
            profile.imports.push(Import {
                dll: call.dll.to_string(),
                name: call.name.to_string(),
                ordinal: None,
                called: true,
            });
        }
    }

    let json = serde_json::to_string_pretty(&profile)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))?;
    std::fs::write(out_path, json)?;
    Ok(profile)
}
