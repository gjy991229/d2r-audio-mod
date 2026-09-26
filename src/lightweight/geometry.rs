//! Native sprite geometry validation only; no reference-derived plans.
use serde::{Deserialize, Serialize};
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
