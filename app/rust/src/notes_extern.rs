use byteorder::{ByteOrder, LittleEndian};

use crate::constants::COMPACT_NOTE_SIZE;
use crate::notes;
use crate::notes::{get_epk, rseed_generate_rcm, rseed_get_esk};
use crate::types::Diversifier;

#[no_mangle]
pub extern "C" fn rseed_get_esk_epk(
    rseed_ptr: *const [u8; 32],
    d_ptr: *const Diversifier,
    out_esk_ptr: *mut [u8; 32],
    out_epk_ptr: *mut [u8; 32],
) {
    crate::bolos::heartbeat();
    let rseed = unsafe { &*rseed_ptr };

    let out_esk = unsafe { &mut *out_esk_ptr };
    let out_epk = unsafe { &mut *out_epk_ptr };

    rseed_get_esk(rseed, out_esk);
    get_epk(out_esk, d_ptr, out_epk);

    crate::bolos::heartbeat();
}

#[no_mangle]
pub extern "C" fn rseed_get_rcm(rseed_ptr: *const [u8; 32], out_ptr: *mut [u8; 32]) {
    let rseed = unsafe { &*rseed_ptr };
    let out = unsafe { &mut *out_ptr };
    let p = rseed_generate_rcm(rseed);
    out.copy_from_slice(&p.to_bytes());
}

#[no_mangle]
pub extern "C" fn ka_to_key(
    esk_ptr: *const [u8; 32],
    pkd_ptr: *const [u8; 32],
    epk_ptr: *const [u8; 32],
    out_ptr: *mut [u8; 32],
) {
    let esk = unsafe { &*esk_ptr }; //ovk, cv, cmu, epk
    let pkd = unsafe { &*pkd_ptr };
    let epk = unsafe { &*epk_ptr };
    let out = unsafe { &mut *out_ptr }; //ovk, cv, cmu, epk

    let shared_secret = notes::ka_agree(esk, pkd);
    let key = notes::sapling_kdf(&shared_secret, epk);
    crate::bolos::heartbeat();

    out.copy_from_slice(&key);
}

#[no_mangle]
pub extern "C" fn prepare_enccompact_input(
    d_ptr: *const Diversifier,
    value: u64,
    rcm_ptr: *const [u8; 32],
    memotype: u8,
    output_ptr: *mut [u8; COMPACT_NOTE_SIZE + 1],
) {
    let d = unsafe { &*d_ptr };
    let rcm = unsafe { &*rcm_ptr };

    let output = unsafe { &mut *output_ptr };

    let mut input = [0; COMPACT_NOTE_SIZE + 1];
    input[0] = 2;
    input[1..12].copy_from_slice(d);

    let mut vbytes = [0u8; 8];
    LittleEndian::write_u64(&mut vbytes, value);

    input[12..20].copy_from_slice(&vbytes);
    input[20..COMPACT_NOTE_SIZE].copy_from_slice(rcm);
    input[COMPACT_NOTE_SIZE] = memotype;
    output.copy_from_slice(&input);
}
