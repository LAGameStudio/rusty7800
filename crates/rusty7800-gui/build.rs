//! Windows-only: embed `assets/icon.ico` as the exe's Explorer icon
//! (the titlebar/taskbar icon comes from the eframe viewport icon in
//! `main.rs`, which works on every platform).
//!
//! A missing resource compiler (rc.exe / windres) is a warning, not an
//! error, so cross-compiles from hosts without one still succeed — the
//! exe just falls back to the default system icon.

fn main() {
    #[cfg(target_os = "windows")]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        if let Err(e) = res.compile() {
            println!(
                "cargo:warning=icon resource not embedded ({e}); \
                 continuing with the default exe icon"
            );
        }
    }
}
