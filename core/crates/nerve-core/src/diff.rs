//! Frame-delta computation for observation streaming.
//!
//! When the dashboard or an agent subscribes to observations, we don't want
//! to ship the full screenshot every tick — a typical desktop only changes a
//! few pixels per frame. This module computes a compact diff descriptor we
//! attach to each `Observation`.
//!
//! The diff is intentionally lossy: we only report dirty *bounding boxes*,
//! not a per-pixel mask. That keeps the payload small while still letting an
//! agent know "the bottom-right area changed, re-OCR it".

use std::sync::Arc;

use parking_lot::Mutex;
use sha2::Digest;

use nerve_protocol::Bounds;

/// Coarse cache of recent screen snapshots, keyed by session.
#[derive(Debug, Default)]
pub struct FrameCache {
    inner: Mutex<Option<FrameState>>,
}

#[derive(Debug)]
struct FrameState {
    width: i32,
    height: i32,
    /// Tile-level hashes laid out row-major.
    tiles: Vec<[u8; 32]>,
    /// PNG bytes of the previous full frame (for callers who want it).
    last_png: Vec<u8>,
}

impl FrameCache {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Compute a diff against the previous frame and update the cache. Returns
    /// the list of dirty tile bounding boxes (in pixel coordinates) plus the
    /// previous frame's PNG length so callers can decide how often to ship
    /// full vs delta.
    pub fn diff_and_replace(
        &self,
        rgba: &[u8],
        width: i32,
        height: i32,
        png_bytes: Vec<u8>,
    ) -> Vec<Bounds> {
        let tile = 64; // pixels per tile edge
        let cols = (width + tile - 1) / tile;
        let rows = (height + tile - 1) / tile;
        let mut tiles: Vec<[u8; 32]> = vec![[0u8; 32]; (cols * rows) as usize];

        // Hash each tile's pixel block. We assume RGBA8888 row-major.
        let row_stride = (width as usize) * 4;
        for ty in 0..rows {
            for tx in 0..cols {
                let mut hasher = sha2::Sha256::new();
                let x0 = (tx * tile) as usize * 4;
                let y0 = (ty * tile) as usize;
                let y1 = std::cmp::min((ty * tile + tile) as usize, height as usize);
                let x_bytes = std::cmp::min(((tx + 1) * tile) as usize * 4, row_stride);
                for y in y0..y1 {
                    let row_start = y * row_stride + x0;
                    let row_end = y * row_stride + x_bytes;
                    if row_end <= rgba.len() && row_start < row_end {
                        hasher.update(&rgba[row_start..row_end]);
                    }
                }
                let mut h = [0u8; 32];
                h.copy_from_slice(&hasher.finalize());
                tiles[(ty * cols + tx) as usize] = h;
            }
        }

        let mut dirty: Vec<Bounds> = Vec::new();
        let mut guard = self.inner.lock();
        let prev = guard.take();
        if let Some(prev) = prev {
            if prev.width == width && prev.height == height && prev.tiles.len() == tiles.len() {
                for ty in 0..rows {
                    for tx in 0..cols {
                        let idx = (ty * cols + tx) as usize;
                        if prev.tiles[idx] != tiles[idx] {
                            dirty.push(Bounds {
                                x: tx * tile,
                                y: ty * tile,
                                width: tile,
                                height: tile,
                            });
                        }
                    }
                }
            } else {
                // Size change — treat as fully dirty.
                dirty.push(Bounds {
                    x: 0,
                    y: 0,
                    width,
                    height,
                });
            }
        }
        *guard = Some(FrameState {
            width,
            height,
            tiles,
            last_png: png_bytes,
        });
        dirty
    }

    pub fn last_png(&self) -> Option<Vec<u8>> {
        self.inner.lock().as_ref().map(|s| s.last_png.clone())
    }
}

/// Convenience: collapse a list of dirty tiles into a smaller list by merging
/// adjacent tiles.
pub fn coalesce(mut tiles: Vec<Bounds>) -> Vec<Bounds> {
    if tiles.is_empty() {
        return tiles;
    }
    tiles.sort_by_key(|b| (b.y, b.x));
    let mut out: Vec<Bounds> = Vec::with_capacity(tiles.len());
    for b in tiles {
        if let Some(last) = out.last_mut() {
            // Same row + adjacent column → extend horizontally.
            if last.y == b.y && last.height == b.height && last.x + last.width == b.x {
                last.width += b.width;
                continue;
            }
        }
        out.push(b);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_frames_have_no_diff() {
        let pixels = vec![255u8; 64 * 64 * 4];
        let cache = FrameCache::new();
        let _ = cache.diff_and_replace(&pixels, 64, 64, vec![]);
        let diff = cache.diff_and_replace(&pixels, 64, 64, vec![]);
        assert!(diff.is_empty());
    }

    #[test]
    fn differing_pixels_produce_dirty_tile() {
        let frame_a = vec![0u8; 128 * 128 * 4];
        let mut frame_b = vec![0u8; 128 * 128 * 4];
        // Flip a single pixel in the top-left tile of frame_b.
        frame_b[0] = 255;
        let cache = FrameCache::new();
        let _ = cache.diff_and_replace(&frame_a, 128, 128, vec![]);
        let diff = cache.diff_and_replace(&frame_b, 128, 128, vec![]);
        assert!(!diff.is_empty());
        let coalesced = coalesce(diff);
        // All dirty regions should be inside the top-left tile.
        let _ = frame_a; // silence warning
        for b in coalesced {
            assert!(b.x < 64 && b.y < 64);
        }
    }
}
