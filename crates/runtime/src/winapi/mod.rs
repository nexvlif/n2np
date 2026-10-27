pub mod kernel32;

#[derive(Clone, Debug)]
pub struct StdHandles {
    pub stdin: i64,
    pub stdout: i64,
    pub stderr: i64,
}