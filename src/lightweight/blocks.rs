//! Small BC1/BC3/BC4 material mip conversion. Only used when a reference requests
//! dimensions that cannot be obtained by selecting an existing native mip.
fn rgb565(v: u16) -> [u8; 3] {
    let r = (v >> 11) & 31;
    let g = (v >> 5) & 63;
    let b = v & 31;
    [
        (r * 255 / 31) as u8,
        (g * 255 / 63) as u8,
        (b * 255 / 31) as u8,
    ]
}
fn to565(c: [u8; 3]) -> u16 {
    (((c[0] as u32 * 31 + 127) / 255) as u16) << 11
        | (((c[1] as u32 * 63 + 127) / 255) as u16) << 5
        | ((c[2] as u32 * 31 + 127) / 255) as u16
}
fn colors(a: u16, b: u16, force_four: bool) -> [[u8; 4]; 4] {
    let a3 = rgb565(a);
    let b3 = rgb565(b);
    let mut out = [[0; 4]; 4];
    out[0] = [a3[0], a3[1], a3[2], 255];
    out[1] = [b3[0], b3[1], b3[2], 255];
    for c in 0..3 {
        if a > b || force_four {
            out[2][c] = ((2 * a3[c] as u16 + b3[c] as u16) / 3) as u8;
            out[3][c] = ((a3[c] as u16 + 2 * b3[c] as u16) / 3) as u8;
        } else {
            out[2][c] = ((a3[c] as u16 + b3[c] as u16) / 2) as u8;
        }
    }
    out[2][3] = 255;
    if a > b || force_four {
        out[3][3] = 255;
    }
    out
}
fn alphas(a: u8, b: u8) -> [u8; 8] {
    let mut out = [a, b, 0, 0, 0, 0, 0, 0];
    if a > b {
        for i in 1..=6 {
            out[i + 1] = (((7 - i) * a as usize + i * b as usize) / 7) as u8;
        }
    } else {
        for i in 1..=4 {
            out[i + 1] = (((5 - i) * a as usize + i * b as usize) / 5) as u8;
        }
        out[6] = 0;
        out[7] = 255;
    }
    out
}
pub fn decode(bytes: &[u8], w: usize, h: usize, format: u16) -> Result<Vec<u8>, String> {
    if format == 31 {
        if bytes.len() != w * h * 4 {
            return Err("invalid raw mip".into());
        }
        return Ok(bytes.to_vec());
    }
    let stride = match format {
        57 | 58 | 63 => 8,
        61 | 62 => 16,
        _ => return Err("unsupported BC format".into()),
    };
    if bytes.len() != w.div_ceil(4) * h.div_ceil(4) * stride {
        return Err("invalid BC mip length".into());
    }
    let mut out = vec![0; w * h * 4];
    for by in 0..h.div_ceil(4) {
        for bx in 0..w.div_ceil(4) {
            let p = (by * w.div_ceil(4) + bx) * stride;
            let block = &bytes[p..p + stride];
            let alpha = if stride == 16 || format == 63 {
                Some(alphas(block[0], block[1]))
            } else {
                None
            };
            let mut alpha_bits = 0u64;
            if alpha.is_some() {
                for i in 0..6 {
                    alpha_bits |= (block[2 + i] as u64) << (8 * i);
                }
            }
            let c = if stride == 16 { &block[8..] } else { block };
            let palette = if format != 63 {
                colors(
                    u16::from_le_bytes(c[..2].try_into().unwrap()),
                    u16::from_le_bytes(c[2..4].try_into().unwrap()),
                    stride == 16,
                )
            } else {
                [[0; 4]; 4]
            };
            let bits = if format != 63 {
                u32::from_le_bytes(c[4..8].try_into().unwrap())
            } else {
                0
            };
            for y in 0..4 {
                for x in 0..4 {
                    let px = bx * 4 + x;
                    let py = by * 4 + y;
                    if px >= w || py >= h {
                        continue;
                    }
                    let index = y * 4 + x;
                    let mut rgba = palette[((bits >> (index * 2)) & 3) as usize];
                    if let Some(palette) = alpha {
                        let a = palette[((alpha_bits >> (index * 3)) & 7) as usize];
                        if format == 63 {
                            rgba = [a, a, a, 255];
                        } else {
                            rgba[3] = a;
                        }
                    }
                    out[(py * w + px) * 4..(py * w + px) * 4 + 4].copy_from_slice(&rgba);
                }
            }
        }
    }
    Ok(out)
}
fn encode_alpha(values: &[u8; 16]) -> [u8; 8] {
    let a = *values.iter().max().unwrap();
    let b = *values.iter().min().unwrap();
    let palette = alphas(a, b);
    let mut bits = 0u64;
    for (i, v) in values.iter().enumerate() {
        let index = palette
            .iter()
            .enumerate()
            .min_by_key(|(_, p)| v.abs_diff(**p))
            .unwrap()
            .0;
        bits |= (index as u64) << (i * 3);
    }
    let mut out = [0u8; 8];
    out[0] = a;
    out[1] = b;
    for i in 0..6 {
        out[2 + i] = (bits >> (i * 8)) as u8;
    }
    out
}
fn encode_color(pixels: &[[u8; 4]; 16], alpha: bool) -> [u8; 8] {
    let transparent = alpha && pixels.iter().any(|p| p[3] < 128);
    let mut lo = [255u8; 3];
    let mut hi = [0u8; 3];
    for p in pixels.iter().filter(|p| !transparent || p[3] >= 128) {
        for c in 0..3 {
            lo[c] = lo[c].min(p[c]);
            hi[c] = hi[c].max(p[c]);
        }
    }
    let mut a = to565(hi);
    let mut b = to565(lo);
    if transparent {
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
    } else {
        if a < b {
            std::mem::swap(&mut a, &mut b);
        }
        if a == b {
            if a < u16::MAX {
                a += 1;
            } else {
                b -= 1;
            }
        }
    }
    let palette = colors(a, b, !alpha);
    let mut bits = 0u32;
    for (i, p) in pixels.iter().enumerate() {
        let index = if transparent && p[3] < 128 {
            3
        } else {
            (0..if transparent { 3 } else { 4 })
                .min_by_key(|&j| {
                    (0..3)
                        .map(|c| {
                            let d = p[c] as i32 - palette[j][c] as i32;
                            d * d
                        })
                        .sum::<i32>()
                })
                .unwrap()
        };
        bits |= (index as u32) << (2 * i);
    }
    let mut out = [0; 8];
    out[..2].copy_from_slice(&a.to_le_bytes());
    out[2..4].copy_from_slice(&b.to_le_bytes());
    out[4..].copy_from_slice(&bits.to_le_bytes());
    out
}
pub fn encode(pixels: &[u8], w: usize, h: usize, format: u16) -> Result<Vec<u8>, String> {
    if pixels.len() != w * h * 4 || w == 0 || h == 0 {
        return Err("invalid material pixels".into());
    }
    if format == 31 {
        return Ok(pixels.to_vec());
    }
    if ![57, 58, 61, 62, 63].contains(&format) {
        return Err("unsupported output material format".into());
    }
    let mut out = Vec::new();
    for by in 0..h.div_ceil(4) {
        for bx in 0..w.div_ceil(4) {
            let mut p = [[0u8; 4]; 16];
            for y in 0..4 {
                for x in 0..4 {
                    let i = ((by * 4 + y).min(h - 1) * w + (bx * 4 + x).min(w - 1)) * 4;
                    p[y * 4 + x].copy_from_slice(&pixels[i..i + 4]);
                }
            }
            if format == 63 {
                out.extend_from_slice(&encode_alpha(&p.map(|p| p[0])));
            } else {
                if format == 61 || format == 62 {
                    out.extend_from_slice(&encode_alpha(&p.map(|p| p[3])));
                }
                out.extend_from_slice(&encode_color(&p, format == 57 || format == 58));
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bc_small_mips_round_trip_with_alpha_and_partial_blocks() {
        for fmt in [31, 57, 58, 61, 62, 63] {
            let pixel = if fmt == 63 {
                [160, 160, 160, 255]
            } else {
                [200, 80, 32, 255]
            };
            let data = pixel.repeat(3 * 2);
            let encoded = encode(&data, 3, 2, fmt).unwrap();
            let decoded = decode(&encoded, 3, 2, fmt).unwrap();
            for (a, b) in data.iter().zip(decoded) {
                assert!(a.abs_diff(b) <= 8, "format {fmt}");
            }
        }
        let transparent = [10, 20, 30, 0].repeat(16);
        for fmt in [57, 61, 62] {
            let b = encode(&transparent, 4, 4, fmt).unwrap();
            assert!(decode(&b, 4, 4, fmt)
                .unwrap()
                .chunks_exact(4)
                .all(|p| p[3] == 0));
        }
    }
}
