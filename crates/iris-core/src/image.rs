/// Scene-referred, linear-light RGB image in the working colour space
/// (linear Rec.2020 primaries, D65 white). Interleaved RGB, row-major, no padding.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImageF {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<f32>,
}

impl ImageF {
    pub fn new(width: usize, height: usize) -> Self {
        Self { width, height, pixels: vec![0.0; width * height * 3] }
    }

    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    pub fn row(&self, y: usize) -> &[f32] {
        let n = self.width * 3;
        &self.pixels[y * n..(y + 1) * n]
    }

    pub fn row_mut(&mut self, y: usize) -> &mut [f32] {
        let n = self.width * 3;
        &mut self.pixels[y * n..(y + 1) * n]
    }
}

/// Encoded samples of an output image: 8 or 16 bits per channel.
#[derive(Clone, Debug, PartialEq)]
pub enum Samples {
    Eight(Vec<u8>),
    Sixteen(Vec<u16>),
}

/// Output-referred image, already encoded in its output colour space (e.g. sRGB).
/// Interleaved RGB, row-major.
#[derive(Clone, Debug, PartialEq)]
pub struct EncodedImage {
    pub width: usize,
    pub height: usize,
    pub samples: Samples,
}

impl EncodedImage {
    /// `bits` is 8 or 16.
    pub fn new(width: usize, height: usize, bits: u32) -> Self {
        let n = width * height * 3;
        let samples = if bits == 16 { Samples::Sixteen(vec![0; n]) } else { Samples::Eight(vec![0; n]) };
        Self { width, height, samples }
    }

    pub fn bits_per_channel(&self) -> u32 {
        match self.samples {
            Samples::Eight(_) => 8,
            Samples::Sixteen(_) => 16,
        }
    }

    /// The 8-bit samples; panics for a 16-bit image.
    pub fn data8(&self) -> &[u8] {
        match &self.samples {
            Samples::Eight(d) => d,
            Samples::Sixteen(_) => panic!("16-bit image has no 8-bit samples"),
        }
    }

    /// The 16-bit samples; panics for an 8-bit image.
    pub fn data16(&self) -> &[u16] {
        match &self.samples {
            Samples::Sixteen(d) => d,
            Samples::Eight(_) => panic!("8-bit image has no 16-bit samples"),
        }
    }
}
