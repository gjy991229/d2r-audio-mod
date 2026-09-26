//! Lossless mip selection and independent RGBA sprite downsampling.
//! Byte layouts are documented in docs/lightweight-formats.md. No third-party
//! converter source or mod image data is embedded in this module.

fn u16_at(b: &[u8], p: usize) -> Result<u16, String> {
    Ok(u16::from_le_bytes(
        b.get(p..p + 2).ok_or("truncated u16")?.try_into().unwrap(),
    ))
}
fn u32_at(b: &[u8], p: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(
        b.get(p..p + 4).ok_or("truncated u32")?.try_into().unwrap(),
    ))
}
fn put32(b: &mut [u8], p: usize, value: usize) -> Result<(), String> {
    let value = u32::try_from(value).map_err(|_| "asset size overflow")?;
    b.get_mut(p..p + 4)
        .ok_or("truncated header")?
        .copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn pixel_size(w: usize, h: usize) -> Result<usize, String> {
    w.checked_mul(h)
        .and_then(|n| n.checked_mul(4))
        .filter(|&n| n > 0 && n <= 512 * 1024 * 1024)
        .ok_or("invalid or excessive pixel dimensions".into())
}

/// Area averaging, alpha-weighted to avoid dark transparent borders. Frames
/// are resized independently: adjacent animation frames never bleed together.
pub(super) fn shrink_rgba(
    src: &[u8],
    w: usize,
    h: usize,
    nw: usize,
    nh: usize,
    alpha_weighted: bool,
) -> Result<Vec<u8>, String> {
    if src.len() != pixel_size(w, h)? {
        return Err("invalid RGBA dimensions".into());
    }
    let mut out = vec![0; pixel_size(nw, nh)?];
    for y in 0..nh {
        let y0 = y * h / nh;
        let y1 = ((y + 1) * h / nh).max(y0 + 1);
        for x in 0..nw {
            let x0 = x * w / nw;
            let x1 = ((x + 1) * w / nw).max(x0 + 1);
            let mut rgba = [0u64; 4];
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let p = (sy * w + sx) * 4;
                    let a = src[p + 3] as u64;
                    for c in 0..3 {
                        rgba[c] += src[p + c] as u64 * if alpha_weighted { a } else { 1 };
                    }
                    rgba[3] += a;
                }
            }
            let p = (y * nw + x) * 4;
            let weight = if alpha_weighted {
                rgba[3]
            } else {
                ((x1 - x0) * (y1 - y0)) as u64
            };
            if weight != 0 {
                for c in 0..3 {
                    out[p + c] = ((rgba[c] + weight / 2) / weight) as u8;
                }
            }
            out[p + 3] = ((rgba[3] + ((x1 - x0) * (y1 - y0)) as u64 / 2)
                / ((x1 - x0) * (y1 - y0)) as u64) as u8;
        }
    }
    Ok(out)
}

pub fn texture(bytes: &[u8], max_side: usize) -> Result<Vec<u8>, String> {
    if max_side == 0 || max_side > 16384 || !max_side.is_power_of_two() {
        return Err("unsupported target texture size".into());
    }
    if bytes.get(..4) != Some(&[0x3c, 0x44, 0x45, 0x28]) {
        return Err("unsupported texture magic".into());
    }
    let w = u32_at(bytes, 8)? as usize;
    let h = u32_at(bytes, 12)? as usize;
    let count = u32_at(bytes, 28)? as usize;
    if w == 0
        || h == 0
        || w > 32768
        || h > 32768
        || !(1..=16).contains(&count)
        || u32_at(bytes, 16)? != 1
    {
        return Err("unsupported texture dimensions, depth or mip count".into());
    }
    let format = u16_at(bytes, 4)?;
    let table_end = 36 + count * 8;
    if bytes.len() < table_end {
        return Err("truncated texture mip table".into());
    }
    let mut mips = Vec::new();
    let mut previous_end = table_end;
    for i in 0..count {
        let size = u32_at(bytes, 36 + i * 8)? as usize;
        let offset_pos = 40 + i * 8;
        let start = offset_pos
            .checked_add(u32_at(bytes, offset_pos)? as usize)
            .ok_or("mip offset overflow")?;
        let end = start.checked_add(size).ok_or("mip size overflow")?;
        let mw = (w >> i).max(1);
        let mh = (h >> i).max(1);
        let expected = match format {
            31 => pixel_size(mw, mh)?,
            57 | 58 | 63 => mw.div_ceil(4) * mh.div_ceil(4) * 8,
            61 | 62 => mw.div_ceil(4) * mh.div_ceil(4) * 16,
            _ => return Err(format!("unsupported texture format {format}")),
        };
        if size != expected || start < previous_end || end > bytes.len() {
            return Err("invalid mip range or compressed size".into());
        }
        mips.push(&bytes[start..end]);
        previous_end = end;
    }
    if previous_end != bytes.len() {
        return Err("unsupported texture trailing data".into());
    }
    let selected = (0..count).find(|&i| (w >> i).max(1).max((h >> i).max(1)) <= max_side);
    let (nw, nh, payloads): (usize, usize, Vec<Vec<u8>>) = if let Some(i) = selected {
        (
            (w >> i).max(1),
            (h >> i).max(1),
            mips[i..].iter().map(|p| p.to_vec()).collect(),
        )
    } else if format == 31 {
        let nw = w.min(max_side);
        let nh = h.min(max_side);
        // Texture alpha can be a material channel, not opacity. Keep all
        // channels independent; only UI sprites use premultiplied averaging.
        let mut payloads = vec![shrink_rgba(mips[0], w, h, nw, nh, false)?];
        let (mut cw, mut ch) = (nw, nh);
        while cw > 1 || ch > 1 {
            let (dw, dh) = ((cw / 2).max(1), (ch / 2).max(1));
            payloads.push(shrink_rgba(
                payloads.last().unwrap(),
                cw,
                ch,
                dw,
                dh,
                false,
            )?);
            (cw, ch) = (dw, dh);
        }
        (nw, nh, payloads)
    } else {
        return Err("compressed texture has no sufficiently small native mip".into());
    };
    let mut out = bytes[..36].to_vec();
    put32(&mut out, 8, nw)?;
    put32(&mut out, 12, nh)?;
    put32(&mut out, 28, payloads.len())?;
    // Observed high byte mirrors mip count; do not silently rewrite unknown
    // header variants as if they used the same layout.
    if out[7] != count as u8 {
        return Err("unsupported texture mip flag".into());
    }
    out[7] = payloads.len() as u8;
    out.resize(36 + payloads.len() * 8, 0);
    for (i, payload) in payloads.iter().enumerate() {
        let start = out.len();
        put32(&mut out, 36 + i * 8, payload.len())?;
        put32(&mut out, 40 + i * 8, start - (40 + i * 8))?;
        out.extend_from_slice(payload);
    }
    Ok(out)
}

#[cfg(test)]
pub fn sprite(bytes: &[u8], divisor: usize) -> Result<Vec<u8>, String> {
    if ![1, 2, 4, 8].contains(&divisor) {
        return Err("unsupported sprite divisor".into());
    }
    if bytes.len() < 40
        || (bytes.get(..4) != Some(b"SpA1") && bytes.get(..4) != Some(b"SPa1"))
        || u16_at(bytes, 4)? != 31
    {
        return Err("unsupported sprite format (expected SpA1 RGBA31)".into());
    }
    let w = u32_at(bytes, 8)? as usize;
    let h = u32_at(bytes, 12)? as usize;
    let frames = u32_at(bytes, 20)? as usize;
    let frame_w = u16_at(bytes, 6)? as usize;
    if frames == 0 || frame_w == 0 || frames > w || bytes.len() != 40 + pixel_size(w, h)? {
        return Err("unsupported sprite frame table or payload length".into());
    }
    let (padding, trailing) =
        if w % frames == 0 && w / frames >= frame_w && w / frames - frame_w <= 2 {
            (w / frames - frame_w, true)
        } else if frames > 1
            && w >= frames * frame_w
            && (w - frames * frame_w) % (frames - 1) == 0
            && (w - frames * frame_w) / (frames - 1) <= 2
        {
            ((w - frames * frame_w) / (frames - 1), false)
        } else {
            return Err("unsupported sprite frame spacing".into());
        };
    let stride = frame_w + padding;
    let new_frame = (frame_w / divisor).max(1);
    let new_stride = new_frame + padding;
    let nw = frames * new_stride - if trailing { 0 } else { padding };
    let nh = (h / divisor).max(1);
    let mut pixels = vec![0u8; pixel_size(nw, nh)?];
    for frame in 0..frames {
        let last_gap = if !trailing && frame + 1 == frames {
            padding
        } else {
            0
        };
        let cell_w = stride - last_gap;
        let new_cell_w = new_stride - last_gap;
        let mut cell = vec![0; pixel_size(cell_w, h)?];
        for y in 0..h {
            let from = 40 + (y * w + frame * stride) * 4;
            cell[y * cell_w * 4..(y + 1) * cell_w * 4]
                .copy_from_slice(&bytes[from..from + cell_w * 4]);
        }
        let resized = shrink_rgba(&cell, cell_w, h, new_cell_w, nh, true)?;
        for y in 0..nh {
            let to = (y * nw + frame * new_stride) * 4;
            pixels[to..to + new_cell_w * 4]
                .copy_from_slice(&resized[y * new_cell_w * 4..(y + 1) * new_cell_w * 4]);
        }
    }
    let mut out = bytes[..40].to_vec();
    out[6..8].copy_from_slice(&(new_frame as u16).to_le_bytes());
    put32(&mut out, 8, nw)?;
    put32(&mut out, 12, nh)?;
    put32(&mut out, 32, pixels.len())?;
    put32(&mut out, 36, 4)?;
    out.extend_from_slice(&pixels);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn texture_rebases_relative_offsets_and_keeps_compressed_mip_bytes() {
        let mut b = vec![0; 36 + 3 * 8];
        b[..4].copy_from_slice(&[0x3c, 0x44, 0x45, 0x28]);
        b[4] = 57;
        b[6] = 2;
        b[7] = 3;
        for (p, n) in [(8, 4), (12, 4), (16, 1), (28, 3), (32, 4)] {
            put32(&mut b, p, n).unwrap();
        }
        for i in 0..3 {
            let start = b.len();
            put32(&mut b, 36 + i * 8, 8).unwrap();
            put32(&mut b, 40 + i * 8, start - (40 + i * 8)).unwrap();
            b.extend_from_slice(&[i as u8; 8]);
        }
        let out = texture(&b, 2).unwrap();
        assert_eq!(u32_at(&out, 8).unwrap(), 2);
        assert_eq!(out[7], 2);
        assert_eq!(&out[52..], &[vec![1; 8], vec![2; 8]].concat());
        assert_eq!(texture(&out, 2).unwrap(), out);
        let mut broken = b.clone();
        put32(&mut broken, 40, 0).unwrap();
        assert!(texture(&broken, 2).is_err());
    }
    #[test]
    fn sprite_downsampling_keeps_frame_boundaries() {
        let mut b = vec![0; 40];
        b[..4].copy_from_slice(b"SpA1");
        b[4] = 31;
        b[6] = 4;
        for (p, n) in [(8, 8), (12, 4), (20, 2)] {
            put32(&mut b, p, n).unwrap();
        }
        for _ in 0..4 {
            for x in 0..8 {
                b.extend_from_slice(if x < 4 {
                    &[255, 0, 0, 255]
                } else {
                    &[0, 0, 255, 255]
                });
            }
        }
        let out = sprite(&b, 2).unwrap();
        assert_eq!(u32_at(&out, 8).unwrap(), 4);
        assert_eq!(u32_at(&out, 20).unwrap(), 2);
        assert_eq!(&out[40..48], &[255, 0, 0, 255, 255, 0, 0, 255]);
        assert_eq!(&out[48..56], &[0, 0, 255, 255, 0, 0, 255, 255]);
        assert!(sprite(&b[..b.len() - 1], 2).is_err());
    }
    #[test]
    fn sprite_between_frame_gutters_survive_odd_atlas_widths() {
        let mut b = vec![0; 40];
        b[..4].copy_from_slice(b"SpA1");
        b[4] = 31;
        b[6] = 4;
        for (p, n) in [(8, 9), (12, 4), (20, 2)] {
            put32(&mut b, p, n).unwrap();
        }
        b.extend(vec![255; 9 * 4 * 4]);
        let out = sprite(&b, 2).unwrap();
        assert_eq!(u32_at(&out, 8).unwrap(), 5);
        assert_eq!(u16_at(&out, 6).unwrap(), 2);
        assert_eq!(out.len(), 40 + 5 * 2 * 4);
        assert!(sprite(&b, 0).is_err());
    }
    #[test]
    fn transparent_pixels_do_not_add_dark_edges() {
        let out = shrink_rgba(&[255, 0, 0, 255, 0, 0, 0, 0], 2, 1, 1, 1, true).unwrap();
        assert_eq!(out, vec![255, 0, 0, 128]);
        let material =
            shrink_rgba(&[128, 128, 255, 0, 128, 128, 255, 0], 2, 1, 1, 1, false).unwrap();
        assert_eq!(material, vec![128, 128, 255, 0]);
    }
    #[test]
    fn raw_texture_without_mips_can_be_reduced() {
        let mut b = vec![0; 44];
        b[..4].copy_from_slice(&[0x3c, 0x44, 0x45, 0x28]);
        b[4] = 31;
        b[6] = 2;
        b[7] = 1;
        for (p, n) in [
            (8, 8),
            (12, 8),
            (16, 1),
            (28, 1),
            (32, 4),
            (36, 256),
            (40, 4),
        ] {
            put32(&mut b, p, n).unwrap();
        }
        b.extend(vec![255; 256]);
        let out = texture(&b, 4).unwrap();
        assert_eq!(u32_at(&out, 8).unwrap(), 4);
        assert_eq!(u32_at(&out, 28).unwrap(), 3);
        assert_eq!(texture(&out, 4).unwrap(), out);
        assert!(texture(&b, 0).is_err());
    }
}
