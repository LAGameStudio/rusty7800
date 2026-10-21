Rusty7800 0.1.0 — Atari 7800 emulator (desktop app)
=====================================================

QUICK START
  1. Double-click rusty7800-gui.exe.
  2. Drag a ROM file (.a78 or .bin) onto the window — or launch with:
       rusty7800-gui.exe --rom game.a78
  3. Press Run.

No installation, no BIOS file, and no ROMs are included: bring your
own legally-dumped .a78/.bin images.

CONTROLS
  Player 1: arrow keys to move, Z = button A, X = button B.
  Buttons:  Run/Pause, Step 1f (advance one frame), Reset.
  Scale:    1x-4x integer display scaling.
  Machine:  NTSC/PAL, Warm/Cool/Hot palettes, Developer Mode.

REGION TIP
  If the status bar shows a red REGION MISMATCH (e.g. a PAL ROM on an
  NTSC machine), switch Machine to the named driver — mismatched games
  hang instead of booting. The bar also reports the ROM's TV region.

COMMAND LINE
  rusty7800-gui.exe [--rom file.a78] [--machine driver] [--smoke]
  --smoke runs a headless self-test (no window) and prints the hash.

MACHINES
  a7800 a7800u1 a7800u2 a7800dev a7800p a7800pu1 a7800pu2 a7800pdev

Built from https://github.com/LAGameStudio/rusty7800 (BSD-3-Clause).
