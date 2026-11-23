//! Viewer-side layout for multiple simultaneous presenter streams.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Single,
    Grid,
    PictureInPicture,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Clone)]
pub struct Tile {
    pub stream: String,
    pub rect: Rect,
}

#[derive(Debug, Clone)]
pub struct TileLayout {
    pub layout: Layout,
    pub tiles: Vec<Tile>,
}

impl TileLayout {
    pub fn compute(layout: Layout, streams: &[String]) -> Self {
        let tiles = match layout {
            Layout::Single => streams
                .first()
                .map(|s| {
                    vec![Tile {
                        stream: s.clone(),
                        rect: Rect {
                            x: 0.0,
                            y: 0.0,
                            w: 1.0,
                            h: 1.0,
                        },
                    }]
                })
                .unwrap_or_default(),
            Layout::Grid => {
                let n = streams.len().max(1);
                let cols = (n as f32).sqrt().ceil() as usize;
                let rows = n.div_ceil(cols);
                let (w, h) = (1.0 / cols as f32, 1.0 / rows as f32);
                streams
                    .iter()
                    .enumerate()
                    .map(|(i, s)| Tile {
                        stream: s.clone(),
                        rect: Rect {
                            x: (i % cols) as f32 * w,
                            y: (i / cols) as f32 * h,
                            w,
                            h,
                        },
                    })
                    .collect()
            }
            Layout::PictureInPicture => {
                let mut tiles: Vec<Tile> = streams
                    .iter()
                    .take(1)
                    .map(|s| Tile {
                        stream: s.clone(),
                        rect: Rect {
                            x: 0.0,
                            y: 0.0,
                            w: 1.0,
                            h: 1.0,
                        },
                    })
                    .collect();
                let inset = 0.25f32;
                for (i, s) in streams.iter().skip(1).enumerate() {
                    tiles.push(Tile {
                        stream: s.clone(),
                        rect: Rect {
                            x: 1.0 - inset - 0.01,
                            y: 0.01 + i as f32 * (inset + 0.01),
                            w: inset,
                            h: inset * 0.5625,
                        },
                    });
                }
                tiles
            }
        };
        Self { layout, tiles }
    }

    /// Which stream is under a normalized pointer position?
    pub fn hit_test(&self, x: f32, y: f32) -> Option<&str> {
        self.tiles
            .iter()
            .rev()
            .find(|t| {
                x >= t.rect.x
                    && x <= t.rect.x + t.rect.w
                    && y >= t.rect.y
                    && y <= t.rect.y + t.rect.h
            })
            .map(|t| t.stream.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("s{i}")).collect()
    }

    #[test]
    fn single_uses_first_stream() {
        let layout = TileLayout::compute(Layout::Single, &names(3));
        assert_eq!(layout.tiles.len(), 1);
        assert_eq!(layout.tiles[0].stream, "s0");
    }

    #[test]
    fn grid_covers_all_streams() {
        let layout = TileLayout::compute(Layout::Grid, &names(4));
        assert_eq!(layout.tiles.len(), 4);
        for tile in &layout.tiles {
            assert!((tile.rect.w - 0.5).abs() < 1e-6);
            assert!((tile.rect.h - 0.5).abs() < 1e-6);
        }
    }

    #[test]
    fn pip_overlays_smaller_tiles() {
        let layout = TileLayout::compute(Layout::PictureInPicture, &names(3));
        assert_eq!(layout.tiles.len(), 3);
        assert_eq!(layout.hit_test(0.5, 0.5), Some("s0"));
        let last = layout.tiles.last().unwrap();
        assert_eq!(
            layout.hit_test(last.rect.x + 0.01, last.rect.y + 0.01),
            Some(last.stream.as_str())
        );
    }

    #[test]
    fn empty_streams_produce_no_tiles() {
        assert!(TileLayout::compute(Layout::Single, &[]).tiles.is_empty());
        assert!(TileLayout::compute(Layout::Grid, &[]).tiles.is_empty());
    }
}
