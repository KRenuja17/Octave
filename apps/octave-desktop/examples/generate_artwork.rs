//! Regenerate Octave's original abstract fixture covers. No external art is used.
fn main() {
    let palettes = [
        ([226, 139, 113], [30, 57, 60]),
        ([235, 175, 70], [110, 56, 43]),
        ([162, 190, 178], [35, 77, 80]),
        ([114, 117, 179], [25, 30, 65]),
        ([80, 151, 158], [21, 57, 77]),
        ([198, 153, 168], [68, 60, 112]),
    ];
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
    for (index, (top, bottom)) in palettes.iter().enumerate() {
        let mut img = image::RgbImage::new(512, 512);
        for (x, y, pixel) in img.enumerate_pixels_mut() {
            let u = x as f32 / 512.0;
            let v = y as f32 / 512.0;
            let grain = (((x * 73 + y * 113) % 29) as f32 - 14.0) * 0.13;
            let mut c = std::array::from_fn::<_, 3, _>(|i| {
                top[i] as f32 * (1.0 - v) + bottom[i] as f32 * v + grain
            });
            let distance = ((u - 0.65).powi(2) + (v - 0.32).powi(2)).sqrt();
            match index {
                0 | 1 => {
                    if distance < 0.145 {
                        c = [248.0, 218.0, 162.0];
                    }
                    if v > 0.66 + 0.10 * (u * 5.0).sin() {
                        c = std::array::from_fn(|i| c[i] * [0.69, 0.72, 0.77][i]);
                    }
                    if v > 0.88 + 0.07 * (u * 6.0 + 1.0).sin() {
                        c = std::array::from_fn(|i| c[i] * 0.66);
                    }
                    if index == 0 && u > 0.16 && u < 0.20 {
                        c = std::array::from_fn(|i| c[i] * 0.78);
                    }
                }
                2 => {
                    let d = ((u - 0.50).powi(2) + (v - 0.51).powi(2)).sqrt();
                    if (d - 0.28).abs() < 0.065 {
                        c = [228.0, 226.0, 193.0];
                    }
                    if u > 0.48 && u < 0.53 {
                        c = [49.0, 80.0, 79.0];
                    }
                    if v > 0.76 && v < 0.80 {
                        c = [222.0, 212.0, 175.0];
                    }
                }
                3 => {
                    if (u * 1.6 + v * 0.9).fract() < 0.06 {
                        c = [181.0, 160.0, 184.0];
                    }
                    if v > 0.55 && u > 0.20 && u < 0.42 {
                        c = [30.0, 40.0, 70.0];
                    }
                    if v > 0.37 && u > 0.55 && u < 0.72 {
                        c = [38.0, 46.0, 84.0];
                    }
                    if distance < 0.09 {
                        c = [239.0, 197.0, 178.0];
                    }
                }
                4 => {
                    if (v * 6.0 + u * 1.4 + (u * 5.0).sin() * 0.3).fract() < 0.12 {
                        c = [159.0, 205.0, 197.0];
                    }
                    if distance < 0.13 {
                        c = [224.0, 211.0, 177.0];
                    }
                }
                _ => {
                    let d = ((u - 0.44).powi(2) + (v - 0.48).powi(2)).sqrt();
                    if d < 0.28 {
                        c = [228.0, 187.0, 171.0];
                    }
                    if ((u - 0.60).powi(2) + (v - 0.39).powi(2)).sqrt() < 0.24 {
                        c = [113.0, 90.0, 139.0];
                    }
                    if v > 0.85 {
                        c = [63.0, 60.0, 102.0];
                    }
                }
            }
            *pixel = image::Rgb(c.map(|n| n.clamp(0.0, 255.0) as u8));
        }
        img.save(root.join(format!("cover-{index}.png"))).unwrap();
    }
}
