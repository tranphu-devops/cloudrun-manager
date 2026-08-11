// Không có dòng này thì bản release trên Windows sẽ mở kèm một cửa sổ console đen.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    cloud_run_cockpit_lib::run()
}
