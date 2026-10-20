//! Rusty7800 `maria` — Atari MARIA graphics chip.
//!
//! Phase 2 port of `a7800-5.2/src/mame/video/maria.{h,cpp}` (BSD-3-Clause).
//! Covers: register map, DLL/DL walker, direct + indirect modes, all pixel
//! modes (160A/B, 320A/B/C/D, Kangaroo), holey DMA holes, DMA cycle counting
//! with hole penalties and last-line/DLI penalties, DMALIMIT halt, dev-mode
//! DMA overlay, VBLANK/WSYNC, DLL-end NMI.
//!
//! CPU interaction is machine-side: `draw_scanline` returns the DMA cycle
//! count (machine stalls SALLY `(cycles+3)/4`, like MAME's
//! `spin_until_time`); a limit hit returns `halt=true` (machine spins the
//! CPU to HSYNC and latches WSYNC, like MAME's `spin_until_trigger`).
//! `interrupt` returns whether an HSYNC trigger fired. `startdma` returns
//! whether an NMI pulse was issued.
//!
//! Addressing notes: MAME `READ_MEM(x)` yields 0 for `x < 0x20`, else a
//! program-space byte. The program space is 16-bit, so DMA addresses that
//! overflow (e.g. `graph_adr + x`, `dl + n`, `data_addr`) wrap at 64 KiB.

/// DMA time budget per scanline (maria.cpp DMALIMIT).
pub const DMA_LIMIT: i32 = 430;
/// Startup cost of every DMA line (maria.cpp: `maria_cycles = 16`).
pub const DMA_STARTUP: i32 = 16;
/// Shutdown penalty on a zone's last line (`m_offset == 0`).
pub const DMA_SHUTDOWN: i32 = 8;
/// Extra penalty when the DLL entry requests an interrupt (byte 3 & 0x80).
pub const DMA_INTERRUPT_OVERHEAD: i32 = 17;
/// Line-RAM width in low-res pixels.
pub const LINE_W: usize = 160;

/// DMA source: program-space bytes (machine wires the real bus).
pub trait MariaBus {
    fn read(&mut self, addr: u16) -> u8;
}

/// Per-pixel output: two half-pixels (320-wide framebuffer entries).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelOut {
    pub left: u8,
    pub right: u8,
    /// True when the dev-mode DMA overlay supplied the color.
    pub dev_overlay: bool,
}

/// MARIA state.
///
/// NOTE: serde is implemented manually at the bottom of this file:
/// `LINE_W`-wide line RAM exceeds serde's fixed-array impls, so the
/// image below mirrors every field with the buffers flattened.
#[derive(Clone, Debug)]
pub struct Maria {
    /// Color RAM: 32 pens (indices into the 256-entry system palette).
    pub palette: [u8; 32],
    line_ram: [[u8; LINE_W]; 2],
    active_buffer: usize,
    write_mode: bool,
    dll: u16,
    dl: u16,
    holey: u8,
    offset: u8,
    vblank: u8,
    dmaon: bool,
    dpp: u16,
    wsync: bool,
    color_kill: bool,
    cwidth: bool,
    bcntl: bool,
    kangaroo: bool,
    rm: u8,
    nmi: bool,
    charbase: u16,
    /// Developer-mode DMA usage overlay (a7800dev/a7800pdev).
    pub devmode_flag: bool,
    devmode_dmacolor: u8,
    devmode_dmacolorwidth: usize,
    /// DMA cycles consumed by the most recent `draw_scanline`.
    last_dma_cycles: i32,
    /// True when the most recent line hit DMALIMIT.
    limit_hit: bool,
}

impl Default for Maria {
    fn default() -> Self {
        Self::new()
    }
}

impl Maria {
    pub fn new() -> Self {
        let mut m = Maria {
            palette: [0; 32],
            line_ram: [[0; LINE_W]; 2],
            active_buffer: 0,
            write_mode: false,
            dll: 0,
            dl: 0,
            holey: 0,
            offset: 0,
            vblank: 0x80,
            dmaon: false,
            dpp: 0,
            wsync: false,
            color_kill: false,
            cwidth: false,
            bcntl: false,
            kangaroo: false,
            rm: 0,
            nmi: false,
            charbase: 0,
            devmode_flag: false,
            devmode_dmacolor: 0,
            devmode_dmacolorwidth: 0,
            last_dma_cycles: 0,
            limit_hit: false,
        };
        m.reset();
        m
    }

    /// device_reset().
    pub fn reset(&mut self) {
        self.palette = [0; 32];
        self.line_ram = [[0; LINE_W]; 2];
        self.active_buffer = 0;
        self.write_mode = false;
        self.dmaon = false;
        self.vblank = 0x80;
        self.dll = 0;
        self.wsync = false;
        self.color_kill = false;
        self.cwidth = false;
        self.bcntl = false;
        self.kangaroo = false;
        self.rm = 0;
        self.dl = 0;
        self.holey = 0;
        self.offset = 0;
        self.dpp = 0;
        self.nmi = false;
        self.charbase = 0;
        self.devmode_dmacolor = 0;
        self.devmode_dmacolorwidth = 0;
        self.last_dma_cycles = 0;
        self.limit_hit = false;
    }

    pub fn dmaon(&self) -> bool {
        self.dmaon
    }

    pub fn wsync_latched(&self) -> bool {
        self.wsync
    }

    pub fn dma_cycles(&self) -> i32 {
        self.last_dma_cycles
    }

    pub fn dma_limit_hit(&self) -> bool {
        self.limit_hit
    }

    pub fn vblank(&self) -> u8 {
        self.vblank
    }

    pub fn dev_overlay(&self) -> (u8, usize) {
        (self.devmode_dmacolor, self.devmode_dmacolorwidth)
    }

    /// DLL/DL/offset state (also feeds the agent socket state view).
    pub fn dll(&self) -> u16 {
        self.dll
    }

    pub fn dl(&self) -> u16 {
        self.dl
    }

    pub fn zone_offset(&self) -> u8 {
        self.offset
    }

    /// Blank the displayed (standby) line buffer: MAME draws blank
    /// scanlines when DMA is off (`maria.cpp`), so the machine calls this
    /// for raster lines outside the display window — vblank/overscan rows
    /// read background instead of stale DMA output.
    pub fn blank_displayed_line(&mut self) {
        self.line_ram[1 - self.active_buffer] = [0; LINE_W];
    }

    /// The just-rendered working (active) line buffer (160 pens).
    pub fn working_buffer(&self) -> &[u8; LINE_W] {
        &self.line_ram[self.active_buffer]
    }

    /// MAME READ_MEM: addresses below 0x20 read as 0 (TIA shadow guard).
    fn read_mem<B: MariaBus>(bus: &mut B, addr: u32) -> u8 {
        if addr < 0x20 {
            0
        } else {
            bus.read((addr & 0xFFFF) as u16)
        }
    }

    /// Register read; `offset` is the bus offset within 0x20–0x3F.
    pub fn read(&self, offset: u8) -> u8 {
        match offset & 0x1F {
            0x08 => self.vblank, // MSTAT
            _ => 0x00,           // undefined reads return 0x00 (see maria.cpp)
        }
    }

    /// Register write.
    pub fn write(&mut self, offset: u8, data: u8) {
        let off = offset & 0x1F;
        if (off & 3) != 0 {
            self.palette[off as usize] = data;
        }
        match off {
            0x00 => {
                // BACKGRND: all color-RAM entries with 00 LSBs = background.
                for i in 0..8 {
                    self.palette[4 * i] = data;
                }
            }
            0x04 => {
                // WSYNC: machine halts the CPU to HSYNC.
                self.wsync = true;
            }
            0x0C => {
                // DPPH
                self.dpp = (self.dpp & 0x00FF) | ((data as u16) << 8);
            }
            0x10 => {
                // DPPL
                self.dpp = (self.dpp & 0xFF00) | data as u16;
            }
            0x14 => {
                // CHARBASE
                self.charbase = (data as u16) << 8;
            }
            0x1C => {
                // CTRL
                self.color_kill = data & 0x80 != 0;
                match (data >> 5) & 3 {
                    0x02 => self.dmaon = true,
                    0x03 => self.dmaon = false,
                    _ => {} // DMA test modes: unusable, ignore (MAME logs)
                }
                self.cwidth = data & 0x10 != 0;
                self.bcntl = data & 0x08 != 0; // border: accepted, not displayed
                self.kangaroo = data & 0x04 != 0;
                self.rm = data & 0x03;
            }
            _ => {}
        }
    }

    /// Hole test (is_holey): bit1 ↔ 0x9000-masked holes, bit0 ↔ 0x8800.
    pub fn is_holey(&self, addr: u32) -> bool {
        ((self.holey & 0x02) != 0 && (addr & 0x9000) == 0x9000)
            || ((self.holey & 0x01) != 0 && (addr & 0x8800) == 0x8800)
    }

    /// Write one graphics byte into the working line buffer.
    /// Returns cells advanced (2 in 160B-ish write mode, else 4).
    /// `hpos`/`pal` match the C++ call-site semantics.
    pub fn write_line_ram<B: MariaBus>(
        &mut self,
        bus: &mut B,
        addr: u32,
        hpos: u8,
        pal: u8,
    ) -> u8 {
        let data = Self::read_mem(bus, addr);
        let palb = pal << 2;
        let mut off = hpos as usize;
        let buf = self.active_buffer;
        if self.write_mode {
            // 160B-ish: two pixels per byte, odd bit packing.
            let mut c = (palb & 0x10) | (data & 0x0C) | (data >> 6);
            if ((c & 3) != 0 || self.kangaroo) && off < LINE_W {
                self.line_ram[buf][off] = c;
            }
            off += 1;
            c = (palb & 0x10) | ((data & 0x03) << 2) | ((data & 0x30) >> 4);
            if ((c & 3) != 0 || self.kangaroo) && off < LINE_W {
                self.line_ram[buf][off] = c;
            }
            2
        } else {
            for i in 0..4 {
                let c = palb | ((data >> (6 - 2 * i)) & 0x03);
                if ((c & 3) != 0 || self.kangaroo) && off < LINE_W {
                    self.line_ram[buf][off] = c;
                }
                off += 1;
            }
            4
        }
    }

    /// DMA + render one scanline into the working buffer.
    /// Returns `(dma_cycles, halt_until_hsync)`.
    pub fn draw_scanline<B: MariaBus>(&mut self, bus: &mut B) -> (i32, bool) {
        for cell in self.line_ram[self.active_buffer].iter_mut() {
            *cell = 0;
        }
        self.last_dma_cycles = 0;
        self.limit_hit = false;
        if !self.dmaon {
            return (0, false);
        }

        let mut cycles = DMA_STARTUP;
        let mut dl = self.dl as u32;

        // End-of-list gate: a header byte1 with `(b & 0x5F) == 0` ends the
        // zone. Consequence (matches MAME/hardware): extended headers must
        // carry bit 6 (e.g. 0x60/0xE0), and width-32 normal headers need a
        // palette with bit 1 set (e.g. pal 2/3/6/7); a 0x00 byte always ends.
        while (Self::read_mem(bus, dl + 1) & 0x5F) != 0 && cycles < DMA_LIMIT {
            // Header fetch.
            let (graph_adr, width, mut hpos, pal, ind);
            if Self::read_mem(bus, dl + 1) & 0x1F == 0 {
                // Extended header (5 bytes).
                graph_adr = ((Self::read_mem(bus, dl + 2) as u32) << 8)
                    | Self::read_mem(bus, dl) as u32;
                width = ((Self::read_mem(bus, dl + 3) ^ 0xFF) & 0x1F) + 1;
                hpos = Self::read_mem(bus, dl + 4);
                pal = Self::read_mem(bus, dl + 3) >> 5;
                self.write_mode = Self::read_mem(bus, dl + 1) & 0x80 != 0;
                ind = Self::read_mem(bus, dl + 1) & 0x20 != 0;
                dl += 5;
                cycles += 10;
            } else {
                // Normal header (4 bytes).
                graph_adr = ((Self::read_mem(bus, dl + 2) as u32) << 8)
                    | Self::read_mem(bus, dl) as u32;
                width = ((Self::read_mem(bus, dl + 1) ^ 0xFF) & 0x1F) + 1;
                hpos = Self::read_mem(bus, dl + 3);
                pal = Self::read_mem(bus, dl + 1) >> 5;
                ind = false;
                dl += 4;
                cycles += 8;
            }

            let mut hole_known = false;
            for x in 0..width {
                if cycles >= DMA_LIMIT {
                    break;
                }
                if ind {
                    let c = Self::read_mem(bus, graph_adr + x as u32);
                    let data_addr = (self.charbase as u32 | c as u32)
                        + ((self.offset as u32) << 8);
                    if self.is_holey(data_addr) {
                        if !hole_known {
                            cycles += 3;
                            hole_known = true;
                        }
                        continue;
                    }
                    cycles += 3;
                    if self.cwidth {
                        let cells = self.write_line_ram(bus, data_addr, hpos, pal);
                        hpos = hpos.wrapping_add(cells);
                        let cells =
                            self.write_line_ram(bus, data_addr + 1, hpos, pal);
                        hpos = hpos.wrapping_add(cells);
                        cycles += 6;
                    } else {
                        let cells = self.write_line_ram(bus, data_addr, hpos, pal);
                        hpos = hpos.wrapping_add(cells);
                        cycles += 3;
                    }
                } else {
                    let data_addr =
                        graph_adr + x as u32 + ((self.offset as u32) << 8);
                    if self.is_holey(data_addr) {
                        if !hole_known {
                            cycles += 3;
                            hole_known = true;
                        }
                        continue;
                    }
                    let cells = self.write_line_ram(bus, data_addr, hpos, pal);
                    hpos = hpos.wrapping_add(cells);
                    cycles += 3;
                }
            }

            // Last-line post-render penalties.
            if self.offset == 0 {
                cycles += DMA_SHUTDOWN;
                if Self::read_mem(bus, self.dll as u32 + 3) & 0x80 != 0 {
                    cycles += DMA_INTERRUPT_OVERHEAD;
                }
            }
        }

        if self.devmode_flag {
            if cycles >= DMA_LIMIT {
                self.devmode_dmacolor = 0x1F;
                self.devmode_dmacolorwidth = 8;
            } else {
                let mut col = (cycles * 0x0F) / 428;
                if col > 0x0F {
                    col = 0x0F;
                }
                self.devmode_dmacolor = (col as u8) | 0x40;
                self.devmode_dmacolorwidth = 4;
            }
        }

        self.last_dma_cycles = cycles;
        if cycles >= DMA_LIMIT {
            // CPU can't run until next scanline (MAME spins to HSYNC + WSYNC).
            self.limit_hit = true;
            self.wsync = true;
            return (cycles, true);
        }
        (cycles, false)
    }

    /// Render one low-res cell to two half-pixels (driver calls per pixelx).
    /// Reads the standby buffer live, so mid-scanline palette writes show
    /// mid-scanline — as on hardware and in MAME.
    pub fn render_pixel(&self, pixelx: usize) -> PixelOut {
        if self.devmode_flag && pixelx < self.devmode_dmacolorwidth {
            return PixelOut {
                left: self.devmode_dmacolor,
                right: self.devmode_dmacolor,
                dev_overlay: true,
            };
        }
        let standby = self.line_ram[1 - self.active_buffer][pixelx];
        let (mut left, mut right) = match self.rm {
            // 160A/B: doubled cell.
            0x00 | 0x01 => {
                let pen = self.palette[(standby & 0x1F) as usize];
                (pen, pen)
            }
            // 320B/D: b4 b1 b3 / b4 b0 b2.
            0x02 => {
                let d0 = (standby & 0x10) | (standby & 0x02) | ((standby >> 3) & 1);
                let d1 =
                    (standby & 0x10) | ((standby << 1) & 0x02) | ((standby >> 2) & 1);
                (
                    self.palette[(d0 & 0x1F) as usize],
                    self.palette[(d1 & 0x1F) as usize],
                )
            }
            // 320A/C: b4 b3 b2 b1 / b4 b3 b2 b0.
            _ => {
                let d0 = (standby & 0x1C) | (standby & 0x02);
                let d1 = (standby & 0x1C) | ((standby << 1) & 0x02);
                (
                    self.palette[(d0 & 0x1F) as usize],
                    self.palette[(d1 & 0x1F) as usize],
                )
            }
        };
        if self.color_kill {
            left &= 0x0F;
            right &= 0x0F;
        }
        PixelOut {
            left,
            right,
            dev_overlay: false,
        }
    }

    /// Per-scanline housekeeping (driver `interrupt` timer callback).
    /// Returns true when a pending WSYNC releases an HSYNC trigger.
    /// `vpos` is the current raster position; `lines` the machine line count.
    pub fn interrupt(&mut self, vpos: u16, lines: u16) -> bool {
        let fired = self.wsync;
        self.wsync = false;
        let frame = vpos % (lines + 1);
        if frame == 16 {
            self.vblank = 0x00;
        }
        if frame == lines - 5 {
            self.vblank = 0x80;
        }
        fired
    }

    /// Start-of-scanline DMA (driver `maria_startdma`).
    /// Returns `(dma_cycles, halt_until_hsync, nmi_pulsed)`.
    pub fn startdma<B: MariaBus>(
        &mut self,
        bus: &mut B,
        vpos: u16,
        lines: u16,
    ) -> (i32, bool, bool) {
        self.active_buffer = 1 - self.active_buffer;
        let frame = vpos % (lines + 1);

        if frame == 16 && self.dmaon {
            // End of vblank: latch the display list (DLL changes during
            // vblank only).
            self.dll = self.dpp;
            self.dl = ((Self::read_mem(bus, self.dll as u32 + 1) as u16) << 8)
                | Self::read_mem(bus, self.dll as u32 + 2) as u16;
            let b0 = Self::read_mem(bus, self.dll as u32);
            self.offset = b0 & 0x0F;
            self.holey = (b0 & 0x60) >> 5;
            self.nmi = b0 & 0x80 != 0;
        }

        let mut cycles = 0;
        let mut halt = false;
        if frame > 15 && frame < lines - 5 {
            (cycles, halt) = self.draw_scanline(bus);
        }

        if frame > 16 && frame < lines - 5 && self.dmaon {
            if self.offset == 0 {
                self.dll = self.dll.wrapping_add(3);
                self.dl = ((Self::read_mem(bus, self.dll as u32 + 1) as u16) << 8)
                    | Self::read_mem(bus, self.dll as u32 + 2) as u16;
                let b0 = Self::read_mem(bus, self.dll as u32);
                self.offset = b0 & 0x0F;
                self.holey = (b0 & 0x60) >> 5;
                self.nmi = b0 & 0x80 != 0;
            } else {
                self.offset -= 1;
            }
        }

        let nmi = self.nmi;
        self.nmi = false;
        (cycles, halt, nmi)
    }
}

/// Save-state image: every field, with line RAM flattened (row-major,
/// buffer 0 then buffer 1) because `[u8; 160]` is beyond serde's
/// fixed-array impls.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MariaState {
    pub palette: Vec<u8>,
    pub line_ram: Vec<u8>,
    pub active_buffer: usize,
    pub write_mode: bool,
    pub dll: u16,
    pub dl: u16,
    pub holey: u8,
    pub offset: u8,
    pub vblank: u8,
    pub dmaon: bool,
    pub dpp: u16,
    pub wsync: bool,
    pub color_kill: bool,
    pub cwidth: bool,
    pub bcntl: bool,
    pub kangaroo: bool,
    pub rm: u8,
    pub nmi: bool,
    pub charbase: u16,
    pub devmode_flag: bool,
    pub devmode_dmacolor: u8,
    pub devmode_dmacolorwidth: usize,
    pub last_dma_cycles: i32,
    pub limit_hit: bool,
}

impl Maria {
    /// Capture full state (exact resume, mid-line safe).
    pub fn save(&self) -> MariaState {
        let mut line_ram = Vec::with_capacity(2 * LINE_W);
        line_ram.extend_from_slice(&self.line_ram[0]);
        line_ram.extend_from_slice(&self.line_ram[1]);
        MariaState {
            palette: self.palette.to_vec(),
            line_ram,
            active_buffer: self.active_buffer,
            write_mode: self.write_mode,
            dll: self.dll,
            dl: self.dl,
            holey: self.holey,
            offset: self.offset,
            vblank: self.vblank,
            dmaon: self.dmaon,
            dpp: self.dpp,
            wsync: self.wsync,
            color_kill: self.color_kill,
            cwidth: self.cwidth,
            bcntl: self.bcntl,
            kangaroo: self.kangaroo,
            rm: self.rm,
            nmi: self.nmi,
            charbase: self.charbase,
            devmode_flag: self.devmode_flag,
            devmode_dmacolor: self.devmode_dmacolor,
            devmode_dmacolorwidth: self.devmode_dmacolorwidth,
            last_dma_cycles: self.last_dma_cycles,
            limit_hit: self.limit_hit,
        }
    }

    /// Restore full state; malformed buffer lengths are ignored.
    pub fn load(&mut self, s: &MariaState) {
        if s.palette.len() == 32 {
            self.palette.copy_from_slice(&s.palette);
        }
        if s.line_ram.len() == 2 * LINE_W {
            self.line_ram[0].copy_from_slice(&s.line_ram[..LINE_W]);
            self.line_ram[1].copy_from_slice(&s.line_ram[LINE_W..]);
        }
        self.active_buffer = s.active_buffer & 1;
        self.write_mode = s.write_mode;
        self.dll = s.dll;
        self.dl = s.dl;
        self.holey = s.holey;
        self.offset = s.offset;
        self.vblank = s.vblank;
        self.dmaon = s.dmaon;
        self.dpp = s.dpp;
        self.wsync = s.wsync;
        self.color_kill = s.color_kill;
        self.cwidth = s.cwidth;
        self.bcntl = s.bcntl;
        self.kangaroo = s.kangaroo;
        self.rm = s.rm;
        self.nmi = s.nmi;
        self.charbase = s.charbase;
        self.devmode_flag = s.devmode_flag;
        self.devmode_dmacolor = s.devmode_dmacolor;
        self.devmode_dmacolorwidth = s.devmode_dmacolorwidth;
        self.last_dma_cycles = s.last_dma_cycles;
        self.limit_hit = s.limit_hit;
    }
}
