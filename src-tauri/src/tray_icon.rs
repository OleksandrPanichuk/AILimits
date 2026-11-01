//! Draws the tray icon: two bars, session on top and weekly below.
//! A bar's fill shows how much is LEFT, its color shows the risk.

use crate::pace::PaceStatus;
use crate::usage::{Limit, Snapshot};

pub const SIZE: u32 = 32;

type Rgba = [u8; 4];

const TRACK: Rgba = [128, 128, 128, 110];
const GREEN: Rgba = [52, 199, 89, 255];
const AMBER: Rgba = [255, 179, 0, 255];
const RED: Rgba = [255, 69, 58, 255];

pub fn render(snapshot: Option<&Snapshot>) -> Vec<u8> {
    let mut px = vec![0u8; (SIZE * SIZE * 4) as usize];
    let session = snapshot.and_then(|s| s.session());
    let weekly = snapshot.and_then(|s| s.weekly());
    draw_bar(&mut px, 5, 10, session);
    draw_bar(&mut px, 18, 10, weekly);
    px
}

fn color_for(l: &Limit) -> Rgba {
    let status = l.pace.as_ref().map(|p| &p.status);
    if l.used_percent >= 95.0 || status == Some(&PaceStatus::Exhausted) {
        RED
    } else if l.used_percent >= 75.0 || status == Some(&PaceStatus::RunsOut) {
        AMBER
    } else {
        GREEN
    }
}

fn draw_bar(px: &mut [u8], top: u32, height: u32, limit: Option<&Limit>) {
    let (left, right) = (1u32, SIZE - 1);
    let width = right - left;
    let (fill_w, color) = match limit {
        Some(l) => {
            let remaining = (100.0 - l.used_percent).clamp(0.0, 100.0) / 100.0;
            let w = (remaining * width as f64).round() as u32;
            // Keep a sliver visible so "almost empty" differs from "no data".
            (if remaining > 0.0 { w.max(2) } else { 0 }, color_for(l))
        }
        None => (0, TRACK),
    };
    for y in top..top + height {
        for x in left..right {
            // Rounded corners: skip the four corner pixels.
            let corner = (x == left || x == right - 1) && (y == top || y == top + height - 1);
            if corner {
                continue;
            }
            let c = if x - left < fill_w { color } else { TRACK };
            let i = ((y * SIZE + x) * 4) as usize;
            px[i..i + 4].copy_from_slice(&c);
        }
    }
}
