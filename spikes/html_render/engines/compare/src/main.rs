//! Scores each engine's render against the Chromium reference and builds
//! side-by-side review sheets (Chromium | Blitz | litehtml).
//! Usage: compare <out_dir>   (expects out/{chromium,blitz,litehtml}/*.png)
use image::{imageops, GrayImage, Rgba, RgbaImage};
use std::{fs, path::Path};

const ENGINES: [&str; 2] = ["blitz", "litehtml"];
/// Column width in the review sheet (renders are 800px wide).
const COL: u32 = 520;
const GAP: u32 = 12;
/// Sheet segment height, so tall emails stay legible when viewed.
const SEG: u32 = 1100;

fn main() {
    let out = Path::new(&std::env::args().nth(1).unwrap()).to_path_buf();
    let sheets = out.join("sheets");
    fs::create_dir_all(&sheets).unwrap();
    let mut names: Vec<String> = fs::read_dir(out.join("chromium")).unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path().file_stem().unwrap().to_string_lossy().to_string())
        .collect();
    names.sort();
    println!("email\tchromium_h\tblitz_h\tblitz_ssim\tlitehtml_h\tlitehtml_ssim");
    for name in names {
        let load = |e: &str| image::open(out.join(e).join(format!("{name}.png"))).ok().map(|i| i.to_rgba8());
        let reference = load("chromium").unwrap();
        let mut row = format!("{name}\t{}", reference.height());
        let mut cols = vec![reference.clone()];
        for e in ENGINES {
            match load(e) {
                Some(img) => {
                    row += &format!("\t{}\t{:.3}", img.height(), ssim(&reference, &img));
                    cols.push(img);
                }
                None => {
                    row += "\t0\t0";
                    cols.push(RgbaImage::from_pixel(800, 600, Rgba([255, 0, 0, 255])));
                }
            }
        }
        println!("{row}");
        write_sheets(&sheets, &name, &cols);
    }
}

/// Mean SSIM over 8x8 windows on grayscale renders downscaled to 400px wide,
/// padded with white to a common height. Downscaling forgives sub-pixel text
/// differences (antialiasing, hinting) while keeping layout errors visible;
/// height mismatches are penalized because the padding is compared too.
fn ssim(a: &RgbaImage, b: &RgbaImage) -> f64 {
    let prep = |img: &RgbaImage| {
        let g = imageops::grayscale(img);
        let h = (g.height() as f64 * 400.0 / g.width() as f64).round().max(1.0) as u32;
        imageops::resize(&g, 400, h, imageops::FilterType::Triangle)
    };
    let (a, b) = (prep(a), prep(b));
    let h = a.height().max(b.height());
    let pad = |g: &GrayImage| {
        let mut out = GrayImage::from_pixel(400, h, image::Luma([255]));
        imageops::overlay(&mut out, g, 0, 0);
        out
    };
    let (a, b) = (pad(&a), pad(&b));
    let (c1, c2) = ((0.01f64 * 255.0).powi(2), (0.03f64 * 255.0).powi(2));
    let (mut total, mut n) = (0.0, 0u32);
    for wy in (0..h.saturating_sub(7)).step_by(8) {
        for wx in (0..393).step_by(8) {
            let (mut sa, mut sb, mut saa, mut sbb, mut sab) = (0.0, 0.0, 0.0, 0.0, 0.0);
            for y in wy..wy + 8 {
                for x in wx..wx + 8 {
                    let (pa, pb) = (a.get_pixel(x, y)[0] as f64, b.get_pixel(x, y)[0] as f64);
                    sa += pa; sb += pb; saa += pa * pa; sbb += pb * pb; sab += pa * pb;
                }
            }
            let (ma, mb) = (sa / 64.0, sb / 64.0);
            let (va, vb, cov) = (saa / 64.0 - ma * ma, sbb / 64.0 - mb * mb, sab / 64.0 - ma * mb);
            total += ((2.0 * ma * mb + c1) * (2.0 * cov + c2)) / ((ma * ma + mb * mb + c1) * (va + vb + c2));
            n += 1;
        }
    }
    total / n as f64
}

fn write_sheets(dir: &Path, name: &str, cols: &[RgbaImage]) {
    let scaled: Vec<RgbaImage> = cols.iter().map(|c| {
        let h = (c.height() as f64 * COL as f64 / c.width() as f64).round() as u32;
        imageops::resize(c, COL, h, imageops::FilterType::Triangle)
    }).collect();
    let total_h = scaled.iter().map(|s| s.height()).max().unwrap();
    let width = COL * 3 + GAP * 2;
    let header = 10;
    let mut seg = 0;
    let mut y0 = 0;
    while y0 < total_h {
        let h = SEG.min(total_h - y0);
        let mut sheet = RgbaImage::from_pixel(width, h + header, Rgba([90, 90, 90, 255]));
        // Colored header strip per column: grey = Chromium, orange = Blitz, blue = litehtml.
        let colors = [Rgba([60, 60, 60, 255]), Rgba([230, 120, 20, 255]), Rgba([30, 110, 220, 255])];
        for (i, s) in scaled.iter().enumerate() {
            let x = i as u32 * (COL + GAP);
            for yy in 0..header { for xx in 0..COL { sheet.put_pixel(x + xx, yy, colors[i]); } }
            if y0 < s.height() {
                let part = imageops::crop_imm(s, 0, y0, COL, h.min(s.height() - y0)).to_image();
                imageops::overlay(&mut sheet, &part, x as i64, header as i64);
            }
        }
        sheet.save(dir.join(format!("{name}__{seg}.png"))).unwrap();
        seg += 1;
        y0 += SEG;
    }
}
