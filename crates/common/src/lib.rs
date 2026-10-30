use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub version: u32,
    pub target: TargetInfo,
    pub modules: Vec<Module>,
    pub imports: Vec<Import>,
    pub files: Vec<String>,
    pub registry: Vec<String>,
    pub threads: bool,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetInfo {
    pub name: String,
    pub path: Option<String>,
    pub arch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Module {
    pub name: String,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Import {
    pub dll: String,
    pub name: String,
    pub ordinal: Option<u16>,
    pub called: bool,
}

impl Profile {
    pub fn new(name: &str) -> Self {
        Self {
            version: 1,
            target: TargetInfo {
                name: name.to_string(),
                path: None,
                arch: "x86_64".to_string(),
            },
            modules: vec![],
            imports: vec![],
            files: vec![],
            registry: vec![],
            threads: false,
            notes: None,
        }
    }
}
