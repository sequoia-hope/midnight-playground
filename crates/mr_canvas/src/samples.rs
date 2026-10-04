//! The per-sample pixels of a multisampled render pass (DECISIONS D651).
//!
//! Once Chrome's GPU canvas draws something it multisamples (a stroke that
//! is not a single line segment), Skia renders the whole pass into an 8×
//! multisampled target: every later draw blends into each sample on its
//! own, and the pixels are the samples' average when the pass is resolved
//! (when the canvas is read). So two strokes that each cover half a pixel
//! cover it fully if they cover different samples, where blending their
//! coverage would leave a quarter uncovered.
//!
//! Most pixels have all eight samples equal; only those that differ are
//! stored, and the canvas's pixmap always holds the resolved pixel.

/// Samples of the pixels whose samples differ, premultiplied RGBA8.
pub(crate) struct Samples {
    /// Per pixel: 0 when its samples all equal the pixel, else 1 + its
    /// index in `pool`.
    index: Vec<u32>,
    pool: Vec<[[u8; 4]; 8]>,
}

impl Samples {
    pub(crate) fn new(pixels: usize) -> Samples {
        Samples {
            index: vec![0; pixels],
            pool: Vec::new(),
        }
    }

    /// The samples of pixel `i`, if they differ.
    pub(crate) fn get_mut(&mut self, i: usize) -> Option<&mut [[u8; 4]; 8]> {
        match self.index[i] {
            0 => None,
            k => Some(&mut self.pool[k as usize - 1]),
        }
    }

    /// The samples of pixel `i`, made from its value if they were equal.
    pub(crate) fn get_or_split(&mut self, i: usize, px: [u8; 4]) -> &mut [[u8; 4]; 8] {
        if self.index[i] == 0 {
            self.pool.push([px; 8]);
            self.index[i] = self.pool.len() as u32;
        }
        &mut self.pool[self.index[i] as usize - 1]
    }
}

/// The pixel of eight samples as the GPU resolves them: the mean of each
/// channel, a tie rounded down (Chrome's half-covered opaque pixel is 127).
pub(crate) fn resolve(s: &[[u8; 4]; 8]) -> [u8; 4] {
    let mut out = [0u8; 4];
    for (c, o) in out.iter_mut().enumerate() {
        let sum: u32 = s.iter().map(|p| p[c] as u32).sum();
        *o = ((sum + 3) >> 3) as u8;
    }
    out
}
