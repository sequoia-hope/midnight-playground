//! The ground under a corridor: a grid of real heights in game metres,
//! written by `tools/atlas/ground.py` (docs/vision/peninsula.md, stage 2)
//! and included at compile time by the level that drives on it.
//!
//! Little-endian: `b"MPGROUND"`, u32 width, u32 height, f32 step, f32 x0,
//! f32 z0 (game metres; cell (0, 0)'s centre, the north-west corner), u32
//! length of the zlib stream, then zlib of i16 heights in decimetres, row by
//! row from the north.

/// A height grid with bilinear sampling, clamped at its edges.
#[derive(Clone, Debug, PartialEq)]
pub struct HeightGrid {
    pub x0: f64,
    pub z0: f64,
    pub step: f64,
    pub w: usize,
    pub h: usize,
    /// Heights in metres, row by row from the north.
    pub values: Vec<f32>,
}

impl HeightGrid {
    pub fn parse(bytes: &[u8]) -> Result<HeightGrid, String> {
        if bytes.len() < 32 || &bytes[0..8] != b"MPGROUND" {
            return Err("ground: not an MPGROUND file".to_owned());
        }
        let u32_at =
            |o: usize| u32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]);
        let f32_at =
            |o: usize| f32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]);
        let w = u32_at(8) as usize;
        let h = u32_at(12) as usize;
        let step = f64::from(f32_at(16));
        let x0 = f64::from(f32_at(20));
        let z0 = f64::from(f32_at(24));
        let len = u32_at(28) as usize;
        if w < 2
            || h < 2
            || step.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater)
            || w.checked_mul(h).is_none()
        {
            return Err("ground: bad grid size".to_owned());
        }
        let stream = bytes
            .get(32..32 + len)
            .ok_or_else(|| "ground: short file".to_owned())?;
        let raw = miniz_oxide::inflate::decompress_to_vec_zlib(stream)
            .map_err(|e| format!("ground: inflating: {e:?}"))?;
        if raw.len() != w * h * 2 {
            return Err(format!(
                "ground: {} bytes of heights for {w} x {h} cells",
                raw.len()
            ));
        }
        let values = raw
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| f32::from(i16::from_le_bytes(*c)) / 10.0)
            .collect();
        Ok(HeightGrid {
            x0,
            z0,
            step,
            w,
            h,
            values,
        })
    }

    /// The height at (x, z), bilinear between the cells' centres, the
    /// edges' values carried on outside the grid.
    pub fn height(&self, x: f64, z: f64) -> f64 {
        let (w, h) = (self.w as f64, self.h as f64);
        let gx = ((x - self.x0) / self.step).clamp(0.0, w - 1.001);
        let gz = ((z - self.z0) / self.step).clamp(0.0, h - 1.001);
        let (i, j) = (gx.floor(), gz.floor());
        let (tx, tz) = (gx - i, gz - j);
        let k = j as usize * self.w + i as usize;
        let v = |k: usize| f64::from(self.values[k]);
        let a = v(k) + (v(k + 1) - v(k)) * tx;
        let b = v(k + self.w) + (v(k + self.w + 1) - v(k + self.w)) * tx;
        a + (b - a) * tz
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> HeightGrid {
        // 3 x 2 cells of 10 m from (0, 0): a slope rising east.
        let heights: [i16; 6] = [0, 100, 200, 0, 100, 200];
        let mut raw = Vec::new();
        for v in heights {
            raw.extend_from_slice(&v.to_le_bytes());
        }
        let z = miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6);
        let mut b = b"MPGROUND".to_vec();
        b.extend_from_slice(&3u32.to_le_bytes());
        b.extend_from_slice(&2u32.to_le_bytes());
        b.extend_from_slice(&10f32.to_le_bytes());
        b.extend_from_slice(&0f32.to_le_bytes());
        b.extend_from_slice(&0f32.to_le_bytes());
        b.extend_from_slice(&(z.len() as u32).to_le_bytes());
        b.extend_from_slice(&z);
        HeightGrid::parse(&b).unwrap()
    }

    #[test]
    fn samples_bilinearly_and_clamps() {
        let g = grid();
        assert_eq!(g.height(0.0, 0.0), 0.0);
        assert!((g.height(5.0, 5.0) - 5.0).abs() < 1e-9);
        assert!((g.height(15.0, 3.0) - 15.0).abs() < 1e-9);
        assert!((g.height(-50.0, 100.0) - 0.0).abs() < 1e-9);
        assert!((g.height(500.0, -100.0) - 20.0).abs() < 0.05);
    }

    #[test]
    fn rejects_other_files() {
        assert!(HeightGrid::parse(b"MPATLAS1 not this").is_err());
    }
}
