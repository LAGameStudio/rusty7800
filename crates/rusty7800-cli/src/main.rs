//! Rusty7800 headless runner (native exe).
//!
//! Loads a `.a78`/`.bin` image, steps N frames, writes the last frame as
//! a binary PPM screenshot (pens mapped through the machine palette), and
//! prints the frame hash. Zero dependencies beyond the core: `cargo build
//! -p rusty7800-cli --release` produces the Windows exe.
//!
//! Usage: `rusty7800-cli --rom game.a78 [--machine a7800] [--frames 60]
//!           [--out frame.ppm] [--bios 7800.u7]`

use rusty7800_core::cart::load_image;
use rusty7800_core::machine::Machine;
use rusty7800_core::state::fnv1a;
use rusty7800_core::timing::MachineId;

use std::process::ExitCode;

fn usage() -> &'static str {
    "usage: rusty7800-cli --rom <file.a78|file.bin> [--machine <driver>] [--frames N] [--out frame.ppm] [--bios bios.rom]\n\
     machines: a7800 a7800u1 a7800u2 a7800dev a7800p a7800pu1 a7800pu2 a7800pdev"
}

fn parse_machine(name: &str) -> Option<MachineId> {
    MachineId::all()
        .into_iter()
        .find(|m| m.driver_name() == name)
}

/// Binary P6 PPM: no image crate needed.
fn write_ppm(path: &str, w: usize, h: usize, rgb: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::File::create(path)?;
    f.write_all(format!("P6\n{w} {h}\n255\n").as_bytes())?;
    f.write_all(rgb)?;
    Ok(())
}

fn run() -> Result<(), String> {
    let mut rom: Option<String> = None;
    let mut machine = "a7800".to_string();
    let mut frames: u64 = 60;
    let mut out = "frame.ppm".to_string();
    let mut bios: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--rom" => rom = Some(args.next().ok_or("--rom needs a file")?),
            "--machine" => machine = args.next().ok_or("--machine needs a name")?,
            "--frames" => {
                frames = args
                    .next()
                    .ok_or("--frames needs a number")?
                    .parse()
                    .map_err(|_| "--frames needs a number")?
            }
            "--out" => out = args.next().ok_or("--out needs a file")?,
            "--bios" => bios = Some(args.next().ok_or("--bios needs a file")?),
            "--help" | "-h" => return Err(usage().to_string()),
            other => return Err(format!("unknown arg {other}\n{}", usage())),
        }
    }
    let rom_path = rom.ok_or_else(|| usage().to_string())?;
    let id =
        parse_machine(&machine).ok_or_else(|| format!("unknown machine {machine}\n{}", usage()))?;

    let image = std::fs::read(&rom_path).map_err(|e| format!("read {rom_path}: {e}"))?;
    let loaded = load_image(&image).map_err(|e| format!("load image: {e:?}"))?;
    let mut m = Machine::new(id);
    if let Some(path) = bios {
        // NTSC 4K BIOS images sit at 0x3000; full 16K images at 0x0000
        // (same rule as `Machine::load_bios` callers in the test suite).
        let file = std::fs::read(&path).map_err(|e| format!("read {path}: {e}"))?;
        m.load_bios(&file, if file.len() <= 0x1000 { 0x3000 } else { 0x0000 });
    }
    m.load_cart(loaded);
    m.reset();
    for _ in 0..frames {
        m.step_frame();
    }
    let (w, h) = m.frame_dims();
    let pal = id.palette();
    let mut rgb = Vec::with_capacity(w * h * 3);
    for pen in m.frame() {
        let (r, g, b) = pal[*pen as usize];
        rgb.extend_from_slice(&[r, g, b]);
    }
    write_ppm(&out, w, h, &rgb).map_err(|e| format!("write {out}: {e}"))?;
    println!(
        "machine={} slot={} frames={frames} pc={:04X} hash={:08x} {w}x{h} -> {out}",
        id.driver_name(),
        m.slot_key().unwrap_or("none"),
        m.sally.pc,
        fnv1a(m.frame())
    );
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("rusty7800-cli: {e}");
            ExitCode::FAILURE
        }
    }
}
