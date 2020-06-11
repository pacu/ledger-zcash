//#![no_std]
#![no_builtins]
#![allow(dead_code, unused_imports)]

mod bolos;
mod constants;

extern crate core;

use jubjub::{AffineNielsPoint, AffinePoint, ExtendedNielsPoint, ExtendedPoint, Fq, Fr};

use blake2s_simd::{blake2s, Hash as Blake2sHash, Params as Blake2sParams};

fn debug(_msg: &str) {}

use core::convert::TryInto;
use core::mem;
#[cfg(not(test))]
use core::panic::PanicInfo;

extern crate hex;

/*#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}*/

use crypto_api_chachapoly::{ChaCha20Ietf, ChachaPolyIetf};
use subtle::ConditionallySelectable; //TODO: replace me with no-std version

const COMPACT_NOTE_SIZE: usize = (
    1  + // version
        11 + // diversifier
        8  + // value
        32
    // rcv
);
const NOTE_PLAINTEXT_SIZE: usize = COMPACT_NOTE_SIZE + 512;
const OUT_PLAINTEXT_SIZE: usize = (
    32 + // pk_d
        32
    // esk
);
const ENC_CIPHERTEXT_SIZE: usize = NOTE_PLAINTEXT_SIZE + 16;
const OUT_CIPHERTEXT_SIZE: usize = OUT_PLAINTEXT_SIZE + 16;

pub fn generate_esk(buffer: [u8; 64]) -> [u8; 32] {
    //Rng.fill_bytes(&mut buffer); fill with random bytes
    let esk = Fr::from_bytes_wide(&buffer);
    esk.to_bytes()
}

pub fn derive_public(esk: [u8; 32], g_d: [u8; 32]) -> [u8; 32] {
    let p = AffinePoint::from_bytes(g_d).unwrap();
    let q = p.to_niels().multiply_bits(&esk);
    let t = AffinePoint::from(q);
    t.to_bytes()
}

pub fn sapling_ka_agree(esk: [u8; 32], pk_d: [u8; 32]) -> [u8; 32] {
    let p = AffinePoint::from_bytes(pk_d).unwrap();
    let q = p.mul_by_cofactor();
    let v = q.to_niels().multiply_bits(&esk);
    let t = AffinePoint::from(v);
    t.to_bytes()
}

fn kdf_sapling(dhsecret: [u8; 32], epk: [u8; 32]) -> [u8; 32] {
    let mut input = [0u8; 64];
    (&mut input[..32]).copy_from_slice(&dhsecret);
    (&mut input[32..]).copy_from_slice(&epk);
    bolos::blake2b_kdf_sapling(&input)
}

fn prf_ock(ovk: [u8; 32], cv: [u8; 32], cmu: [u8; 32], epk: [u8; 32]) -> [u8; 32] {
    let mut ock_input = [0u8; 128];
    ock_input[0..32].copy_from_slice(&ovk); //Todo: compute this from secret key
    ock_input[32..64].copy_from_slice(&cv);
    ock_input[64..96].copy_from_slice(&cmu);
    ock_input[96..128].copy_from_slice(&epk);

    bolos::blake2b_prf_ock(&ock_input)
}

#[inline(never)]
fn pkd_group_hash(d: &[u8; 11]) -> [u8; 32] {
    let h = bolos::blake2s_diversification(d);

    let v = AffinePoint::from_bytes(h).unwrap();
    let q = v.mul_by_cofactor();
    let t = AffinePoint::from(q);
    t.to_bytes()
}

fn chacha_encryptnote(
    key: [u8; 32],
    plaintext: [u8; NOTE_PLAINTEXT_SIZE],
) -> [u8; ENC_CIPHERTEXT_SIZE] {
    let mut output = [0u8; ENC_CIPHERTEXT_SIZE];
    ChachaPolyIetf::aead_cipher()
        .seal_to(&mut output, &plaintext, &[], &key, &[0u8; 12])
        .unwrap();
    output
}

fn chacha_decryptnote(
    key: [u8; 32],
    ciphertext: [u8; ENC_CIPHERTEXT_SIZE],
) -> [u8; ENC_CIPHERTEXT_SIZE] {
    let mut plaintext = [0u8; ENC_CIPHERTEXT_SIZE];
    ChachaPolyIetf::aead_cipher()
        .open_to(&mut plaintext, &ciphertext, &[], &key, &[0u8; 12])
        .unwrap();
    plaintext
}

fn handle_chunk(bits: u8, cur: &mut Fr) -> Fr {
    let c = bits & 1;
    let b = bits & 2;
    let a = bits & 4;
    let mut tmp = *cur;
    if a == 4 {
        tmp = tmp.add(cur);
    }
    *cur = cur.double(); // 2^1 * cur
    if b == 2 {
        tmp = tmp.add(cur);
    }
    // conditionally negate
    if c == 1 {
        tmp = tmp.neg();
    }
    return tmp;
}

//assumption here that ceil(bitsize / 8) == m.len(), so appended with zero bits to fill the bytes
fn pedersen_hash_len(m: &[u8], bitsize: u64) -> [u8; 32] {
    let points = [
        [
            0xca, 0x3c, 0x24, 0x32, 0xd4, 0xab, 0xbf, 0x77, 0x32, 0x46, 0x4e, 0xc0, 0x8b, 0x2e,
            0x47, 0xf9, 0x5e, 0xdc, 0x7e, 0x83, 0x6b, 0x16, 0xc9, 0x79, 0x57, 0x1b, 0x52, 0xd3,
            0xa2, 0x87, 0x9e, 0xa8,
        ],
        [
            0x91, 0x18, 0xbf, 0x4e, 0x3c, 0xc5, 0x0d, 0x7b, 0xe8, 0xd3, 0xfa, 0x98, 0xeb, 0xbe,
            0x3a, 0x1f, 0x25, 0xd9, 0x01, 0xc0, 0x42, 0x11, 0x89, 0xf7, 0x33, 0xfe, 0x43, 0x5b,
            0x7f, 0x8c, 0x5d, 0x01,
        ],
        [
            0x57, 0xd4, 0x93, 0x97, 0x2c, 0x50, 0xed, 0x80, 0x98, 0xb4, 0x84, 0x17, 0x7f, 0x2a,
            0xb2, 0x8b, 0x53, 0xe8, 0x8c, 0x8e, 0x6c, 0xa4, 0x00, 0xe0, 0x9e, 0xee, 0x4e, 0xd2,
            0x00, 0x15, 0x2e, 0xb6,
        ],
        [
            0xe9, 0x70, 0x35, 0xa3, 0xec, 0x4b, 0x71, 0x84, 0x85, 0x6a, 0x1f, 0xa1, 0xa1, 0xaf,
            0x03, 0x51, 0xb7, 0x47, 0xd9, 0xd8, 0xcb, 0x0a, 0x07, 0x91, 0xd8, 0xca, 0x56, 0x4b,
            0x0c, 0xe4, 0x7e, 0x2f,
        ],
        [
            0xef, 0x8a, 0x65, 0xc3, 0x99, 0x82, 0x96, 0x99, 0x4c, 0xd1, 0x59, 0x58, 0x09, 0xd8,
            0xb9, 0xb3, 0xe5, 0xc9, 0x06, 0x14, 0x38, 0x32, 0x78, 0x39, 0x0a, 0x9d, 0xab, 0x03,
            0x21, 0xc5, 0x4b, 0xc9,
        ],
        [
            0x9a, 0x62, 0x8d, 0x9f, 0x11, 0x82, 0x60, 0x43, 0xa7, 0x13, 0x6b, 0xc6, 0xd2, 0x00,
            0x02, 0xa8, 0x28, 0x6a, 0x13, 0x0a, 0x07, 0xb1, 0xcd, 0x64, 0xe5, 0xb6, 0xbf, 0xe8,
            0x89, 0x46, 0xec, 0xe4,
        ],
    ];

    let mut i = 0;
    let mut counter: usize = 0;
    let mut pointcounter: usize = 0;
    let maxcounter: usize = 63;
    let mut remainingbits = bitsize;

    let mut x: u64 = 0;
    let mut bits: u8 = 0;

    let mut acc = Fr::zero();
    let mut cur = Fr::one();
    let mut tmp = Fr::zero();
    let mut result_point = ExtendedPoint::identity();

    let mut rem: u64 = 0;
    let mut el: u64 = 0;

    let mut k = 1;
    while i < m.len() {
        x = 0;
        rem = if i + 6 <= m.len() {
            6
        } else {
            (m.len() - i) as u64
        };
        x += m[i] as u64;
        i += 1;
        let mut j = 1;
        while j < rem {
            x <<= 8;
            x += m[i] as u64;
            i += 1;
            j += 1;
        }
        if i == m.len() {
            //handling last bytes
            remainingbits %= 48;
            el = remainingbits / 3;
            remainingbits %= 3;
        } else {
            el = 16;
        }
        k = 1;
        while k < (el + 1) {
            bits = (x >> (rem * 8 - k * 3) & 7) as u8;
            tmp = handle_chunk(bits, &mut cur);
            acc = acc.add(&tmp);

            //extract bits from index
            counter += 1;
            if counter == maxcounter {
                //add point to result_point
                let mut str = points[pointcounter];
                let q = AffinePoint::from_bytes(str).unwrap().to_niels();
                let mut p = q.multiply_bits(&acc.to_bytes());
                result_point = result_point + p;

                counter = 0;
                pointcounter += 1;
                acc = Fr::zero();
                cur = Fr::one();
            } else {
                cur = cur.double().double().double();
            }
            k += 1;
        }
    } //change to loop
    if remainingbits > 0 {
        if rem * 8 < k * 3 {
            let tr = if rem % 3 == 1 { 3 } else { 1 };
            bits = ((x & tr) << (rem % 3)) as u8;
        } else {
            bits = (x >> (rem * 8 - k * 3) & 7) as u8;
        }
        tmp = handle_chunk(bits, &mut cur);
        acc = acc.add(&tmp);
        counter += 1;
    }
    if counter > 0 {
        let mut str = points[pointcounter];
        let q = AffinePoint::from_bytes(str).unwrap().to_niels();
        let mut p = q.multiply_bits(&acc.to_bytes());
        result_point = result_point + p;
    }
    return AffinePoint::from(result_point).get_u().to_bytes();
}

fn pedersen_hash(m: &[u8]) -> [u8; 32] {
    let points = [
        [
            0xca, 0x3c, 0x24, 0x32, 0xd4, 0xab, 0xbf, 0x77, 0x32, 0x46, 0x4e, 0xc0, 0x8b, 0x2e,
            0x47, 0xf9, 0x5e, 0xdc, 0x7e, 0x83, 0x6b, 0x16, 0xc9, 0x79, 0x57, 0x1b, 0x52, 0xd3,
            0xa2, 0x87, 0x9e, 0xa8,
        ],
        [
            0x91, 0x18, 0xbf, 0x4e, 0x3c, 0xc5, 0x0d, 0x7b, 0xe8, 0xd3, 0xfa, 0x98, 0xeb, 0xbe,
            0x3a, 0x1f, 0x25, 0xd9, 0x01, 0xc0, 0x42, 0x11, 0x89, 0xf7, 0x33, 0xfe, 0x43, 0x5b,
            0x7f, 0x8c, 0x5d, 0x01,
        ],
        [
            0x57, 0xd4, 0x93, 0x97, 0x2c, 0x50, 0xed, 0x80, 0x98, 0xb4, 0x84, 0x17, 0x7f, 0x2a,
            0xb2, 0x8b, 0x53, 0xe8, 0x8c, 0x8e, 0x6c, 0xa4, 0x00, 0xe0, 0x9e, 0xee, 0x4e, 0xd2,
            0x00, 0x15, 0x2e, 0xb6,
        ],
        [
            0xe9, 0x70, 0x35, 0xa3, 0xec, 0x4b, 0x71, 0x84, 0x85, 0x6a, 0x1f, 0xa1, 0xa1, 0xaf,
            0x03, 0x51, 0xb7, 0x47, 0xd9, 0xd8, 0xcb, 0x0a, 0x07, 0x91, 0xd8, 0xca, 0x56, 0x4b,
            0x0c, 0xe4, 0x7e, 0x2f,
        ],
    ];

    let table = [
        (0, 0, 0),
        (2, 3, 1),
        (5, 1, 2),
        (8, 0, 0),
        (10, 3, 1),
        (13, 1, 2),
        (16, 0, 0),
    ];

    let mut i = 0;
    let mut counter: usize = 1;
    let mut pointcounter: usize = 0;
    let maxcounter: usize = 63;

    //handle first u8 different as only 6 bits are possibly set
    //todo: here we assume 6 LSB of M[0] are possibly set, depends on encoding!

    let mut acc = Fr::zero();
    let mut cur = Fr::one();
    let mut tmp = Fr::zero();

    let mut bits = (m[i] >> 3) & 7;
    tmp = handle_chunk(bits, &mut cur);
    cur = cur.double().double().double();
    acc = acc.add(&tmp);

    bits = (m[i]) & 7;
    tmp = handle_chunk(bits, &mut cur);
    cur = cur.double().double().double();

    acc = acc.add(&tmp);
    counter += 1;
    i += 1;

    let mut result_point = ExtendedPoint::identity();
    if i == m.len() {
        //empty message
        let mut str = points[pointcounter];
        let q = AffinePoint::from_bytes(str).unwrap().to_niels();
        let mut p = q.multiply_bits(&acc.to_bytes());
        result_point = result_point + p;
        return AffinePoint::from(result_point).get_u().to_bytes();
    }
    let mut el: u64 = 0;
    let mut ft = 0;
    let mut tr = 0;
    let mut x: u64 = 0;
    while i < m.len() {
        x = 0;
        let rem: u64 = if i + 6 <= m.len() {
            6
        } else {
            (m.len() - i) as u64
        };
        x += m[i] as u64;
        i += 1;
        let mut j = 1;
        while j < rem {
            x <<= 8;
            x += m[i] as u64;
            i += 1;
            j += 1;
        }

        let entry = table[rem as usize];
        el = entry.0 as u64;
        ft = entry.1 as u64;
        tr = entry.2 as u64;

        for j in 1..(el + 1) {
            bits = (x >> (rem * 8 - j * 3) & 7) as u8;
            tmp = handle_chunk(bits, &mut cur);
            acc = acc.add(&tmp);

            //extract bits from index
            counter += 1;
            if counter == maxcounter {
                //add point to result_point
                let mut str = points[pointcounter];
                let q = AffinePoint::from_bytes(str).unwrap().to_niels();
                let mut p = q.multiply_bits(&acc.to_bytes());
                result_point = result_point + p;

                counter = 1;
                pointcounter += 1;
                acc = Fr::zero();
                cur = Fr::one();
            } else {
                cur = cur.double().double().double();
            }
        }
    }
    if ft > 0 {
        bits = ((x & ft) << tr) as u8;
        tmp = handle_chunk(bits, &mut cur);
        acc = acc.add(&tmp);
    }
    let mut str = points[pointcounter];
    let q = AffinePoint::from_bytes(str).unwrap().to_niels();
    let mut p = q.multiply_bits(&acc.to_bytes());
    result_point = result_point + p;

    return AffinePoint::from(result_point).get_u().to_bytes();
}

#[cfg(test)]
fn encode_test(v: &[u8]) -> Vec<u8> {
    let n = if v.len() % 8 > 0 {
        1 + v.len() / 8
    } else {
        v.len() / 8
    };
    let mut result: Vec<u8> = std::vec::Vec::new();
    let mut i = 0;
    while i < n {
        result.push(0);
        for j in 0..8 {
            let s = if i * 8 + j < v.len() { v[i * 8 + j] } else { 0 };
            result[i] += s;
            if j < 7 {
                result[i] <<= 1;
            }
        }
        i += 1;
    }
    result
}
#[cfg(test)]
mod tests {
    use crate::*;
    use hex::encode;

    #[test]
    fn test_encode_test() {
        let f1: [u8; 9] = [0, 0, 0, 0, 0, 0, 0, 1, 1];
        assert_eq!(encode_test(&f1).as_slice(), &[1, 128]);
    }

    #[test]
    fn test_handlechunk() {
        let bits: u8 = 1;
        let mut cur = Fr::one();
        let tmp = handle_chunk(bits, &mut cur);
        //     assert_eq!(tmp.to_bytes(),[3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn test_pedersen_small() {
        let input_bits: [u8; 9] = [1, 1, 1, 1, 1, 1, 1, 0, 0];
        let m = encode_test(&input_bits);
        let h = pedersen_hash_len(&m, 9);
        assert_eq!(pedersen_hash_len(&[254, 0], 9), h);
    }

    #[test]
    fn test_pedersen_onechunk() {
        let input_bits: [u8; 189] = [
            1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 1, 1, 1, 1, 0, 1, 1, 1, 0, 0, 0, 1, 1, 1, 0, 0, 0,
            0, 1, 0, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1, 0, 1, 1, 0, 1, 0, 0, 0, 1, 1, 0, 1, 0, 0, 0,
            0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 1, 1, 1, 1, 1, 0, 1, 1, 1, 0, 1, 0, 1,
            1, 1, 0, 0, 1, 0, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 1, 1, 1, 0, 0, 0, 0, 1, 1, 1, 0, 1, 1,
            0, 0, 1, 1, 0, 0, 1, 0, 1, 0, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 1, 0, 1,
            1, 1, 0, 1, 0, 0, 1, 1, 1, 1, 1, 0, 0, 1, 0, 1, 0, 1, 0, 0, 1, 1, 0, 1, 0, 0, 0, 1, 0,
            1, 0, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 0, 0,
        ];
        let m = encode_test(&input_bits);
        let h = pedersen_hash_len(&m, input_bits.len() as u64);
        assert_eq!(
            h,
            [
                0xdd, 0xf5, 0x21, 0xad, 0xc3, 0xa5, 0x97, 0xf5, 0xcf, 0x72, 0x29, 0xff, 0x02, 0xcf,
                0xed, 0x7e, 0x94, 0x9f, 0x01, 0xb6, 0x1d, 0xf3, 0xe1, 0xdc, 0xdf, 0xf5, 0x20, 0x76,
                0x31, 0x10, 0xa5, 0x2d
            ]
        );
    }

    #[test]
    fn test_pedersen_big() {
        let input_bits: [u8; 190] = [
            1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 0, 0, 1, 1, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0,
            0, 1, 1, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 1, 1, 1, 0, 0, 0, 1, 1, 1, 0, 1, 1,
            0, 0, 1, 1, 1, 1, 1, 0, 0, 1, 0, 1, 0, 1, 0, 0, 0, 0, 1, 0, 1, 1, 0, 1, 1, 0, 0, 1, 1,
            1, 1, 1, 0, 1, 0, 0, 0, 1, 0, 1, 0, 1, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1,
            1, 0, 0, 1, 0, 1, 1, 0, 0, 0, 0, 1, 1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1,
            0, 1, 1, 0, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 0, 1, 1, 0, 0, 0, 1, 1, 0, 0, 1, 1, 1, 1, 0,
            0, 0, 0, 1, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 1, 1,
        ];
        let m = encode_test(&input_bits);
        let h = pedersen_hash_len(&m, input_bits.len() as u64);
        assert_eq!(
            h,
            [
                0x40, 0x0c, 0xf2, 0x1e, 0xeb, 0x6f, 0x8e, 0x59, 0x4a, 0x0e, 0xcd, 0x2b, 0x7f, 0x7a,
                0x68, 0x46, 0x34, 0xd9, 0x6e, 0xdf, 0x51, 0xfb, 0x3d, 0x19, 0x2d, 0x99, 0x40, 0xe6,
                0xc7, 0x47, 0x12, 0x60
            ]
        );

        let inp2: [u8; 756] = [
            1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0,
            1, 0, 1, 1, 1, 1, 0, 1, 0, 1, 1, 0, 0, 1, 1, 1, 0, 0, 0, 0, 1, 1, 0, 0, 1, 0, 0, 1, 1,
            1, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 0, 1, 0, 0, 0, 0, 0, 1, 1, 1, 0, 1, 1,
            1, 1, 1, 0, 0, 1, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 0, 1, 1, 0, 0, 1, 0, 1, 0, 1, 1, 1, 0,
            1, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 0, 0, 1, 0, 1, 1, 1, 0, 0, 0, 1, 1, 0, 1, 1, 1,
            0, 0, 0, 0, 0, 1, 1, 0, 1, 0, 0, 0, 0, 0, 1, 1, 1, 1, 0, 1, 0, 1, 1, 0, 0, 0, 1, 0, 0,
            0, 0, 1, 1, 0, 1, 0, 1, 0, 1, 1, 1, 1, 1, 0, 0, 0, 1, 0, 1, 0, 1, 1, 1, 0, 0, 1, 0, 0,
            1, 1, 0, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 1, 1, 0, 0, 1, 0, 0, 1, 1, 1, 1, 0, 1, 1, 1,
            1, 0, 0, 1, 1, 1, 0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 1, 1,
            0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 1, 1, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0, 1, 1, 0, 1, 1, 0,
            0, 1, 0, 0, 0, 0, 1, 0, 1, 0, 0, 1, 0, 1, 0, 0, 1, 0, 1, 0, 1, 1, 0, 1, 1, 0, 0, 1, 0,
            1, 1, 1, 1, 1, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 1, 1,
            1, 0, 0, 1, 0, 1, 1, 1, 1, 0, 1, 0, 1, 1, 1, 0, 1, 1, 0, 1, 0, 1, 1, 1, 1, 0, 1, 0, 0,
            1, 1, 0, 0, 0, 1, 0, 0, 1, 1, 0, 1, 1, 1, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 1, 0, 1, 0, 1,
            0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1,
            0, 1, 1, 0, 1, 0, 1, 0, 1, 0, 1, 1, 0, 0, 0, 1, 1, 0, 0, 1, 0, 1, 1, 1, 0, 0, 1, 0, 0,
            0, 1, 0, 0, 0, 1, 1, 1, 1, 1, 0, 1, 0, 0, 0, 0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 0, 1,
            1, 1, 1, 0, 1, 0, 0, 0, 0, 1, 0, 1, 0, 0, 1, 0, 0, 0, 1, 1, 0, 0, 1, 0, 0, 0, 1, 1, 0,
            0, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0, 0, 0, 1, 0, 1, 1, 0, 1, 0, 0, 0, 1, 1, 1, 0, 1, 0, 0,
            1, 1, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 1, 1, 0, 1, 1, 0, 1, 0, 1, 0, 0, 0,
            1, 0, 1, 0, 1, 0, 1, 1, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 1, 1, 0, 0, 1, 1,
            0, 1, 0, 1, 1, 0, 1, 1, 1, 1, 0, 1, 0, 1, 0, 0, 1, 1, 1, 1, 0, 0, 0, 1, 0, 1, 1, 1, 1,
            1, 1, 0, 1, 0, 1, 0, 0, 1, 1, 1, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 0, 1, 1, 0, 1, 0, 1, 0,
            1, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 0, 1, 0, 1, 0, 0, 1, 0,
            0, 0, 1, 0, 1, 1, 0, 1, 0, 0, 0, 0, 0, 1, 0, 0, 1, 0, 1, 0, 0, 1, 0, 0, 0, 1, 1, 0, 1,
            0, 1, 1, 1, 1, 0, 0, 1, 1, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1,
            1, 1,
        ];
        let m2 = encode_test(&inp2);
        let h2 = pedersen_hash_len(&m2, inp2.len() as u64);
        assert_eq!(
            h2,
            [
                0x27, 0xae, 0xf2, 0xe8, 0xeb, 0xed, 0xad, 0x19, 0x39, 0x37, 0x9f, 0x4f, 0x44, 0x7e,
                0xfb, 0xd9, 0x25, 0x5a, 0x87, 0x4c, 0x70, 0x08, 0x81, 0x6a, 0x80, 0xd8, 0xf2, 0xb1,
                0xec, 0x92, 0x41, 0x31
            ]
        );

        let inp3: [u8; 945] = [
            0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0,
            0, 1, 0, 1, 1, 1, 1, 0, 1, 0, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 1, 0, 1, 1, 1, 0, 0, 0, 0,
            0, 1, 0, 1, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 0, 0, 1, 1, 0, 0, 0,
            1, 1, 0, 1, 1, 0, 1, 0, 1, 0, 1, 1, 0, 0, 1, 0, 0, 0, 0, 1, 0, 1, 0, 0, 1, 0, 0, 1, 0,
            0, 1, 1, 0, 1, 0, 1, 0, 0, 0, 1, 1, 1, 1, 1, 0, 1, 0, 1, 0, 0, 0, 0, 1, 1, 0, 1, 1, 0,
            1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 1, 0, 1, 0, 0, 0, 1, 1, 0, 1, 1, 1, 1,
            1, 0, 0, 1, 1, 1, 0, 0, 1, 1, 0, 1, 1, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 1, 1,
            1, 1, 1, 0, 1, 0, 1, 0, 0, 1, 0, 0, 1, 1, 0, 0, 0, 1, 1, 1, 0, 0, 1, 0, 1, 1, 1, 1, 0,
            0, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 0, 0, 1, 0, 0, 0, 0, 1, 0, 1, 1, 0, 1, 1, 0, 1, 1,
            0, 0, 1, 1, 0, 0, 0, 0, 1, 1, 1, 1, 1, 0, 1, 0, 0, 1, 1, 0, 1, 1, 1, 1, 1, 0, 0, 0, 1,
            0, 0, 1, 1, 0, 0, 0, 0, 0, 1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 0, 0, 1, 0, 0, 1,
            0, 1, 1, 0, 0, 1, 0, 0, 0, 1, 1, 1, 0, 1, 1, 0, 1, 0, 1, 0, 0, 0, 1, 1, 0, 1, 1, 0, 1,
            0, 1, 1, 1, 0, 0, 0, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1, 1, 0, 1, 0, 1, 1, 1, 0, 1, 0, 1, 0,
            0, 1, 0, 0, 1, 0, 0, 1, 1, 1, 0, 0, 0, 0, 0, 1, 1, 1, 1, 0, 1, 0, 1, 1, 1, 0, 1, 1, 1,
            1, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 0,
            0, 1, 0, 0, 0, 1, 1, 0, 1, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 0, 0, 1, 1, 1, 0, 0, 0, 1, 0,
            1, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 1, 1, 0, 1, 1, 0, 1, 0, 0, 0, 0, 0, 1, 0, 1,
            1, 1, 0, 0, 0, 0, 1, 1, 1, 1, 0, 0, 1, 1, 1, 0, 1, 0, 1, 0, 1, 1, 1, 0, 0, 0, 1, 1, 1,
            1, 0, 0, 1, 0, 0, 1, 0, 1, 1, 1, 1, 0, 0, 0, 1, 1, 0, 0, 0, 0, 1, 0, 0, 1, 0, 1, 1, 0,
            0, 0, 0, 1, 1, 1, 0, 1, 1, 0, 0, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 1, 1,
            1, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 1, 0, 1, 0, 0, 1, 0, 0, 1, 1, 0, 0, 0, 0, 1,
            1, 1, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 1, 1, 1, 1, 0, 1, 1, 0, 1, 0, 0, 1, 0, 0, 0, 1, 0,
            1, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 0, 1, 1,
            1, 1, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 1, 0, 1, 1, 0, 1, 1, 0, 0, 0, 1, 1, 0, 1, 1, 1, 0,
            0, 1, 1, 0, 0, 0, 1, 0, 1, 0, 0, 1, 1, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 1, 1, 0, 1, 1,
            1, 1, 0, 1, 0, 0, 1, 1, 0, 1, 1, 1, 0, 1, 1, 0, 1, 0, 1, 1, 1, 1, 0, 1, 0, 0, 1, 0, 1,
            0, 0, 1, 1, 0, 0, 0, 0, 1, 1, 0, 0, 1, 1, 1, 0, 1, 0, 0, 0, 0, 1, 1, 0, 1, 1, 1, 1, 1,
            0, 0, 1, 0, 0, 1, 0, 1, 0, 0, 0, 1, 1, 1, 1, 0, 1, 1, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 0,
            0, 0, 1, 1, 0, 0, 0, 0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 1, 1, 0, 0, 1, 0, 1,
            0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 0, 1, 1, 0, 1, 1, 0, 1, 1, 1, 0, 0, 1, 1,
            1, 0, 0, 1, 1, 0, 0, 1, 1, 1, 0, 0, 0, 0, 1, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 1,
            0, 0, 0, 1, 0, 1, 1, 1, 1, 0, 0, 1, 0, 0, 1, 1, 0,
        ];
        let m3 = encode_test(&inp3);
        let h3 = pedersen_hash_len(&m3, inp3.len() as u64);
        assert_eq!(
            h3,
            [
                0x37, 0x5f, 0xdd, 0x7b, 0x29, 0xde, 0x6e, 0x22, 0x5e, 0xbb, 0x7a, 0xe4, 0x20, 0x3c, 0xa5, 0x0e, 0xca, 0x7c, 0x9b, 0xab, 0x97, 0x1c, 0xc6, 0x91, 0x3c, 0x6f, 0x13, 0xed, 0xf3, 0x27, 0xe8, 0x00
            ]
        );
    }

    #[test]
    fn test_sharedsecret() {
        let esk: [u8; 32] = [
            0x81, 0xc7, 0xb2, 0x17, 0x1f, 0xf4, 0x41, 0x52, 0x50, 0xca, 0xc0, 0x1f, 0x59, 0x82,
            0xfd, 0x8f, 0x49, 0x61, 0x9d, 0x61, 0xad, 0x78, 0xf6, 0x83, 0x0b, 0x3c, 0x60, 0x61,
            0x45, 0x96, 0x2a, 0x0e,
        ];
        let pk_d: [u8; 32] = [
            0x88, 0x99, 0xc6, 0x44, 0xbf, 0xc6, 0x0f, 0x87, 0x83, 0xf9, 0x2b, 0xa9, 0xf8, 0x18,
            0x9e, 0xd2, 0x77, 0xbf, 0x68, 0x3d, 0x5d, 0x1d, 0xae, 0x02, 0xc5, 0x71, 0xff, 0x47,
            0x86, 0x9a, 0x0b, 0xa6,
        ];
        let sharedsecret: [u8; 32] = [
            0x2e, 0x35, 0x7d, 0x82, 0x2e, 0x02, 0xdc, 0xe8, 0x84, 0xee, 0x94, 0x8a, 0xb4, 0xff,
            0xb3, 0x20, 0x6b, 0xa5, 0x74, 0x77, 0xac, 0x7d, 0x7b, 0x07, 0xed, 0x44, 0x6c, 0x3b,
            0xe4, 0x48, 0x1b, 0x3e,
        ];
        assert_eq!(sapling_ka_agree(esk, pk_d), sharedsecret);
    }

    #[test]
    fn test_encryption() {
        let k_enc = [
            0x6d, 0xf8, 0x5b, 0x17, 0x89, 0xb0, 0xb7, 0x8b, 0x46, 0x10, 0xf2, 0x5d, 0x36, 0x8c,
            0xb5, 0x11, 0x14, 0x0a, 0x7c, 0x0a, 0xf3, 0xbc, 0x3d, 0x2a, 0x22, 0x6f, 0x92, 0x7d,
            0xe6, 0x02, 0xa7, 0xf1,
        ];
        let p_enc = [
            0x01, 0xdc, 0xe7, 0x7e, 0xbc, 0xec, 0x0a, 0x26, 0xaf, 0xd6, 0x99, 0x8c, 0x00, 0xe1,
            0xf5, 0x05, 0x00, 0x00, 0x00, 0x00, 0x39, 0x17, 0x6d, 0xac, 0x39, 0xac, 0xe4, 0x98,
            0x0e, 0xcc, 0x8d, 0x77, 0x8e, 0x89, 0x86, 0x02, 0x55, 0xec, 0x36, 0x15, 0x06, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf6, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00,
        ];

        let c_enc = [
            0xbd, 0xcb, 0x94, 0x72, 0xa1, 0xac, 0xad, 0xf1, 0xd0, 0x82, 0x07, 0xf6, 0x3c, 0xaf,
            0x4f, 0x3a, 0x76, 0x3c, 0x67, 0xd0, 0x66, 0x56, 0x0a, 0xd9, 0x6c, 0x1e, 0xf9, 0x52,
            0xf8, 0x46, 0xa9, 0xc2, 0x80, 0x82, 0xdd, 0xef, 0x45, 0x21, 0xf6, 0x82, 0x54, 0x76,
            0xad, 0xe3, 0x2e, 0xeb, 0x34, 0x64, 0x06, 0xa5, 0xee, 0xc9, 0x4b, 0x4a, 0xb9, 0xe4,
            0x55, 0x12, 0x42, 0xb1, 0x44, 0xa4, 0xf8, 0xc8, 0x28, 0xbc, 0x19, 0x7f, 0x3e, 0x92,
            0x5f, 0x61, 0x7f, 0xc4, 0xb9, 0xc1, 0xb1, 0x53, 0xad, 0x15, 0x3a, 0x3c, 0x56, 0xf8,
            0x1f, 0xc4, 0x8b, 0xf5, 0x4e, 0x6e, 0xe8, 0x89, 0x5f, 0x27, 0x8c, 0x5e, 0x4c, 0x6a,
            0xe7, 0xa8, 0xa0, 0x23, 0x86, 0x70, 0x85, 0xb4, 0x07, 0xbe, 0xce, 0x40, 0x0b, 0xc6,
            0xaa, 0xec, 0x06, 0xaf, 0xf8, 0xb0, 0x49, 0xbc, 0xb2, 0x63, 0x63, 0xc6, 0xde, 0x01,
            0x8d, 0x2d, 0xa0, 0x41, 0xcc, 0x2e, 0xb8, 0xd0, 0x86, 0x4a, 0x70, 0xdf, 0x68, 0x47,
            0xb3, 0x37, 0x5a, 0x31, 0x86, 0x6c, 0x49, 0xa8, 0x02, 0x5a, 0xd7, 0x17, 0xe7, 0x79,
            0xbd, 0x0f, 0xb5, 0xce, 0xed, 0x3e, 0xc4, 0x40, 0x8e, 0x18, 0x50, 0x69, 0x4b, 0xa3,
            0x56, 0x39, 0xdd, 0x8b, 0x55, 0xd2, 0xbf, 0xdf, 0xc6, 0x40, 0x6c, 0x78, 0xc0, 0x0e,
            0xb5, 0xfc, 0x48, 0x76, 0x4b, 0xf4, 0xd8, 0x4d, 0xe1, 0xa0, 0x26, 0xd9, 0x02, 0x86,
            0x60, 0xa9, 0xa5, 0xc1, 0xc5, 0x94, 0xb8, 0x15, 0x8c, 0x69, 0x1e, 0x50, 0x68, 0xc8,
            0x51, 0xda, 0xfa, 0x30, 0x10, 0xe3, 0x9b, 0x70, 0xc4, 0x66, 0x83, 0x73, 0xbb, 0x59,
            0xac, 0x53, 0x07, 0x0c, 0x7b, 0x3f, 0x76, 0x62, 0x03, 0x84, 0x27, 0xb3, 0x72, 0xfd,
            0x75, 0x36, 0xe5, 0x4d, 0x8c, 0x8e, 0x61, 0x56, 0x2c, 0xb0, 0xe5, 0x7e, 0xf7, 0xb4,
            0x43, 0xde, 0x5e, 0x47, 0x8f, 0x4b, 0x02, 0x9c, 0x36, 0xaf, 0x71, 0x27, 0x1a, 0x0f,
            0x9d, 0x57, 0xbe, 0x80, 0x1b, 0xc4, 0xf2, 0x61, 0x8d, 0xc4, 0xf0, 0xab, 0xd1, 0x5f,
            0x0b, 0x42, 0x0c, 0x11, 0x14, 0xbb, 0xd7, 0x27, 0xe4, 0xb3, 0x1a, 0x6a, 0xaa, 0xd8,
            0xfe, 0x53, 0xb7, 0xdf, 0x60, 0xb4, 0xe0, 0xc9, 0xe9, 0x45, 0x7b, 0x89, 0x3f, 0x20,
            0xec, 0x18, 0x61, 0x1e, 0x68, 0x03, 0x05, 0xfe, 0x04, 0xba, 0x3b, 0x8d, 0x30, 0x1f,
            0x5c, 0xd8, 0x2c, 0x2c, 0x8d, 0x1c, 0x58, 0x5d, 0x51, 0x15, 0x4b, 0x46, 0x88, 0xff,
            0x5a, 0x35, 0x0b, 0x60, 0xae, 0x30, 0xda, 0x4f, 0x74, 0xc3, 0xd5, 0x5c, 0x73, 0xda,
            0xe8, 0xad, 0x9a, 0xb8, 0x0b, 0xbb, 0x5d, 0xdf, 0x1b, 0xea, 0xec, 0x12, 0x0f, 0xc4,
            0xf7, 0x8d, 0xe5, 0x4f, 0xef, 0xe1, 0xa8, 0x41, 0x35, 0x79, 0xfd, 0xce, 0xa2, 0xf6,
            0x56, 0x74, 0x10, 0x4c, 0xba, 0xac, 0x7e, 0x0d, 0xe5, 0x08, 0x3d, 0xa7, 0xb1, 0xb7,
            0xf2, 0xe9, 0x43, 0x70, 0xdd, 0x0a, 0x3e, 0xed, 0x71, 0x50, 0x36, 0x54, 0x2f, 0xa4,
            0x0e, 0xd4, 0x89, 0x2b, 0xaa, 0xfb, 0x57, 0x2e, 0xe0, 0xf9, 0x45, 0x9c, 0x1c, 0xbe,
            0x3a, 0xd1, 0xb6, 0xaa, 0xf1, 0x1f, 0x54, 0x93, 0x59, 0x52, 0xbe, 0x6b, 0x95, 0x38,
            0xa9, 0xa3, 0x9e, 0xde, 0x64, 0x2b, 0xb0, 0xcd, 0xac, 0x1c, 0x09, 0x09, 0x2c, 0xd7,
            0x11, 0x16, 0x0a, 0x8d, 0x45, 0x19, 0xb4, 0xce, 0x20, 0xff, 0xf6, 0x61, 0x2b, 0xc7,
            0xb0, 0x53, 0x93, 0xbb, 0x7e, 0x96, 0xf8, 0xea, 0x4b, 0xbc, 0x97, 0x83, 0x1f, 0x20,
            0x46, 0xe1, 0xcb, 0x5a, 0x2c, 0xe7, 0xca, 0x36, 0xfd, 0x06, 0xab, 0x39, 0x56, 0xa8,
            0x03, 0xd4, 0x32, 0x5a, 0xae, 0x72, 0xef, 0xb7, 0x07, 0xca, 0xa0, 0x44, 0xd3, 0xf8,
            0xfc, 0x7d, 0x09, 0x46, 0xbe, 0xb1, 0x1c, 0xdd, 0xc8, 0x53, 0xdb, 0xcf, 0x24, 0x3a,
            0xf3, 0xe5, 0x92, 0xb8, 0x1d, 0xb3, 0x64, 0x19, 0xd3, 0x4a, 0x4b, 0xb1, 0xee, 0x53,
            0xc1, 0xa1, 0xba, 0x51, 0xc1, 0x8b, 0x2e, 0xe9, 0x2d, 0xb4, 0xbf, 0x5f, 0xce, 0xeb,
            0x82, 0x0e, 0x8c, 0x58, 0xf8, 0x16, 0x6c, 0x3a, 0xcb, 0xf7, 0x61, 0xb5, 0xb1, 0xf2,
            0x9c, 0x3f, 0x11, 0x81, 0x67, 0xbb, 0x6c, 0xdb, 0x23, 0x30, 0x35, 0x29, 0x6a, 0xd4,
            0x0e, 0x8a, 0xa0, 0xce, 0xf5, 0x70,
        ];
        assert_eq!(chacha_encryptnote(k_enc, p_enc)[0..32], c_enc[0..32]);
        assert_eq!(chacha_decryptnote(k_enc, c_enc)[0..32], p_enc[0..32]);
    }

    #[test]
    fn test_kdf() {
        let esk: [u8; 32] = [
            0x81, 0xc7, 0xb2, 0x17, 0x1f, 0xf4, 0x41, 0x52, 0x50, 0xca, 0xc0, 0x1f, 0x59, 0x82,
            0xfd, 0x8f, 0x49, 0x61, 0x9d, 0x61, 0xad, 0x78, 0xf6, 0x83, 0x0b, 0x3c, 0x60, 0x61,
            0x45, 0x96, 0x2a, 0x0e,
        ];
        let g_d = pkd_group_hash(&[
            0xdc, 0xe7, 0x7e, 0xbc, 0xec, 0x0a, 0x26, 0xaf, 0xd6, 0x99, 0x8c,
        ]);
        let dp = derive_public(esk, g_d);

        let epk: [u8; 32] = [
            0x7e, 0xb9, 0x28, 0xf9, 0xf6, 0xd5, 0x96, 0xbf, 0xbf, 0x81, 0x4e, 0x3d, 0xd0, 0xe2,
            0x4f, 0xdc, 0x52, 0x03, 0x0f, 0xd1, 0x0f, 0x49, 0x0b, 0xa2, 0x04, 0x58, 0x68, 0xda,
            0x98, 0xf3, 0x49, 0x36,
        ];
        assert_eq!(dp, epk);
        let k_enc = [
            0x6d, 0xf8, 0x5b, 0x17, 0x89, 0xb0, 0xb7, 0x8b, 0x46, 0x10, 0xf2, 0x5d, 0x36, 0x8c,
            0xb5, 0x11, 0x14, 0x0a, 0x7c, 0x0a, 0xf3, 0xbc, 0x3d, 0x2a, 0x22, 0x6f, 0x92, 0x7d,
            0xe6, 0x02, 0xa7, 0xf1,
        ];
        let sharedsecret: [u8; 32] = [
            0x2e, 0x35, 0x7d, 0x82, 0x2e, 0x02, 0xdc, 0xe8, 0x84, 0xee, 0x94, 0x8a, 0xb4, 0xff,
            0xb3, 0x20, 0x6b, 0xa5, 0x74, 0x77, 0xac, 0x7d, 0x7b, 0x07, 0xed, 0x44, 0x6c, 0x3b,
            0xe4, 0x48, 0x1b, 0x3e,
        ];
        assert_eq!(kdf_sapling(sharedsecret, epk), k_enc);
    }

    #[test]
    fn test_ock() {
        //prf_ock(ovk, cv, cmu, ephemeral_key)
        let ovk: [u8; 32] = [
            0x98, 0xd1, 0x69, 0x13, 0xd9, 0x9b, 0x04, 0x17, 0x7c, 0xab, 0xa4, 0x4f, 0x6e, 0x4d,
            0x22, 0x4e, 0x03, 0xb5, 0xac, 0x03, 0x1d, 0x7c, 0xe4, 0x5e, 0x86, 0x51, 0x38, 0xe1,
            0xb9, 0x96, 0xd6, 0x3b,
        ];

        let cv: [u8; 32] = [
            0xa9, 0xcb, 0x0d, 0x13, 0x72, 0x32, 0xff, 0x84, 0x48, 0xd0, 0xf0, 0x78, 0xb6, 0x81,
            0x4c, 0x66, 0xcb, 0x33, 0x1b, 0x0f, 0x2d, 0x3d, 0x8a, 0x08, 0x5b, 0xed, 0xba, 0x81,
            0x5f, 0x00, 0xa8, 0xdb,
        ];

        let cmu: [u8; 32] = [
            0x8d, 0xe2, 0xc9, 0xb3, 0xf9, 0x14, 0x67, 0xd5, 0x14, 0xfe, 0x2f, 0x97, 0x42, 0x2c,
            0x4f, 0x76, 0x11, 0xa9, 0x1b, 0xb7, 0x06, 0xed, 0x5c, 0x27, 0x72, 0xd9, 0x91, 0x22,
            0xa4, 0x21, 0xe1, 0x2d,
        ];

        let epk: [u8; 32] = [
            0x7e, 0xb9, 0x28, 0xf9, 0xf6, 0xd5, 0x96, 0xbf, 0xbf, 0x81, 0x4e, 0x3d, 0xd0, 0xe2,
            0x4f, 0xdc, 0x52, 0x03, 0x0f, 0xd1, 0x0f, 0x49, 0x0b, 0xa2, 0x04, 0x58, 0x68, 0xda,
            0x98, 0xf3, 0x49, 0x36,
        ];

        let ock: [u8; 32] = [
            0x41, 0x14, 0x43, 0xfc, 0x1d, 0x92, 0x54, 0x33, 0x74, 0x15, 0xb2, 0x14, 0x7a, 0xde,
            0xcd, 0x48, 0xf3, 0x13, 0x76, 0x9c, 0x3b, 0xa1, 0x77, 0xd4, 0xcd, 0x34, 0xd6, 0xfb,
            0xd1, 0x40, 0x27, 0x0d,
        ];

        assert_eq!(prf_ock(ovk, cv, cmu, epk), ock);
    }
}
