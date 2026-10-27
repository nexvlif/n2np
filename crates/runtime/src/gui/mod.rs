#[cfg(target_os = "linux")]
pub fn run_gui() {
    log::info!("[n2np-gui] starting gui stub (winit)");
}

#[cfg(not(target_os = "linux"))]
pub fn run_gui() {
    log::info!("[n2np-gui] gui only on linux for now");
}