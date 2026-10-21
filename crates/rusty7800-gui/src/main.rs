//! Rusty7800 native app (Windows exe).
//!
//! eframe window over the core: ROM picker (CLI arg or drag-and-drop),
//! machine selector, run/pause/step/reset, integer scaling, keyboard
//! input on port 1 (arrows + `Z`/`X` for the Proline buttons), TIA audio
//! through the default output device, live status (fps, frame, PC, hash).
//!
//! `--smoke` runs a hermetic self-test instead of opening a window:
//! boots an in-code bar ROM, steps 120 frames through the same
//! load/step/render path the display uses, prints the hash, exits.

use eframe::egui;

mod audio;

use rusty7800_core::cart::load_image;
use rusty7800_core::ctrl::PortDevice;
use rusty7800_core::machine::Machine;
use rusty7800_core::state::fnv1a;
use rusty7800_core::timing::MachineId;

fn driver_names() -> Vec<&'static str> {
    MachineId::all().iter().map(|m| m.driver_name()).collect()
}

/// Picker entries: `(driver name, friendly label)` — the label is the
/// MAME `full_name` (`Atari 7800 (PAL) Warm`, …) so the region is
/// visible before a ROM is even loaded.
fn machine_entries() -> Vec<(&'static str, &'static str)> {
    MachineId::all().iter().map(|m| (m.driver_name(), m.spec().full_name)).collect()
}

fn machine_label(name: &str) -> &'static str {
    MachineId::all()
        .iter()
        .find(|m| m.driver_name() == name)
        .map(|m| m.spec().full_name)
        .unwrap_or("unknown machine")
}

/// First driver whose region matches a ROM region claim (`a7800` for
/// NTSC, `a7800p` for PAL) — points the mismatch warning at a fix.
fn driver_for_region(is_pal: bool) -> &'static str {
    MachineId::all()
        .iter()
        .find(|m| m.spec().is_pal == is_pal)
        .map(|m| m.driver_name())
        .unwrap_or("a7800")
}

fn machine_id(name: &str) -> MachineId {
    MachineId::all()
        .into_iter()
        .find(|m| m.driver_name() == name)
        .unwrap_or(MachineId::Ntsc)
}

/// Pens -> RGBA through the machine palette (shared by display + `--smoke`).
fn render_rgba(m: &Machine, out: &mut Vec<u8>) {
    let pal = m.machine_id().palette();
    let (w, h) = m.frame_dims();
    out.resize(w * h * 4, 0);
    for (dst, pen) in out.as_chunks_mut::<4>().0.iter_mut().zip(m.frame()) {
        let (r, g, b) = pal[*pen as usize];
        dst[0] = r;
        dst[1] = g;
        dst[2] = b;
        dst[3] = 0xFF;
    }
}

/// In-code bar ROM for `--smoke` (geometry authority: phase7_machine.rs).
fn smoke_image() -> Vec<u8> {
    let mut img = vec![0xFFu8; 0x1000];
    #[rustfmt::skip]
    let code: &[u8] = &[
        0x78, 0xD8, 0xA2, 0xFF, 0x9A, 0xA9, 0x00, 0x8D, 0x20, 0x00,
        0xA9, 0x05, 0x8D, 0x21, 0x00, 0xA9, 0x3F, 0x8D, 0x27, 0x00,
        0xA9, 0xF1, 0x8D, 0x2C, 0x00, 0xA9, 0x00, 0x8D, 0x30, 0x00,
        0xA9, 0x40, 0x8D, 0x3C, 0x00, 0x4C, 0x23, 0xF0,
    ];
    img[..code.len()].copy_from_slice(code);
    img[0x100..0x108].copy_from_slice(&[0x0F, 0xF1, 0x10, 0x00, 0x00, 0xF1, 0x2F, 0x00]);
    img[0x110..0x114].copy_from_slice(&[0x26, 0x38, 0xF0, 20]);
    img[0x115] = 0x00;
    img[0x026..0x02E].copy_from_slice(&[0xFF; 8]);
    img[0x130] = 0x00;
    img[0xFFA] = 0x23;
    img[0xFFB] = 0xF0;
    img[0xFFC] = 0x00;
    img[0xFFD] = 0xF0;
    img[0xFFE] = 0x23;
    img[0xFFF] = 0xF0;
    img
}

fn smoke() -> Result<(), String> {
    let mut m = Machine::new(MachineId::Ntsc);
    m.load_cart(load_image(&smoke_image()).map_err(|e| format!("{e:?}"))?);
    m.reset();
    let mut rgba = Vec::new();
    for _ in 0..120 {
        m.step_frame();
    }
    render_rgba(&m, &mut rgba);
    assert_eq!(rgba.len(), 320 * 263 * 4);
    println!(
        "smoke: pc={:04X} hash={:08x} rgba={} bytes",
        m.sally.pc,
        fnv1a(m.frame()),
        rgba.len()
    );
    if m.sally.pc != 0xF023 || !m.frame().contains(&0x3F) {
        return Err("smoke: bar ROM did not boot and draw".into());
    }
    Ok(())
}

struct App {
    machine: Machine,
    machine_name: String,
    rom_bytes: Option<Vec<u8>>,
    rom_label: String,
    running: bool,
    frames: u64,
    scale: usize,
    texture: Option<egui::TextureHandle>,
    rgba: Vec<u8>,
    fps_ema: f64,
    last_tick: std::time::Instant,
    status: String,
    audio: Option<audio::AudioOut>,
    audio_note: String,
}

impl App {
    fn new(
        cc: &eframe::CreationContext<'_>,
        rom_path: Option<String>,
        machine_name: String,
    ) -> Self {
        let mut app = App {
            machine: Machine::new(machine_id(&machine_name)),
            machine_name,
            rom_bytes: None,
            rom_label: "drop a .a78/.bin here".into(),
            running: false,
            frames: 0,
            scale: 2,
            texture: None,
            rgba: Vec::new(),
            fps_ema: 60.0,
            last_tick: std::time::Instant::now(),
            status: String::new(),
            audio: None,
            audio_note: String::new(),
        };
        // Audio is best-effort: headless/CI machines have no device, and
        // the app must stay usable silent.
        match audio::start(rusty7800_core::machine::TIA_AUDIO_CLOCK) {
            Ok(out) => {
                app.audio_note = format!("audio {}Hz", out.out_rate);
                app.audio = Some(out);
            }
            Err(e) => app.audio_note = format!("silent ({e})"),
        }
        app.machine.reset();
        if let Some(path) = rom_path {
            app.load_file(&path);
        }
        let (w, h) = app.machine.frame_dims();
        app.texture = Some(cc.egui_ctx.load_texture(
            "screen",
            egui::ColorImage::from_rgba_unmultiplied([w, h], &vec![0; w * h * 4]),
            egui::TextureOptions::NEAREST,
        ));
        app
    }

    fn load_bytes(&mut self, bytes: Vec<u8>, label: String) {
        match load_image(&bytes) {
            Ok(loaded) => {
                let slot = loaded.slot_key.to_string();
                // Rebuild so a machine switch can't leak old-device state.
                self.machine = Machine::new(machine_id(&self.machine_name));
                self.machine.load_cart(loaded);
                self.machine.reset();
                self.rom_bytes = Some(bytes);
                self.rom_label = format!("{label} ({slot})");
                self.frames = 0;
                self.running = true;
                self.status.clear();
            }
            Err(e) => self.status = format!("load failed: {e:?}"),
        }
    }

    fn load_file(&mut self, path: &str) {
        match std::fs::read(path) {
            Ok(bytes) => self.load_bytes(bytes, path.to_string()),
            Err(e) => self.status = format!("read {path}: {e}"),
        }
    }

    /// Drain one frame's TIA samples into the device queue (if any).
    fn feed_audio(&mut self) {
        if let Some(audio) = &self.audio {
            audio.feed(&self.machine.take_audio());
        } else {
            self.machine.take_audio();
        }
    }

    /// Keyboard -> port-1 Proline (arrows + Z/X); no-ops on other devices.
    fn poll_input(&mut self, ctx: &egui::Context) {
        if let Some(PortDevice::Proline(d)) = self.machine.port1.device_mut() {
            ctx.input(|i| {
                d.up = i.key_down(egui::Key::ArrowUp);
                d.down = i.key_down(egui::Key::ArrowDown);
                d.left = i.key_down(egui::Key::ArrowLeft);
                d.right = i.key_down(egui::Key::ArrowRight);
                d.btn_a = i.key_down(egui::Key::Z);
                d.btn_b = i.key_down(egui::Key::X);
            });
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // Drag-and-drop ROM loading (native: absolute path always present).
        for file in ctx.input(|i| i.raw.dropped_files.clone()) {
            self.load_file(&file.path().to_string_lossy());
        }

        egui::Panel::top("menu").show(&mut *ui, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .button(if self.running { "Pause" } else { "Run" })
                    .clicked()
                {
                    self.running = !self.running;
                }
                if ui.button("Step 1f").clicked() {
                    self.running = false;
                    self.machine.step_frame();
                    self.frames += 1;
                    self.feed_audio();
                }
                if ui.button("Reset").clicked() {
                    self.machine.reset();
                    self.frames = 0;
                }
                egui::ComboBox::from_label("Machine")
                    .selected_text(machine_label(&self.machine_name))
                    .show_ui(ui, |ui| {
                        for (driver, label) in machine_entries() {
                            if ui
                                .selectable_value(&mut self.machine_name, driver.to_string(), label)
                                .clicked()
                            {
                                let rom = self.rom_bytes.clone();
                                self.machine = Machine::new(machine_id(&self.machine_name));
                                self.machine.reset();
                                if let Some(bytes) = rom {
                                    // Bypass load_bytes (same bytes, new machine).
                                    if let Ok(loaded) = load_image(&bytes) {
                                        self.machine.load_cart(loaded);
                                        self.machine.reset();
                                        self.frames = 0;
                                        self.running = true;
                                    }
                                }
                            }
                        }
                    });
                egui::ComboBox::from_label("Scale")
                    .selected_text(self.scale.to_string())
                    .show_ui(ui, |ui| {
                        for s in [1usize, 2, 3, 4] {
                            ui.selectable_value(&mut self.scale, s, s.to_string());
                        }
                    });
                ui.label(&self.rom_label);
            });
        });

        egui::Panel::bottom("status").show(&mut *ui, |ui| {
            ui.horizontal(|ui| {
                ui.monospace(format!(
                    "{:.0} fps  frame {}  pc=${:04X}  hash={:08x}  slot={}",
                    self.fps_ema,
                    self.frames,
                    self.machine.sally.pc,
                    fnv1a(self.machine.frame()),
                    self.machine.slot_key().unwrap_or("none"),
                ));
                ui.monospace(&self.audio_note);
                // ROM TV region vs machine region: PAL-timed games hang
                // on a 263-line machine (and vice versa), so a mismatch
                // gets a loud pointer at the right machine.
                match self.machine.rom_is_pal() {
                    Some(is_pal) => {
                        ui.monospace(if is_pal { "ROM: PAL" } else { "ROM: NTSC" });
                        if self.machine.region_mismatch() {
                            ui.colored_label(
                                egui::Color32::RED,
                                format!(
                                    "REGION MISMATCH — {} ROM on {} machine (try {})",
                                    if is_pal { "PAL" } else { "NTSC" },
                                    if is_pal { "NTSC" } else { "PAL" },
                                    driver_for_region(is_pal),
                                ),
                            );
                        }
                    }
                    None => {
                        if self.machine.has_cart() {
                            ui.monospace("ROM: region unknown");
                        }
                    }
                }
                if !self.status.is_empty() {
                    ui.colored_label(egui::Color32::RED, &self.status);
                }
            });
        });

        if self.running {
            self.poll_input(&ctx);
            let now = std::time::Instant::now();
            let dt = now.duration_since(self.last_tick).as_secs_f64().max(1e-6);
            self.fps_ema += (1.0 / dt - self.fps_ema) * 0.05;
            self.last_tick = now;
            self.machine.step_frame();
            self.frames += 1;
            self.feed_audio();
        }

        egui::CentralPanel::default().show(&mut *ui, |ui| {
            render_rgba(&self.machine, &mut self.rgba);
            let (w, h) = self.machine.frame_dims();
            let img = egui::ColorImage::from_rgba_unmultiplied([w, h], &self.rgba);
            if let Some(tex) = &mut self.texture {
                tex.set(img, egui::TextureOptions::NEAREST);
                ui.image(egui::load::SizedTexture::new(
                    tex.id(),
                    egui::vec2((w * self.scale) as f32, (h * self.scale) as f32),
                ));
            }
        });

        if self.running {
            ui.ctx().request_repaint();
        }
    }
}

fn main() -> eframe::Result<()> {
    let mut rom: Option<String> = None;
    let mut machine_name = "a7800".to_string();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--smoke" => {
                return match smoke() {
                    Ok(()) => Ok(()),
                    Err(e) => {
                        eprintln!("rusty7800-gui --smoke: {e}");
                        std::process::exit(1);
                    }
                }
            }
            "--rom" => rom = Some(args.next().unwrap_or_else(usage_and_exit)),
            "--machine" => machine_name = args.next().unwrap_or_else(usage_and_exit),
            "--help" | "-h" => {
                println!("usage: rusty7800-gui [--rom file.a78] [--machine drv] [--smoke]");
                return Ok(());
            }
            other => {
                eprintln!("unknown arg {other}");
                std::process::exit(2);
            }
        }
    }
    if !driver_names().contains(&machine_name.as_str()) {
        eprintln!("unknown machine {machine_name}");
        std::process::exit(2);
    }
    let machine_name_owned = machine_name;
    eframe::run_native(
        "Rusty7800",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([700.0, 640.0]),
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(App::new(cc, rom, machine_name_owned)) as Box<dyn eframe::App>)),
    )
}

fn usage_and_exit() -> String {
    eprintln!("usage: rusty7800-gui [--rom file.a78] [--machine drv] [--smoke]");
    std::process::exit(2);
}
