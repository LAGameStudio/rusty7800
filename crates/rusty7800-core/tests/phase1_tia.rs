//! Phase 1 acceptance: TIA audio registers, clocking, and output.

use rusty7800_core::tia::{
    TiaAudio, AUDC_POLY5_DIV3, AUDC_PURE, AUDC_SET_TO_1, AUDC0, AUDC1, AUDF0, AUDF1, AUDV0, AUDV1,
};

#[test]
fn register_masking() {
    let mut t = TiaAudio::new(31_400, 31_400);
    t.write(AUDC0, 0xF0); // masked to 0x00 = volume-only
    t.write(AUDF0, 0xFF); // masked to 0x1F
    t.write(AUDV0, 0xFF); // masked to 0x0F
    // Volume-only mode pins output to volume: 15 << 10.
    assert_eq!(t.outvol(0), 15 << 10);
    // Unknown offsets ignored.
    t.write(0x00, 0xFF);
    assert_eq!(t.outvol(1), 0);
    let _ = (AUDC1, AUDF1, AUDV1, AUDC_SET_TO_1);
}

#[test]
fn volume_only_output_is_constant() {
    let mut t = TiaAudio::new(31_400, 31_400);
    t.write(AUDC0, AUDC_SET_TO_1);
    t.write(AUDV0, 8);
    let mut buf = [0i16; 64];
    t.render(&mut buf);
    assert!(buf.iter().all(|&s| s == 8 << 10));
}

#[test]
fn divide_by_n_and_div3_rules() {
    let mut t = TiaAudio::new(31_400, 31_400);
    t.write(AUDF0, 4);
    t.write(AUDC0, AUDC_PURE);
    assert_eq!(t.div_n_max(0), 5);
    t.write(AUDC0, 0x0C); // DIV3_PURE: x3
    assert_eq!(t.div_n_max(0), 15);
    t.write(AUDC0, AUDC_POLY5_DIV3); // exception: no x3
    assert_eq!(t.div_n_max(0), 5);
}

#[test]
fn pure_tone_toggles_output() {
    // Driver rate == sample rate: one sample per TIA clock.
    let mut t = TiaAudio::new(31_400, 31_400);
    t.write(AUDF0, 0); // div-n = 1: toggle every clock
    t.write(AUDC0, AUDC_PURE);
    t.write(AUDV0, 15);
    let mut buf = [0i16; 8];
    t.render(&mut buf);
    assert!(buf.iter().all(|&s| s == 0 || s == 15 << 10));
    assert!(buf.contains(&0));
    assert!(buf.contains(&(15 << 10)));
}

#[test]
fn poly_tables_initialized() {
    let t = TiaAudio::new(31_400, 31_400);
    assert_eq!(t.poly4().len(), 15);
    assert_eq!(t.poly5().len(), 31);
    assert_eq!(t.poly9().len(), 511);
    for table in [t.poly4(), t.poly5(), t.poly9()] {
        assert!(table.contains(&0) && table.contains(&1));
    }
    // LFSR determinism spot check: poly5 starts 1,1 (x=31: bit0=1,bit1=1).
    assert_eq!(t.poly5()[0], 1);
}

#[test]
fn silence_by_default() {
    let mut t = TiaAudio::new(31_400, 31_400);
    let mut buf = [0i16; 32];
    t.render(&mut buf);
    assert!(buf.iter().all(|&s| s == 0));
}
