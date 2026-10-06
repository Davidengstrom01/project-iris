/// A point on a tone curve; x = input, y = output, both 0..1 in perceptual (gamma) units.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CurvePoint {
    pub x: f32,
    pub y: f32,
}

impl CurvePoint {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

pub type CurvePoints = Vec<CurvePoint>;

/// Minimum horizontal distance between points.
pub const MIN_CURVE_GAP: f32 = 0.01;
pub const MAX_CURVE_POINTS: usize = 16;

fn points(p: &[(f32, f32)]) -> CurvePoints {
    p.iter().map(|&(x, y)| CurvePoint { x, y }).collect()
}

pub fn linear_curve() -> CurvePoints {
    points(&[(0.0, 0.0), (1.0, 1.0)])
}

/// More contrast: slightly lower shadows, raised highlights.
pub fn s_curve() -> CurvePoints {
    points(&[(0.0, 0.0), (0.25, 0.20), (0.5, 0.5), (0.75, 0.80), (1.0, 1.0)])
}

/// Less contrast.
pub fn inverse_s_curve() -> CurvePoints {
    points(&[(0.0, 0.0), (0.25, 0.30), (0.5, 0.5), (0.75, 0.70), (1.0, 1.0)])
}

pub fn is_linear(points: &[CurvePoint]) -> bool {
    if points.iter().any(|p| (p.x - p.y).abs() > 1e-6) {
        return false;
    }
    points.len() >= 2 && points[0].x <= 1e-6 && points[points.len() - 1].x >= 1.0 - 1e-6
}

/// Sorted by x, clamped to [0, 1], points at least [`MIN_CURVE_GAP`] apart, at least two
/// points and at most [`MAX_CURVE_POINTS`]. Anything unusable becomes the linear curve.
pub fn normalized_curve(points: &[CurvePoint]) -> CurvePoints {
    let mut sorted: CurvePoints = points
        .iter()
        .filter(|p| p.x.is_finite() && p.y.is_finite())
        .map(|p| CurvePoint { x: p.x.clamp(0.0, 1.0), y: p.y.clamp(0.0, 1.0) })
        .collect();
    sorted.sort_by(|a, b| a.x.total_cmp(&b.x)); // stable
    let mut result: CurvePoints = Vec::with_capacity(sorted.len());
    for p in sorted {
        if result.last().is_none_or(|last| p.x - last.x >= MIN_CURVE_GAP) {
            result.push(p);
        }
    }
    if result.len() < 2 || result.len() > MAX_CURVE_POINTS {
        return linear_curve();
    }
    result
}

/// A smooth curve through the points that never overshoots between them (monotone cubic
/// Hermite interpolation, Fritsch-Carlson). Flat beyond the first and last point.
#[derive(Clone, Debug)]
pub struct CurveSpline {
    x: Vec<f32>,
    y: Vec<f32>,
    slope: Vec<f32>,
}

impl CurveSpline {
    pub fn new(input: &[CurvePoint]) -> Self {
        let points = normalized_curve(input);
        let n = points.len();
        let x: Vec<f32> = points.iter().map(|p| p.x).collect();
        let y: Vec<f32> = points.iter().map(|p| p.y).collect();

        // Secant slopes, then tangents limited so the interpolant cannot overshoot.
        let secant: Vec<f32> = (0..n - 1).map(|k| (y[k + 1] - y[k]) / (x[k + 1] - x[k])).collect();
        let mut slope = vec![0.0f32; n];
        slope[0] = secant[0];
        slope[n - 1] = secant[n - 2];
        for k in 1..n - 1 {
            slope[k] = if secant[k - 1] * secant[k] <= 0.0 { 0.0 } else { (secant[k - 1] + secant[k]) / 2.0 };
        }
        for k in 0..n - 1 {
            if secant[k] == 0.0 {
                slope[k] = 0.0;
                slope[k + 1] = 0.0;
                continue;
            }
            let a = slope[k] / secant[k];
            let b = slope[k + 1] / secant[k];
            let h = a * a + b * b;
            if h > 9.0 {
                let t = 3.0 / h.sqrt();
                slope[k] = t * a * secant[k];
                slope[k + 1] = t * b * secant[k];
            }
        }
        Self { x, y, slope }
    }

    pub fn eval(&self, x: f32) -> f32 {
        let (xs, ys, m) = (&self.x, &self.y, &self.slope);
        if x <= xs[0] {
            return ys[0];
        }
        if x >= xs[xs.len() - 1] {
            return ys[ys.len() - 1];
        }
        // Last point with xs[k] <= x (upper_bound - 1).
        let k = xs.partition_point(|&v| v <= x) - 1;
        let h = xs[k + 1] - xs[k];
        let t = (x - xs[k]) / h;
        let t2 = t * t;
        let t3 = t2 * t;
        let y = (2.0 * t3 - 3.0 * t2 + 1.0) * ys[k]
            + (t3 - 2.0 * t2 + t) * h * m[k]
            + (-2.0 * t3 + 3.0 * t2) * ys[k + 1]
            + (t3 - t2) * h * m[k + 1];
        y.clamp(0.0, 1.0)
    }
}

/// The tone curves of a photo. Only the RGB (master) curve exists for now; separate red,
/// green and blue curves can be added as further fields.
#[derive(Clone, Debug, PartialEq)]
pub struct ToneCurve {
    pub rgb: CurvePoints,
}

impl Default for ToneCurve {
    fn default() -> Self {
        Self { rgb: linear_curve() }
    }
}

impl ToneCurve {
    pub fn is_identity(&self) -> bool {
        is_linear(&self.rgb)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spline_passes_through_points_without_overshoot() {
        let linear = CurveSpline::new(&linear_curve());
        for x in [0.0, 0.3, 0.77, 1.0] {
            assert!((linear.eval(x) - x).abs() < 1e-6);
        }

        let s = s_curve();
        let spline = CurveSpline::new(&s);
        for p in &s {
            assert!((spline.eval(p.x) - p.y).abs() < 1e-5);
        }
        let mut previous = -1.0;
        for i in 0..=1000 {
            // monotone data gives a monotone curve
            let y = spline.eval(i as f32 / 1000.0);
            assert!(y >= previous);
            previous = y;
        }

        // A sharp step must not ring above or below its points.
        let step = CurveSpline::new(&points(&[(0.0, 0.0), (0.45, 0.1), (0.55, 0.9), (1.0, 1.0)]));
        for i in 0..=1000 {
            let y = step.eval(i as f32 / 1000.0);
            assert!((0.0..=1.0).contains(&y));
        }
        assert!(step.eval(0.3) <= 0.1 + 1e-6);
        assert!(step.eval(0.7) >= 0.9 - 1e-6);
    }

    #[test]
    fn curves_are_normalised() {
        assert_eq!(
            normalized_curve(&points(&[(1.0, 1.0), (0.5, 0.6), (0.0, 0.0)])),
            points(&[(0.0, 0.0), (0.5, 0.6), (1.0, 1.0)])
        );
        assert_eq!(normalized_curve(&points(&[(0.0, 0.0)])), linear_curve()); // too few points
        assert_eq!(normalized_curve(&points(&[(0.0, 0.0), (0.5, 2.0), (1.0, 1.0)]))[1].y, 1.0); // clamped
        assert_eq!(normalized_curve(&points(&[(0.0, 0.0), (0.5, 0.5), (0.502, 0.6), (1.0, 1.0)])).len(), 3);
        assert!(is_linear(&linear_curve()));
        assert!(!is_linear(&s_curve()));
    }
}
