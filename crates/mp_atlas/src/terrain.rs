//! `terrain.bin`: heights and land cover on a grid over the whole box.
//!
//! Little-endian: `b"MPATLAS1"`, u32 width, u32 height, f32 step, f32 x0,
//! f32 z0 (the centre of cell (0, 0), the north-west corner), u32 reserved,
//! then i16 heights in decimetres row by row from the north, then a u8
//! land cover class per cell in the same order (`tools/atlas/build.py`).

use crate::geo::V2;

/// Land cover, as the build script numbers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Cover {
    Sea = 0,
    Water = 1,
    Urban = 2,
    Crop = 3,
    Grass = 4,
    Shrub = 5,
    Forest = 6,
    Barren = 7,
    Wetland = 8,
    Sand = 9,
}

impl Cover {
    pub const ALL: [Cover; 10] = [
        Cover::Sea,
        Cover::Water,
        Cover::Urban,
        Cover::Crop,
        Cover::Grass,
        Cover::Shrub,
        Cover::Forest,
        Cover::Barren,
        Cover::Wetland,
        Cover::Sand,
    ];

    pub fn from_u8(v: u8) -> Cover {
        Cover::ALL.get(v as usize).copied().unwrap_or(Cover::Grass)
    }

    pub fn name(self) -> &'static str {
        match self {
            Cover::Sea => "sea",
            Cover::Water => "water",
            Cover::Urban => "urban",
            Cover::Crop => "crop",
            Cover::Grass => "grass",
            Cover::Shrub => "shrub",
            Cover::Forest => "forest",
            Cover::Barren => "barren",
            Cover::Wetland => "wetland",
            Cover::Sand => "sand",
        }
    }

    pub fn is_water(self) -> bool {
        matches!(self, Cover::Sea | Cover::Water)
    }
}

pub struct Terrain {
    pub width: usize,
    pub height: usize,
    pub step: f64,
    pub x0: f64,
    pub z0: f64,
    /// Heights (m), row by row from the north.
    pub h: Vec<f32>,
    pub cover: Vec<u8>,
}

impl Terrain {
    pub fn from_bytes(b: &[u8]) -> Result<Terrain, String> {
        if b.len() < 32 || &b[0..8] != b"MPATLAS1" {
            return Err("terrain.bin: not an atlas terrain".into());
        }
        let u32_at = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        let f32_at = |o: usize| f32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        let (w, h) = (u32_at(8) as usize, u32_at(12) as usize);
        let n = w * h;
        if b.len() != 32 + n * 3 {
            return Err(format!(
                "terrain.bin: {} bytes for a {w} x {h} grid, expected {}",
                b.len(),
                32 + n * 3
            ));
        }
        let hs = (0..n)
            .map(|i| {
                let o = 32 + i * 2;
                f32::from(i16::from_le_bytes([b[o], b[o + 1]])) / 10.0
            })
            .collect();
        Ok(Terrain {
            width: w,
            height: h,
            step: f64::from(f32_at(16)),
            x0: f64::from(f32_at(20)),
            z0: f64::from(f32_at(24)),
            h: hs,
            cover: b[32 + n * 2..].to_vec(),
        })
    }

    /// Grid coordinates of a local point (cell centres at integers).
    fn grid(&self, p: V2) -> (f64, f64) {
        ((p.x - self.x0) / self.step, (p.z - self.z0) / self.step)
    }

    fn at(&self, i: isize, j: isize) -> f64 {
        let i = i.clamp(0, self.width as isize - 1) as usize;
        let j = j.clamp(0, self.height as isize - 1) as usize;
        f64::from(self.h[j * self.width + i])
    }

    /// The ground's height (m) at a point, bilinear between cell centres.
    pub fn height_at(&self, p: V2) -> f64 {
        let (gx, gz) = self.grid(p);
        let (i, j) = (gx.floor(), gz.floor());
        let (fx, fz) = (gx - i, gz - j);
        let (i, j) = (i as isize, j as isize);
        let top = self.at(i, j) * (1.0 - fx) + self.at(i + 1, j) * fx;
        let bot = self.at(i, j + 1) * (1.0 - fx) + self.at(i + 1, j + 1) * fx;
        top * (1.0 - fz) + bot * fz
    }

    /// The land cover of the cell a point is in.
    pub fn cover_at(&self, p: V2) -> Cover {
        let (gx, gz) = self.grid(p);
        let i = (gx + 0.5).floor().clamp(0.0, self.width as f64 - 1.0) as usize;
        let j = (gz + 0.5).floor().clamp(0.0, self.height as f64 - 1.0) as usize;
        Cover::from_u8(self.cover[j * self.width + i])
    }

    /// The centre of cell (i, j).
    pub fn cell(&self, i: usize, j: usize) -> V2 {
        V2::new(
            self.x0 + i as f64 * self.step,
            self.z0 + j as f64 * self.step,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny() -> Terrain {
        let mut b = b"MPATLAS1".to_vec();
        for v in [2u32, 2] {
            b.extend(v.to_le_bytes());
        }
        for v in [10f32, 0.0, 0.0] {
            b.extend(v.to_le_bytes());
        }
        b.extend(0u32.to_le_bytes());
        for dm in [0i16, 100, 200, 300] {
            b.extend(dm.to_le_bytes());
        }
        b.extend([0u8, 3, 6, 2]);
        Terrain::from_bytes(&b).unwrap()
    }

    #[test]
    fn heights_are_bilinear_and_cover_nearest() {
        let t = tiny();
        assert_eq!(t.height_at(V2::new(0.0, 0.0)), 0.0);
        assert_eq!(t.height_at(V2::new(10.0, 10.0)), 30.0);
        assert_eq!(t.height_at(V2::new(5.0, 5.0)), 15.0);
        assert_eq!(t.cover_at(V2::new(1.0, 1.0)), Cover::Sea);
        assert_eq!(t.cover_at(V2::new(9.0, 1.0)), Cover::Crop);
        assert_eq!(t.cover_at(V2::new(1.0, 9.0)), Cover::Forest);
    }
}
