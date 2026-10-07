use iris_core::{EncodedImage, Samples};
use rayon::prelude::*;

/// 256-bin histograms of an output-encoded (sRGB) image.
#[derive(Clone, Debug, PartialEq)]
pub struct Histogram {
    pub red: [u32; 256],
    pub green: [u32; 256],
    pub blue: [u32; 256],
    pub luminance: [u32; 256],
    pub pixels: u64,
}

impl Default for Histogram {
    fn default() -> Self {
        Self { red: [0; 256], green: [0; 256], blue: [0; 256], luminance: [0; 256], pixels: 0 }
    }
}

impl Histogram {
    fn add(&mut self, r: usize, g: usize, b: usize) {
        self.red[r] += 1;
        self.green[g] += 1;
        self.blue[b] += 1;
        self.luminance[(54 * r + 183 * g + 19 * b) >> 8] += 1; // Rec.709 luma weights
    }

    fn merge(mut self, other: &Histogram) -> Histogram {
        for k in 0..256 {
            self.red[k] += other.red[k];
            self.green[k] += other.green[k];
            self.blue[k] += other.blue[k];
            self.luminance[k] += other.luminance[k];
        }
        self
    }

    pub fn compute(image: &EncodedImage) -> Histogram {
        const CHUNK: usize = 3 * 65536;
        // One histogram per chunk, on the heap: folding 4 KB values through rayon's
        // work-stealing recursion can overflow a worker's stack.
        let parts: Vec<Box<Histogram>> = match &image.samples {
            Samples::Eight(d) => d
                .par_chunks(CHUNK)
                .map(|chunk| {
                    let mut h = Box::<Histogram>::default();
                    for px in chunk.as_chunks::<3>().0 {
                        h.add(px[0].into(), px[1].into(), px[2].into());
                    }
                    h
                })
                .collect(),
            Samples::Sixteen(d) => d
                .par_chunks(CHUNK)
                .map(|chunk| {
                    let mut h = Box::<Histogram>::default();
                    for px in chunk.as_chunks::<3>().0 {
                        h.add(usize::from(px[0] >> 8), usize::from(px[1] >> 8), usize::from(px[2] >> 8));
                    }
                    h
                })
                .collect(),
        };
        let mut total = parts.into_iter().fold(Histogram::default(), |total, part| total.merge(&part));
        total.pixels = (image.width * image.height) as u64;
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_every_pixel() {
        let mut data = vec![100u8; 4 * 2 * 3];
        data[0] = 255; // one pixel with a red channel at 255
        let image = EncodedImage { width: 4, height: 2, samples: Samples::Eight(data) };
        let h = Histogram::compute(&image);
        assert_eq!(h.pixels, 8);
        assert_eq!(h.green[100], 8);
        assert_eq!(h.red[100], 7);
        assert_eq!(h.red[255], 1);
        assert_eq!(h.luminance[100], 7);
    }

    #[test]
    fn large_images_are_counted_in_chunks() {
        let (w, hh) = (500, 300); // more than one chunk
        let mut data = vec![0u16; w * hh * 3];
        for (i, v) in data.iter_mut().enumerate() {
            *v = ((i / 3) % 256 * 257) as u16;
        }
        let h = Histogram::compute(&EncodedImage { width: w, height: hh, samples: Samples::Sixteen(data) });
        assert_eq!(h.pixels, (w * hh) as u64);
        assert_eq!(h.red.iter().map(|&c| u64::from(c)).sum::<u64>(), (w * hh) as u64);
        assert!(h.red.iter().all(|&c| c > 0));
    }
}
