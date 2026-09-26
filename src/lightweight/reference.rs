//! Reference-derived geometry, never a blanket scale policy. Recipes contain
//! source identities, dimensions, frame selections and rectangular operations;
//! no reference RGB buffers or replacement sprite payloads are embedded.
use super::assets;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Geometry {
    pub width: usize,
    pub height: usize,
    pub frames: usize,
    pub frame_width: usize,
}

fn u32_at(b: &[u8], p: usize) -> Result<usize, String> {
    Ok(u32::from_le_bytes(
        b.get(p..p + 4)
            .ok_or("truncated header")?
            .try_into()
            .unwrap(),
    ) as usize)
}
fn put32(b: &mut [u8], p: usize, n: usize) -> Result<(), String> {
    b[p..p + 4].copy_from_slice(
        &u32::try_from(n)
            .map_err(|_| "sprite dimension overflow")?
            .to_le_bytes(),
    );
    Ok(())
}
pub fn fingerprint(b: &[u8]) -> String {
    let hash = b.iter().fold(0xcbf29ce484222325u64, |h, &v| {
        (h ^ v as u64).wrapping_mul(0x100000001b3)
    });
    format!("{:x}-{:016x}", b.len(), hash)
}
impl Geometry {
    pub fn read(b: &[u8]) -> Result<Self, String> {
        if b.len() < 40
            || ![b"SpA1", b"SPa1"].contains(&b[..4].try_into().unwrap())
            || b[4..6] != 31u16.to_le_bytes()
        {
            return Err("unsupported reference sprite format".into());
        }
        let g = Self {
            width: u32_at(b, 8)?,
            height: u32_at(b, 12)?,
            frames: u32_at(b, 20)?,
            frame_width: u16::from_le_bytes(b[6..8].try_into().unwrap()) as usize,
        };
        if g.height == 0
            || g.frame_width == 0
            || g.frame_width > 65535
            || g.frames == 0
            || g.frames > g.width
            || g.pixel_bytes()? > 512 * 1024 * 1024
            || 40 + g.pixel_bytes()? > b.len()
        {
            return Err("invalid reference sprite dimensions or truncated data".into());
        }
        g.cells()?;
        Ok(g)
    }
    fn pixel_bytes(&self) -> Result<usize, String> {
        self.width
            .checked_mul(self.height)
            .and_then(|v| v.checked_mul(4))
            .ok_or("pixel size overflow".into())
    }
    fn cells(&self) -> Result<Vec<(usize, usize)>, String> {
        let base = self
            .frames
            .checked_mul(self.frame_width)
            .ok_or("frame size overflow")?;
        if self.width < base {
            return Err("invalid frame width".into());
        }
        let extra = self.width - base;
        if extra % self.frames == 0 && extra / self.frames <= 2 {
            let stride = self.frame_width + extra / self.frames;
            return Ok((0..self.frames).map(|i| (i * stride, stride)).collect());
        }
        if self.frames > 1 && extra % (self.frames - 1) == 0 && extra / (self.frames - 1) <= 2 {
            let gap = extra / (self.frames - 1);
            let stride = self.frame_width + gap;
            return Ok((0..self.frames)
                .map(|i| {
                    (
                        i * stride,
                        if i + 1 == self.frames {
                            self.frame_width
                        } else {
                            stride
                        },
                    )
                })
                .collect());
        }
        Err("unsupported frame spacing; reference review required".into())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpritePlan {
    pub source: String,
    pub native_fingerprint: String,
    pub native_geometry: Geometry,
    pub output: Geometry,
    pub frame_indices: Vec<usize>,
    pub clear_rects: Vec<[usize; 4]>,
    pub black_rects: Vec<[usize; 4]>,
    pub policy: String,
    pub discarded_reference_tail_bytes: usize,
    pub native_colors_retained: bool,
}

fn frame_pixels(b: &[u8], g: &Geometry, index: usize) -> Result<(Vec<u8>, usize), String> {
    let cells = g.cells()?;
    let &(x, w) = cells.get(index).ok_or("invalid selected frame")?;
    let mut out = vec![0; w * g.height * 4];
    for y in 0..g.height {
        let from = 40 + (y * g.width + x) * 4;
        out[y * w * 4..(y + 1) * w * 4].copy_from_slice(&b[from..from + w * 4]);
    }
    Ok((out, w))
}

fn render_frames(native: &[u8], plan: &SpritePlan) -> Result<Vec<u8>, String> {
    let g = Geometry::read(native)?;
    if g != plan.native_geometry || fingerprint(native) != plan.native_fingerprint {
        return Err("游戏 sprite 已变化，参考配方需要重新核对；拒绝恢复整张原图".into());
    }
    if plan.frame_indices.len() != plan.output.frames {
        return Err("invalid reference frame plan".into());
    }
    let out_bytes = plan.output.pixel_bytes()?;
    if out_bytes > 512 * 1024 * 1024 {
        return Err("sprite plan too large".into());
    }
    let mut pixels = vec![0; out_bytes];
    for ((x, w), &index) in plan.output.cells()?.into_iter().zip(&plan.frame_indices) {
        let (cell, sw) = frame_pixels(native, &g, index)?;
        let resized = assets::shrink_rgba(&cell, sw, g.height, w, plan.output.height, true)?;
        for y in 0..plan.output.height {
            let to = (y * plan.output.width + x) * 4;
            pixels[to..to + w * 4].copy_from_slice(&resized[y * w * 4..(y + 1) * w * 4]);
        }
    }
    Ok(pixels)
}

/// Run rectangles are merged vertically. They express erase/fill operations,
/// not a compressed copy of reference colour pixels.
fn rectangles(w: usize, h: usize, mut predicate: impl FnMut(usize) -> bool) -> Vec<[usize; 4]> {
    let mut active: BTreeMap<(usize, usize), [usize; 4]> = BTreeMap::new();
    let mut done = Vec::new();
    for y in 0..h {
        let mut next = BTreeMap::new();
        let mut x = 0;
        while x < w {
            if !predicate(y * w + x) {
                x += 1;
                continue;
            }
            let start = x;
            while x < w && predicate(y * w + x) {
                x += 1;
            }
            let key = (start, x - start);
            let rect = if let Some(mut r) = active.remove(&key) {
                r[3] += 1;
                r
            } else {
                [start, y, x - start, 1]
            };
            next.insert(key, rect);
        }
        done.extend(active.into_values());
        active = next;
    }
    done.extend(active.into_values());
    done
}

fn paint(
    pixels: &mut [u8],
    g: &Geometry,
    rects: &[[usize; 4]],
    color: [u8; 4],
) -> Result<(), String> {
    if rects.len() > 500_000 {
        return Err("too many sprite rectangles".into());
    }
    for &[x, y, w, h] in rects {
        if w == 0
            || h == 0
            || x.checked_add(w).is_none_or(|v| v > g.width)
            || y.checked_add(h).is_none_or(|v| v > g.height)
        {
            return Err("sprite rectangle outside reference bounds".into());
        }
        for row in y..y + h {
            for col in x..x + w {
                let p = (row * g.width + col) * 4;
                pixels[p..p + 4].copy_from_slice(&color);
            }
        }
    }
    Ok(())
}

pub fn derive_sprite(
    path: &str,
    native: &[u8],
    reference: &[u8],
    lowend: Option<(&str, &[u8])>,
) -> Result<SpritePlan, String> {
    let output = Geometry::read(reference)?;
    let (source, native, variant) = if let Some((candidate, b)) = lowend {
        if Geometry::read(b).is_ok_and(|g| g == output) {
            (candidate, b, true)
        } else {
            (path, native, false)
        }
    } else {
        (path, native, false)
    };
    // The caller only supplies a lowend candidate when it is valid and matches
    // geometry. Keep the original path if a candidate does not qualify.
    let source = if !variant {
        path.to_string()
    } else {
        source.to_string()
    };
    let ng = Geometry::read(native)?;
    if output.frames > ng.frames || output.height > ng.height {
        return Err("reference adds frames or increases height; review required".into());
    }
    let mut plan = SpritePlan {
        source,
        native_fingerprint: fingerprint(native),
        native_geometry: ng.clone(),
        output: output.clone(),
        frame_indices: (0..output.frames).collect(),
        clear_rects: Vec::new(),
        black_rects: Vec::new(),
        policy: String::new(),
        discarded_reference_tail_bytes: reference.len() - (40 + output.pixel_bytes()?),
        native_colors_retained: false,
    };
    let rp = &reference[40..40 + output.pixel_bytes()?];
    if output.frames == 1 && ng.frames > 1 {
        // Match the retained frame using only reference-visible pixels. A
        // sparse mask cannot use hidden background as evidence for a match.
        let ow = output.cells()?[0].1;
        let mut best = None;
        for index in 0..ng.frames {
            let (cell, sw) = frame_pixels(native, &ng, index)?;
            if ow > sw {
                continue;
            }
            let candidate = assets::shrink_rgba(&cell, sw, ng.height, ow, output.height, true)?;
            let mut score = 0u64;
            let mut samples = 0u64;
            let step = (output.width * output.height / 8192).max(1);
            for p in (0..output.width * output.height).step_by(step) {
                if rp[p * 4 + 3] == 0 {
                    continue;
                }
                samples += 1;
                for c in 0..4 {
                    score += rp[p * 4 + c].abs_diff(candidate[p * 4 + c]) as u64;
                }
            }
            if samples > 0 && best.is_none_or(|(_, old)| score < old) {
                best = Some((index, score));
            }
        }
        plan.frame_indices[0] = best.ok_or("unable to identify retained frame")?.0;
    }
    let mut pixels = render_frames(native, &plan)?;
    if variant {
        // This is a native lowend substitution. Do not bake old-version icon
        // pixel masks into the recipe; the current game's icon remains native.
        plan.policy = "native_lowend_variant".into();
    } else {
        plan.clear_rects = rectangles(output.width, output.height, |p| {
            rp[p * 4 + 3] == 0 && pixels[p * 4 + 3] != 0
        });
        plan.black_rects = rectangles(output.width, output.height, |p| {
            rp[p * 4..p * 4 + 4] == [0, 0, 0, 255] && pixels[p * 4..p * 4 + 4] != [0, 0, 0, 255]
        });
        paint(&mut pixels, &output, &plan.clear_rects, [0; 4])?;
        paint(&mut pixels, &output, &plan.black_rects, [0, 0, 0, 255])?;
        plan.policy = if output.frames < ng.frames {
            "select_reference_frames"
        } else if !plan.clear_rects.is_empty() || !plan.black_rects.is_empty() {
            "reference_erase_and_fill"
        } else {
            "reference_geometry"
        }
        .into();
        // Colour-only edits are explicitly reported. We do not embed arbitrary
        // third-party painted pixels or silently claim byte-identical artwork.
        plan.native_colors_retained = rp
            .chunks_exact(4)
            .zip(pixels.chunks_exact(4))
            .any(|(r, p)| r[3] > 0 && r[..3] != p[..3]);
    }
    Ok(plan)
}

pub fn sprite(native: &[u8], plan: &SpritePlan) -> Result<Vec<u8>, String> {
    let mut pixels = render_frames(native, plan)?;
    if plan.policy == "native_lowend_variant"
        && plan.clear_rects.is_empty()
        && plan.black_rects.is_empty()
        && plan.native_geometry == plan.output
    {
        return Ok(native.to_vec());
    }
    paint(&mut pixels, &plan.output, &plan.clear_rects, [0; 4])?;
    paint(&mut pixels, &plan.output, &plan.black_rects, [0, 0, 0, 255])?;
    let mut out = native[..40].to_vec();
    out[6..8].copy_from_slice(
        &u16::try_from(plan.output.frame_width)
            .map_err(|_| "frame width overflow")?
            .to_le_bytes(),
    );
    put32(&mut out, 8, plan.output.width)?;
    put32(&mut out, 12, plan.output.height)?;
    put32(&mut out, 20, plan.output.frames)?;
    put32(&mut out, 32, pixels.len())?;
    put32(&mut out, 36, 4)?;
    out.extend_from_slice(&pixels);
    Ok(out)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TexturePlan {
    pub width: usize,
    pub height: usize,
    pub native_fingerprint: String,
    pub format: u16,
    pub mip_count: usize,
}
pub fn derive_texture(native: &[u8], reference: &[u8]) -> Result<TexturePlan, String> {
    if reference.len() < 36 || reference[..4] != [0x3c, 0x44, 0x45, 0x28] {
        return Err("unrecognised reference texture".into());
    }
    let plan = TexturePlan {
        width: u32_at(reference, 8)?,
        height: u32_at(reference, 12)?,
        native_fingerprint: fingerprint(native),
        format: u16::from_le_bytes(reference[4..6].try_into().unwrap()),
        mip_count: u32_at(reference, 28)?,
    };
    let generated = texture_exact(native, &plan)?;
    if u32_at(&generated, 8)? != plan.width || u32_at(&generated, 12)? != plan.height {
        return Err("reference texture dimensions cannot be reproduced from native mips".into());
    }
    Ok(plan)
}
pub fn texture(native: &[u8], plan: &TexturePlan, override_size: usize) -> Result<Vec<u8>, String> {
    if fingerprint(native) != plan.native_fingerprint {
        return Err("原版纹理已变化，需要重新核对参考配方".into());
    }
    if override_size == 0 {
        texture_exact(native, plan)
    } else {
        assets::texture(native, override_size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn strip(colors: &[[u8; 4]]) -> Vec<u8> {
        let mut b = vec![0; 40];
        b[..4].copy_from_slice(b"SpA1");
        b[4] = 31;
        b[6] = 1;
        put32(&mut b, 8, colors.len()).unwrap();
        put32(&mut b, 12, 1).unwrap();
        put32(&mut b, 20, colors.len()).unwrap();
        for c in colors {
            b.extend(c);
        }
        b
    }
    #[test]
    fn reference_clear_black_and_single_frame_are_preserved() {
        let native = strip(&[[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]]);
        let mut reference = strip(&[[0; 4], [0, 0, 0, 255], [0, 0, 255, 255]]);
        let plan = derive_sprite("data/test.sprite", &native, &reference, None).unwrap();
        assert_eq!(&sprite(&native, &plan).unwrap()[40..], &reference[40..]);
        reference = strip(&[[0, 255, 0, 255]]);
        reference.extend([99; 40]);
        let plan = derive_sprite("data/test.sprite", &native, &reference, None).unwrap();
        assert_eq!(plan.frame_indices, vec![1]);
        assert_eq!(plan.discarded_reference_tail_bytes, 40);
        assert_eq!(sprite(&native, &plan).unwrap().len(), 44);
        let mut changed = native.clone();
        changed[40] = 0;
        assert!(sprite(&changed, &plan).is_err());
    }
    #[test]
    fn native_lowend_is_used_without_uniform_scaling() {
        let native = strip(&[[255; 4], [128; 4]]);
        let low = strip(&[[255; 4]]);
        let plan = derive_sprite(
            "data/test.sprite",
            &native,
            &low,
            Some(("data/test.lowend.sprite", &low)),
        )
        .unwrap();
        assert_eq!(plan.source, "data/test.lowend.sprite");
        assert_eq!(sprite(&low, &plan).unwrap(), low);
    }
    #[test]
    fn non_square_texture_geometry_and_mips_follow_reference() {
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
        let plan = TexturePlan {
            width: 8,
            height: 2,
            native_fingerprint: fingerprint(&b),
            format: 61,
            mip_count: 4,
        };
        let out = texture(&b, &plan, 0).unwrap();
        assert_eq!(u32_at(&out, 8).unwrap(), 8);
        assert_eq!(u32_at(&out, 12).unwrap(), 2);
        assert_eq!(u32_at(&out, 28).unwrap(), 4);
        assert_eq!(assets::texture(&out, 8).unwrap(), out);
    }
}

fn texture_exact(native: &[u8], plan: &TexturePlan) -> Result<Vec<u8>, String> {
    if plan.width == 0
        || plan.height == 0
        || plan.width > 16384
        || plan.height > 16384
        || plan.width * plan.height > 16 * 1024 * 1024
        || plan.mip_count == 0
        || plan.mip_count > 16
    {
        return Err("invalid reference texture header".into());
    }
    let max_mips = (usize::BITS - plan.width.max(plan.height).leading_zeros()) as usize;
    if plan.mip_count > max_mips {
        return Err("invalid reference mip count".into());
    }
    let small = assets::texture(native, plan.width.max(plan.height).next_power_of_two())?;
    let w = u32_at(&small, 8)?;
    let h = u32_at(&small, 12)?;
    let count = u32_at(&small, 28)?;
    let format = u16::from_le_bytes(small[4..6].try_into().unwrap());
    let payload = |i: usize| -> Result<&[u8], String> {
        let len = u32_at(&small, 36 + i * 8)?;
        let start = 40 + i * 8 + u32_at(&small, 40 + i * 8)?;
        small
            .get(start..start + len)
            .ok_or("bad native mip range".into())
    };
    let levels = if w == plan.width
        && h == plan.height
        && format == plan.format
        && count >= plan.mip_count
    {
        (0..plan.mip_count)
            .map(|i| payload(i).map(|p| p.to_vec()))
            .collect::<Result<Vec<_>, _>>()?
    } else {
        let source = super::blocks::decode(payload(0)?, w, h, format)?;
        let mut rgba = assets::shrink_rgba(&source, w, h, plan.width, plan.height, false)?;
        let mut levels = Vec::new();
        let (mut cw, mut ch) = (plan.width, plan.height);
        for i in 0..plan.mip_count {
            levels.push(super::blocks::encode(&rgba, cw, ch, plan.format)?);
            if i + 1 < plan.mip_count {
                let (nw, nh) = ((cw / 2).max(1), (ch / 2).max(1));
                rgba = assets::shrink_rgba(&rgba, cw, ch, nw, nh, false)?;
                (cw, ch) = (nw, nh);
            }
        }
        levels
    };
    let mut out = small[..36].to_vec();
    out[4..6].copy_from_slice(&plan.format.to_le_bytes());
    out[7] = levels.len() as u8;
    put32(&mut out, 8, plan.width)?;
    put32(&mut out, 12, plan.height)?;
    put32(&mut out, 28, levels.len())?;
    out.resize(36 + levels.len() * 8, 0);
    for (i, level) in levels.iter().enumerate() {
        let start = out.len();
        put32(&mut out, 36 + i * 8, level.len())?;
        put32(&mut out, 40 + i * 8, start - (40 + i * 8))?;
        out.extend_from_slice(level);
    }
    Ok(out)
}
