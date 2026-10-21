Rusty7800 0.1.0 — Atari 7800 headless runner
==============================================

Runs a ROM for N frames and writes the last frame as a binary PPM
screenshot, printing the frame hash. For scripting, screenshots, and
regression checks. No ROMs are included: bring your own legally-dumped
.a78/.bin images. No BIOS file is required (--bios is optional).

USAGE
  rusty7800-cli.exe --rom <file.a78|file.bin> [--machine <driver>]
                    [--frames N] [--out frame.ppm] [--bios bios.rom]

  Defaults: --machine a7800, --frames 60, --out frame.ppm.

  Example:
    rusty7800-cli.exe --rom game.a78 --machine a7800p --frames 120 --out shot.ppm

MACHINES
  a7800 a7800u1 a7800u2 a7800dev a7800p a7800pu1 a7800pu2 a7800pdev
  (NTSC/PAL x Warm/Cool/Hot/Developer Mode. Match the machine region
  to the ROM region — mismatched games hang instead of booting.)

Built from https://github.com/LAGameStudio/rusty7800 (BSD-3-Clause).
