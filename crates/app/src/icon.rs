//! The app icon, drawn in code so the binary needs no image assets: a treasure chest.
//!
//! Shared with `build.rs`, which turns it into the `.ico` embedded in the executable.

/// Straight (non-premultiplied) RGBA pixels, row-major, `size * size * 4` bytes.
pub fn chest_rgba(size: u32) -> Vec<u8> {
    let s = size as f32;
    let mut px = vec![0u8; (size * size * 4) as usize];

    const WOOD: [u8; 3] = [139, 82, 45];
    const WOOD_DARK: [u8; 3] = [96, 54, 28];
    const GOLD: [u8; 3] = [245, 190, 60];
    const GOLD_DARK: [u8; 3] = [196, 140, 30];
    const OUTLINE: [u8; 3] = [40, 24, 14];

    let rounded_rect = |x: f32, y: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32| -> bool {
        let cx = x.clamp(x0 + r, x1 - r);
        let cy = y.clamp(y0 + r, y1 - r);
        (x - cx).powi(2) + (y - cy).powi(2) <= r * r
    };

    for py in 0..size {
        for pxi in 0..size {
            let x = (pxi as f32 + 0.5) / s;
            let y = (py as f32 + 0.5) / s;
            let color: Option<[u8; 3]> = if rounded_rect(x, y, 0.06, 0.16, 0.94, 0.88, 0.08) {
                let inner = rounded_rect(x, y, 0.11, 0.21, 0.89, 0.83, 0.05);
                let lid = y < 0.44;
                let band = (0.40..0.48).contains(&y);
                let strap = (0.22..0.30).contains(&x) || (0.70..0.78).contains(&x);
                let lock = rounded_rect(x, y, 0.42, 0.38, 0.58, 0.58, 0.03);
                let keyhole = ((x - 0.5).powi(2) + (y - 0.46).powi(2)).sqrt() < 0.025
                    || ((0.49..0.51).contains(&x) && (0.46..0.53).contains(&y));
                Some(if !inner || (keyhole && lock) {
                    OUTLINE
                } else if lock {
                    GOLD
                } else if band {
                    GOLD_DARK
                } else if strap {
                    GOLD
                } else if lid {
                    WOOD
                } else {
                    WOOD_DARK
                })
            } else {
                None
            };
            if let Some([r, g, b]) = color {
                let i = ((py * size + pxi) * 4) as usize;
                px[i..i + 4].copy_from_slice(&[r, g, b, 255]);
            }
        }
    }
    px
}
