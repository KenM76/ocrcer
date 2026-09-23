//! Two-pass union-find connected-component labelling.
//!
//! General-purpose: used by the feature extractor's hole count and, from
//! chunk 2 onward, by page-level component detection. Per `CLAUDE.md` rule
//! 4 this is the only labeller in the crate.

/// Which neighbours count as adjacent when grouping same-value pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Connectivity {
    /// North, south, east, west.
    Four,
    /// The four plus the diagonals.
    Eight,
}

/// Labels connected components of the nonzero pixels in `mask`.
///
/// `mask` is row-major, `width * height` bytes; any nonzero byte counts as
/// foreground, `0` as background. `connectivity` selects which neighbours
/// count as adjacent for foreground pixels.
///
/// Returns one label per pixel (background pixels hold `0`; each foreground
/// component holds a distinct label in `1..=count`, assigned in
/// first-encounter row-major order) and `count`, the number of components.
/// A `width == 0 || height == 0` mask returns an empty label vector and a
/// count of `0`.
///
/// # Panics
/// Panics if `mask.len() != width as usize * height as usize`.
pub fn label(mask: &[u8], width: u32, height: u32, connectivity: Connectivity) -> (Vec<u32>, u32) {
    assert_eq!(
        mask.len(),
        width as usize * height as usize,
        "mask length must equal width*height"
    );
    let w = width as usize;
    let h = height as usize;
    let mut labels = vec![0u32; w * h];
    if w == 0 || h == 0 {
        return (labels, 0);
    }

    // parent[0] is an unused sentinel; real provisional labels start at 1.
    let mut parent: Vec<u32> = vec![0];

    for y in 0..h {
        for x in 0..w {
            let idx = y * w + x;
            if mask[idx] == 0 {
                continue;
            }
            let mut neighbors: [u32; 4] = [0; 4];
            let mut n = 0;
            if x > 0 && labels[idx - 1] != 0 {
                neighbors[n] = labels[idx - 1];
                n += 1;
            }
            if y > 0 && labels[idx - w] != 0 {
                neighbors[n] = labels[idx - w];
                n += 1;
            }
            if connectivity == Connectivity::Eight {
                if y > 0 && x > 0 && labels[idx - w - 1] != 0 {
                    neighbors[n] = labels[idx - w - 1];
                    n += 1;
                }
                if y > 0 && x + 1 < w && labels[idx - w + 1] != 0 {
                    neighbors[n] = labels[idx - w + 1];
                    n += 1;
                }
            }
            if n == 0 {
                let new_label = parent.len() as u32;
                parent.push(new_label);
                labels[idx] = new_label;
            } else {
                let min_label = neighbors[..n].iter().copied().min().unwrap();
                labels[idx] = min_label;
                for &l in &neighbors[..n] {
                    union(&mut parent, l, min_label);
                }
            }
        }
    }

    for l in labels.iter_mut() {
        if *l != 0 {
            *l = find(&mut parent, *l);
        }
    }

    let mut remap: Vec<u32> = vec![0; parent.len()];
    let mut next_id = 1u32;
    for l in labels.iter_mut() {
        let root = *l;
        if root != 0 {
            if remap[root as usize] == 0 {
                remap[root as usize] = next_id;
                next_id += 1;
            }
            *l = remap[root as usize];
        }
    }
    (labels, next_id - 1)
}

/// One connected component: its label, its bounding box, and how much ink
/// it holds.
///
/// `x1`/`y1` are **exclusive**, so `x1 - x0` is the width. Every consumer of
/// this downstream does width arithmetic and none does an inclusive-range
/// walk, so exclusive is the form that never needs a `+ 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Component {
    pub label: u32,
    pub x0: u32,
    pub y0: u32,
    pub x1: u32,
    pub y1: u32,
    /// Ink pixels in the component, which is not `width * height` for
    /// anything but a solid rectangle.
    pub area: u32,
}

impl Component {
    pub fn width(&self) -> u32 {
        self.x1 - self.x0
    }
    pub fn height(&self) -> u32 {
        self.y1 - self.y0
    }
    /// Horizontal centre, in half-pixel units doubled to stay integral.
    pub fn cx2(&self) -> u32 {
        self.x0 + self.x1
    }
    pub fn overlaps_x(&self, other: &Component) -> bool {
        self.x0 < other.x1 && other.x0 < self.x1
    }
    /// Height of the vertical overlap between two components, zero when they
    /// do not overlap.
    pub fn overlap_y(&self, other: &Component) -> u32 {
        self.y1.min(other.y1).saturating_sub(self.y0.max(other.y0))
    }
}

/// Bounding boxes and ink counts for every component, indexed by
/// `label - 1`, so `components(...)[i].label == i as u32 + 1`.
///
/// Labels are assigned in first-encounter row-major order, so this vector is
/// in a fixed order for a given mask and no sort is needed to make a fixture
/// reproducible.
pub fn components(labels: &[u32], width: u32, height: u32, count: u32) -> Vec<Component> {
    let w = width as usize;
    let mut out: Vec<Component> = (1..=count)
        .map(|label| Component { label, x0: u32::MAX, y0: u32::MAX, x1: 0, y1: 0, area: 0 })
        .collect();
    for y in 0..height as usize {
        for x in 0..w {
            let l = labels[y * w + x];
            if l == 0 {
                continue;
            }
            let c = &mut out[(l - 1) as usize];
            c.x0 = c.x0.min(x as u32);
            c.y0 = c.y0.min(y as u32);
            c.x1 = c.x1.max(x as u32 + 1);
            c.y1 = c.y1.max(y as u32 + 1);
            c.area += 1;
        }
    }
    out
}

/// Labels a mask and returns its components in one call, the form every
/// page-level caller wants.
pub fn find_components(mask: &[u8], width: u32, height: u32, connectivity: Connectivity) -> Vec<Component> {
    let (labels, count) = label(mask, width, height, connectivity);
    components(&labels, width, height, count)
}

fn find(parent: &mut [u32], mut x: u32) -> u32 {
    while parent[x as usize] != x {
        parent[x as usize] = parent[parent[x as usize] as usize];
        x = parent[x as usize];
    }
    x
}

fn union(parent: &mut [u32], a: u32, b: u32) {
    let ra = find(parent, a);
    let rb = find(parent, b);
    if ra != rb {
        if ra < rb {
            parent[rb as usize] = ra;
        } else {
            parent[ra as usize] = rb;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_mask_has_no_components() {
        let (labels, count) = label(&[], 0, 0, Connectivity::Eight);
        assert!(labels.is_empty());
        assert_eq!(count, 0);
    }

    #[test]
    fn solid_block_is_one_component() {
        let mask = vec![1u8; 9];
        let (labels, count) = label(&mask, 3, 3, Connectivity::Four);
        assert_eq!(count, 1);
        assert!(labels.iter().all(|&l| l == 1));
    }

    #[test]
    fn two_separate_blocks_get_two_labels() {
        // 5x1: 1 1 0 1 1
        let mask = [1u8, 1, 0, 1, 1];
        let (labels, count) = label(&mask, 5, 1, Connectivity::Four);
        assert_eq!(count, 2);
        assert_eq!(labels[0], labels[1]);
        assert_eq!(labels[3], labels[4]);
        assert_ne!(labels[0], labels[3]);
    }

    #[test]
    #[should_panic(expected = "mask length must equal width*height")]
    fn mismatched_length_panics() {
        let _ = label(&[1, 0, 1], 2, 2, Connectivity::Four);
    }

    /// Diagonal-touching pixels merge under 8-connectivity and stay separate
    /// under 4-connectivity, which is the whole reason the hole count in
    /// `feature.rs` must fix background at 4-connected: mixing it up with a
    /// uniform choice changes which background region counts as enclosed.
    #[test]
    fn diagonal_pixels_differ_by_connectivity() {
        #[rustfmt::skip]
        let mask = [
            1u8, 0,
            0,   1,
        ];
        let (_, count_four) = label(&mask, 2, 2, Connectivity::Four);
        let (_, count_eight) = label(&mask, 2, 2, Connectivity::Eight);
        assert_eq!(count_four, 2);
        assert_eq!(count_eight, 1);
    }

    /// The concrete case that matters for hole counting: a 1-thick ring
    /// with its top-left wall corner notched out so the interior touches
    /// the border-adjacent corner pixel only diagonally. Background must be
    /// labelled 4-connected (per ARCHITECTURE.md section 3.1) so the
    /// interior stays a separate, non-border-touching component; the
    /// "naive" uniform 8-connected choice merges it into the border
    /// component and loses the hole entirely.
    #[test]
    fn connectivity_choice_changes_hole_topology() {
        #[rustfmt::skip]
        let ink = [
            0u8, 1, 1, 1,
            1,   0, 0, 1,
            1,   0, 0, 1,
            1,   1, 1, 1,
        ];
        let bg: Vec<u8> = ink.iter().map(|&v| if v == 0 { 1 } else { 0 }).collect();

        let (labels_four, count_four) = label(&bg, 4, 4, Connectivity::Four);
        assert_eq!(count_four, 2, "isolated corner pixel + interior 2x2");
        // (0,0) is background, index 0; interior top-left is (1,1), index 5.
        assert_ne!(labels_four[0], labels_four[5]);

        let (labels_eight, count_eight) = label(&bg, 4, 4, Connectivity::Eight);
        assert_eq!(
            count_eight, 1,
            "8-connected background wrongly merges the corner into the interior"
        );
        assert_eq!(labels_eight[0], labels_eight[5]);
    }
}
