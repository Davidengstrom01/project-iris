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

    fn merge(mut self, other: Histogram) -> Histogram {
        for k in 0..256 {
            self.red[k] += other.red[k];
            self.green[k] += other.green[k];
            self.blue[k] += other.blue[k];
            self.luminance[k] += other.luminance[k];
        }
        self
    }

    pub fn compute(image: &EncodedImage) -> Histogram {
        const CHUNK: usize = 3 * 4096;
        let mut total = match &image.samples {
            Samples::Eight(d) => d
                .par_chunks(CHUNK)
                .fold(Histogram::default, |mut h, chunk| {
                    for px in chunk.as_chunks::<3>().0 {
                        h.add(px[0].into(), px[1].into(), px[2].into());
                    }
                    h
                })
                .reduce(Histogram::default, Histogram::merge),
            Samples::Sixteen(d) => d
                .par_chunks(CHUNK)
                .fold(Histogram::default, |mut h, chunk| {
                    for px in chunk.as_chunks::<3>().0 {
                        h.add(usize::from(px[0] >> 8), usize::from(px[1] >> 8), usize::from(px[2] >> 8));
                    }
                    h
                })
                .reduce(Histogram::default, Histogram::merge),
        };
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
}
