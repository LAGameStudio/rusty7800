//! Rusty7800 `tia` — TIA audio (2 channels + noise).
//!
//! Phase 1: audio synthesis + AUDC/AUDF/AUDV registers. (TIA *read*
//! logic — paddles, lightguns, INPT mux — lands in Phase 3 with the
//! driver `tia_r` wrapper.)
//!
//! Behavior reference: `a7800-5.2/src/devices/sound/tiasound.*` (Ron Fries'
//! TIA sound core, used by the a7800 driver as `MCFG_SOUND_TIA_ADD("tia",
//! 31400)` mono). Provenance note: that file is GPL-2.0+, so this module
//! is an independent implementation of the *documented* TIA audio behavior
//! (AUDC mode table, polynomial counters, divide-by-N clocking, sample-rate
//! conversion) written for the BSD-licensed core. Bit-exact parity versus
//! the C++ is validated by ear + RMS comparison in Phase 9.
//!
//! Register offsets (tiaintf: 0x15–0x1A):
pub const AUDC0: u8 = 0x15;
pub const AUDC1: u8 = 0x16;
pub const AUDF0: u8 = 0x17;
pub const AUDF1: u8 = 0x18;
pub const AUDV0: u8 = 0x19;
pub const AUDV1: u8 = 0x1A;

/// Volume shift: AUDV nibble << 10 (AUDV_SHIFT in the reference).
pub const AUDV_SHIFT: u32 = 10;

/// AUDC mode codes.
pub const AUDC_SET_TO_1: u8 = 0x00;
pub const AUDC_POLY4: u8 = 0x01;
pub const AUDC_DIV31_POLY4: u8 = 0x02;
pub const AUDC_POLY5_POLY4: u8 = 0x03;
pub const AUDC_PURE: u8 = 0x04;
pub const AUDC_PURE2: u8 = 0x05;
pub const AUDC_DIV31_PURE: u8 = 0x06;
pub const AUDC_POLY5_2: u8 = 0x07;
pub const AUDC_POLY9: u8 = 0x08;
pub const AUDC_POLY5: u8 = 0x09;
pub const AUDC_DIV31_POLY5: u8 = 0x0A;
pub const AUDC_POLY5_POLY5: u8 = 0x0B;
pub const AUDC_DIV3_PURE: u8 = 0x0C;
pub const AUDC_DIV3_PURE2: u8 = 0x0D;
pub const AUDC_DIV93_PURE: u8 = 0x0E;
pub const AUDC_POLY5_DIV3: u8 = 0x0F;

const POLY4_SIZE: usize = 0x0F;
const POLY5_SIZE: usize = 0x1F;
const POLY9_SIZE: usize = 0x1FF;

/// Divide-by-31 pattern (13:18 duty, not 50%).
const DIV31: [u8; POLY5_SIZE] = [
    0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

fn poly_init<const N: usize>(size: usize, f0: usize, f1: usize) -> [u8; N] {
    let mut poly = [0u8; N];
    let mask: i32 = (1 << size) - 1;
    let mut x = mask;
    for item in poly.iter_mut().take(mask as usize) {
        let bit0 = (if size == f0 { x } else { x >> (size - f0) }) & 1;
        let bit1 = (if size == f1 { x } else { x >> (size - f1) }) & 1;
        *item = (x & 1) as u8;
        x = (x >> 1) | ((bit0 ^ bit1) << (size - 1));
    }
    poly
}

/// Two-channel TIA audio state.
#[derive(Clone, Debug)]
pub struct TiaAudio {
    audc: [u8; 2],
    audf: [u8; 2],
    audv: [i16; 2],
    outvol: [i16; 2],
    bit4: [u8; POLY4_SIZE],
    bit5: [u8; POLY5_SIZE],
    bit9: [u8; POLY9_SIZE],
    p4: [u8; 2],
    p5: [u8; 2],
    p9: [u16; 2],
    div_n_cnt: [u8; 2],
    div_n_max: [u8; 2],
    div_3_cnt: [u8; 2],
    samp_n_max: u16,
    samp_n_cnt: u16,
    oversampling: bool,
}

impl TiaAudio {
    /// `clock`: TIA clock (driver uses 31400); `sample_rate`: output rate.
    pub fn new(clock: u32, sample_rate: u32) -> Self {
        let mut samp_n_max = (((clock as u64) << 8) / sample_rate as u64) as u16;
        let mut samp_n_cnt = samp_n_max;
        let mut oversampling = false;
        if samp_n_max < 256 {
            samp_n_max = (((sample_rate as u64) << 8) / clock as u64) as u16;
            samp_n_cnt = samp_n_max;
            oversampling = true;
        }
        TiaAudio {
            audc: [0; 2],
            audf: [0; 2],
            audv: [0; 2],
            outvol: [0; 2],
            bit4: poly_init(4, 4, 3),
            bit5: poly_init(5, 5, 3),
            bit9: poly_init(9, 9, 5),
            p4: [0; 2],
            p5: [0; 2],
            p9: [0; 2],
            div_n_cnt: [0; 2],
            div_n_max: [0; 2],
            div_3_cnt: [3; 2],
            samp_n_max,
            samp_n_cnt,
            oversampling,
        }
    }

    pub fn div_n_max(&self, chan: usize) -> u8 {
        self.div_n_max[chan]
    }

    pub fn outvol(&self, chan: usize) -> i16 {
        self.outvol[chan]
    }

    pub fn poly4(&self) -> &[u8] {
        &self.bit4
    }

    pub fn poly5(&self) -> &[u8] {
        &self.bit5
    }

    pub fn poly9(&self) -> &[u8] {
        &self.bit9
    }

    /// Register write (offsets AUDC0..AUDV1).
    pub fn write(&mut self, offset: u8, data: u8) {
        let chan = match offset {
            AUDC0 => {
                self.audc[0] = data & 0x0f;
                Some(0)
            }
            AUDC1 => {
                self.audc[1] = data & 0x0f;
                Some(1)
            }
            AUDF0 => {
                self.audf[0] = data & 0x1f;
                Some(0)
            }
            AUDF1 => {
                self.audf[1] = data & 0x1f;
                Some(1)
            }
            AUDV0 => {
                self.audv[0] = ((data & 0x0f) as i16) << AUDV_SHIFT;
                Some(0)
            }
            AUDV1 => {
                self.audv[1] = ((data & 0x0f) as i16) << AUDV_SHIFT;
                Some(1)
            }
            _ => None,
        };
        if let Some(ch) = chan {
            let new_val = if self.audc[ch] == AUDC_SET_TO_1 || self.audc[ch] == AUDC_POLY5_POLY5 {
                // Volume-only: no clocking, output pinned to volume.
                self.outvol[ch] = self.audv[ch];
                0
            } else {
                let mut n = self.audf[ch] + 1;
                if (self.audc[ch] & 0x0c) == 0x0c && self.audc[ch] != AUDC_POLY5_DIV3 {
                    n = n.saturating_mul(3);
                }
                n
            };
            if new_val != self.div_n_max[ch] {
                self.div_n_max[ch] = new_val;
                if self.div_n_cnt[ch] == 0 || new_val == 0 {
                    self.div_n_cnt[ch] = new_val;
                }
            }
        }
    }

    fn clock_channel(&mut self, ch: usize) {
        if self.div_n_cnt[ch] > 1 {
            self.div_n_cnt[ch] -= 1;
            return;
        }
        if self.div_n_cnt[ch] != 1 {
            return;
        }
        let prev_bit5 = self.bit5[self.p5[ch] as usize];
        self.div_n_cnt[ch] = self.div_n_max[ch];
        self.p5[ch] += 1;
        if self.p5[ch] as usize == POLY5_SIZE {
            self.p5[ch] = 0;
        }
        let audc = self.audc[ch];
        let audv = self.audv[ch];
        let p5 = self.p5[ch] as usize;
        let tick = (audc & 0x02) == 0
            || ((audc & 0x01) == 0 && DIV31[p5] != 0)
            || ((audc & 0x01) == 1 && self.bit5[p5] != 0)
            || (audc == AUDC_POLY5_DIV3 && self.bit5[p5] != prev_bit5);
        if !tick {
            return;
        }
        if (audc & 0x04) != 0 {
            // Pure (square-wave) family.
            if audc == AUDC_POLY5_DIV3 {
                if self.bit5[p5] != prev_bit5 {
                    self.div_3_cnt[ch] -= 1;
                    if self.div_3_cnt[ch] == 0 {
                        self.div_3_cnt[ch] = 3;
                        self.outvol[ch] = if self.outvol[ch] != 0 { 0 } else { audv };
                    }
                }
            } else if self.outvol[ch] != 0 {
                self.outvol[ch] = 0;
            } else {
                self.outvol[ch] = audv;
            }
        } else if (audc & 0x08) != 0 {
            if audc == AUDC_POLY9 {
                self.p9[ch] += 1;
                if self.p9[ch] as usize == POLY9_SIZE {
                    self.p9[ch] = 0;
                }
                self.outvol[ch] = if self.bit9[self.p9[ch] as usize] != 0 { audv } else { 0 };
            } else if (audc & 0x02) != 0 {
                if self.outvol[ch] != 0 || (audc & 0x01) != 0 {
                    self.outvol[ch] = 0;
                } else {
                    self.outvol[ch] = audv;
                }
            } else if self.bit5[p5] != 0 {
                self.outvol[ch] = audv;
            } else {
                self.outvol[ch] = 0;
            }
        } else {
            self.p4[ch] += 1;
            if self.p4[ch] as usize == POLY4_SIZE {
                self.p4[ch] = 0;
            }
            self.outvol[ch] = if self.bit4[self.p4[ch] as usize] != 0 { audv } else { 0 };
        }
    }

    /// Render `out.len()` mono samples (sum of both channels).
    pub fn render(&mut self, out: &mut [i16]) {
        let mut i = 0;
        while i < out.len() {
            self.clock_channel(0);
            self.clock_channel(1);
            if !self.oversampling {
                self.samp_n_cnt = self.samp_n_cnt.wrapping_sub(256);
                if self.samp_n_cnt < 256 {
                    self.samp_n_cnt = self.samp_n_cnt.wrapping_add(self.samp_n_max);
                    out[i] = self.outvol[0].wrapping_add(self.outvol[1]);
                    i += 1;
                }
            } else {
                loop {
                    self.samp_n_cnt = self.samp_n_cnt.wrapping_sub(256);
                    out[i] = self.outvol[0].wrapping_add(self.outvol[1]);
                    i += 1;
                    if self.samp_n_cnt < 256 || i >= out.len() {
                        break;
                    }
                }
                if self.samp_n_cnt < 256 {
                    self.samp_n_cnt = self.samp_n_cnt.wrapping_add(self.samp_n_max);
                }
            }
        }
    }
}

/// Save-state image: full channel + polynomial state. Poly tables are
/// `Vec`s because `[u8; 511]` is beyond serde's fixed-array impls
/// (they are deterministic from power-on, but mid-stream positions
/// `p4/p5/p9` only resume exactly against identical tables).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TiaState {
    pub audc: [u8; 2],
    pub audf: [u8; 2],
    pub audv: [i16; 2],
    pub outvol: [i16; 2],
    pub bit4: Vec<u8>,
    pub bit5: Vec<u8>,
    pub bit9: Vec<u8>,
    pub p4: [u8; 2],
    pub p5: [u8; 2],
    pub p9: [u16; 2],
    pub div_n_cnt: [u8; 2],
    pub div_n_max: [u8; 2],
    pub div_3_cnt: [u8; 2],
    pub samp_n_max: u16,
    pub samp_n_cnt: u16,
    pub oversampling: bool,
}

impl TiaAudio {
    /// Capture full audio state (exact resume, mid-frame safe).
    pub fn save(&self) -> TiaState {
        TiaState {
            audc: self.audc,
            audf: self.audf,
            audv: self.audv,
            outvol: self.outvol,
            bit4: self.bit4.to_vec(),
            bit5: self.bit5.to_vec(),
            bit9: self.bit9.to_vec(),
            p4: self.p4,
            p5: self.p5,
            p9: self.p9,
            div_n_cnt: self.div_n_cnt,
            div_n_max: self.div_n_max,
            div_3_cnt: self.div_3_cnt,
            samp_n_max: self.samp_n_max,
            samp_n_cnt: self.samp_n_cnt,
            oversampling: self.oversampling,
        }
    }

    /// Restore full audio state; malformed tables are ignored.
    pub fn load(&mut self, s: &TiaState) {
        self.audc = s.audc;
        self.audf = s.audf;
        self.audv = s.audv;
        self.outvol = s.outvol;
        if s.bit4.len() == POLY4_SIZE {
            self.bit4.copy_from_slice(&s.bit4);
        }
        if s.bit5.len() == POLY5_SIZE {
            self.bit5.copy_from_slice(&s.bit5);
        }
        if s.bit9.len() == POLY9_SIZE {
            self.bit9.copy_from_slice(&s.bit9);
        }
        self.p4 = s.p4;
        self.p5 = s.p5;
        self.p9 = s.p9;
        self.div_n_cnt = s.div_n_cnt;
        self.div_n_max = s.div_n_max;
        self.div_3_cnt = s.div_3_cnt;
        self.samp_n_max = s.samp_n_max;
        self.samp_n_cnt = s.samp_n_cnt;
        self.oversampling = s.oversampling;
    }
}
