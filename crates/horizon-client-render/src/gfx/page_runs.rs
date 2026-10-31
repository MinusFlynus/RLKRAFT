//! Solid draws grouped by vertex arena page within distance bands.
//!
//! Sections sit in arena pages of 256 MiB, so render distance 32 spreads
//! them over half a dozen pages, and a list sorted only by distance switches
//! page every few sections: about a thousand indirect multi-draws of two or
//! three entries a frame. Mesa's Intel driver (and wgpu-core, for each
//! vertex buffer change) pays per call, and below four entries Mesa unrolls
//! the call on the CPU entry by entry. Grouping the sections of one band of
//! distance by page keeps the list front to back to within the band (what
//! the depth test needs to reject hidden fragments early) with one call per
//! page and band.
//!
//! The picture does not change: solid draws write depth and are tested
//! against it, so their order only decides which of two fragments at the
//! same depth stays, and sections do not share faces.

/// The width of a band, in blocks.
const BAND: f64 = 64.0;

/// One section's draws: `start..start + len` of the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Run {
    key: u64,
    pub start: u32,
    pub len: u32,
    /// The caller's data, carried along (occlusion culling's test inputs).
    pub tag: [u32; 3],
}

pub(super) struct PageRuns {
    band: f64,
    runs: Vec<Run>,
    scratch: Vec<([u32; 5], (u32, Option<u32>))>,
}

impl Default for PageRuns {
    fn default() -> Self {
        Self {
            band: BAND,
            runs: Vec::new(),
            scratch: Vec::new(),
        }
    }
}

impl PageRuns {
    /// Camera-solid draws can trade a few more page switches for tighter
    /// front-to-back ordering. Shadow and translucent ordering keep their
    /// existing policy. The override is for matched GPU benchmarks.
    pub fn for_terrain() -> Self {
        let band = std::env::var("HORIZON_TERRAIN_DISTANCE_BAND")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .filter(|n| [8, 16, 32, 64].contains(n))
            .unwrap_or(16);
        Self {
            band: f64::from(band),
            ..Self::default()
        }
    }

    pub fn clear(&mut self) {
        self.runs.clear();
    }

    /// The draws `start..start + len` of a section `distance` from the
    /// camera (squared, as bits) in arena `page`; runs come nearest first.
    pub fn push(&mut self, distance: u64, page: u32, start: usize, len: usize, tag: [u32; 3]) {
        if len > 0 {
            let band = (f64::from_bits(distance).sqrt() / self.band) as u64;
            self.runs.push(Run {
                key: band << 32 | u64::from(page),
                start: start as u32,
                len: len as u32,
                tag,
            });
        }
    }

    /// Reorders `draws[base..]` and their `pages`, which the pushed runs
    /// cover in order, so that each band's runs in one page are adjacent
    /// (in distance order); returns the runs at their new places.
    pub fn regroup(
        &mut self,
        draws: &mut [[u32; 5]],
        pages: &mut [(u32, Option<u32>)],
        base: usize,
    ) -> &[Run] {
        let page = |r: &Run| r.key as u32;
        if self.runs.windows(2).all(|w| page(&w[0]) == page(&w[1])) {
            return &self.runs;
        }
        // Stable: within a band and page the runs stay nearest first.
        self.runs.sort_by_key(|r| r.key);
        self.scratch.clear();
        for r in &mut self.runs {
            let (s, e) = (r.start as usize, (r.start + r.len) as usize);
            r.start = (base + self.scratch.len()) as u32;
            self.scratch
                .extend(draws[s..e].iter().copied().zip(pages[s..e].iter().copied()));
        }
        for (i, &(draw, p)) in self.scratch.iter().enumerate() {
            draws[base + i] = draw;
            pages[base + i] = p;
        }
        &self.runs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(blocks: f64) -> u64 {
        (blocks * blocks).to_bits()
    }

    /// Runs of (distance in blocks, page, draws), as the list builder pushes them.
    fn list(runs: &[(f64, u32, u32)]) -> (PageRuns, Vec<[u32; 5]>, Vec<(u32, Option<u32>)>) {
        let mut grouped = PageRuns::default();
        let (mut draws, mut pages) = (Vec::new(), Vec::new());
        for (n, &(distance, page, count)) in runs.iter().enumerate() {
            let start = draws.len();
            for k in 0..count {
                draws.push([n as u32, k, 0, 0, 0]);
                pages.push((page, None));
            }
            grouped.push(
                bits(distance),
                page,
                start,
                count as usize,
                [n as u32, 0, 0],
            );
        }
        (grouped, draws, pages)
    }

    #[test]
    fn a_band_draws_each_page_once_keeping_the_order_inside_it() {
        let (mut grouped, mut draws, mut pages) = list(&[
            (10.0, 1, 2),
            (20.0, 0, 1),
            (30.0, 1, 3),
            (40.0, 0, 2),
            (70.0, 1, 1),
        ]);
        let runs: Vec<_> = grouped
            .regroup(&mut draws, &mut pages, 0)
            .iter()
            .map(|r| (r.tag[0], r.start, r.len))
            .collect();
        assert_eq!(
            runs,
            [(1, 0, 1), (3, 1, 2), (0, 3, 2), (2, 5, 3), (4, 8, 1)]
        );
        let order: Vec<_> = draws.iter().map(|d| (d[0], d[1])).collect();
        assert_eq!(
            order,
            [
                (1, 0),
                (3, 0),
                (3, 1),
                (0, 0),
                (0, 1),
                (2, 0),
                (2, 1),
                (2, 2),
                (4, 0)
            ]
        );
        let pages: Vec<_> = pages.iter().map(|p| p.0).collect();
        assert_eq!(pages, [0, 0, 0, 1, 1, 1, 1, 1, 1]);
    }

    #[test]
    fn bands_stay_in_distance_order() {
        let (mut grouped, mut draws, mut pages) =
            list(&[(10.0, 1, 1), (100.0, 0, 1), (200.0, 0, 1)]);
        grouped.regroup(&mut draws, &mut pages, 0);
        let order: Vec<_> = draws.iter().map(|d| d[0]).collect();
        assert_eq!(order, [0, 1, 2]);
    }

    #[test]
    fn narrow_bands_preserve_draws_pages_and_culling_tags() {
        for band in [8.0, 16.0, 32.0, 64.0] {
            let mut grouped = PageRuns {
                band,
                ..PageRuns::default()
            };
            let mut draws = vec![[99; 5]];
            let mut pages = vec![(9, None)];
            for (i, distance) in [4.0, 20.0, 36.0, 52.0, 68.0].into_iter().enumerate() {
                let page = (i % 2) as u32;
                grouped.push(
                    bits(distance),
                    page,
                    draws.len(),
                    1,
                    [i as u32, 100 + i as u32, 6],
                );
                draws.push([i as u32, 1, 0, 200 + i as u32, i as u32]);
                pages.push((page, None));
            }
            let before = draws.clone();
            let runs = grouped.regroup(&mut draws, &mut pages, 1);
            assert_eq!(draws[0], [99; 5]);
            for run in runs {
                let i = run.tag[0] as usize;
                assert_eq!(run.tag, [i as u32, 100 + i as u32, 6]);
                assert_eq!(draws[run.start as usize], before[i + 1]);
                assert_eq!(pages[run.start as usize], ((i % 2) as u32, None));
            }
            assert_eq!(runs.len(), 5);
            if band <= 16.0 {
                assert_eq!(draws, before, "separate bands retain nearest-first order");
            }
        }
    }

    #[test]
    fn one_page_and_an_offset_list() {
        let (mut grouped, mut draws, mut pages) = list(&[(10.0, 3, 2), (90.0, 3, 1)]);
        let before = draws.clone();
        assert_eq!(grouped.regroup(&mut draws, &mut pages, 0)[1].start, 2);
        assert_eq!(draws, before);
        // A list after other draws: starts are absolute.
        let mut grouped = PageRuns::default();
        let mut draws = vec![[9; 5], [7, 0, 0, 0, 0], [8, 0, 0, 0, 0]];
        let mut pages = vec![(5, None), (1, None), (0, None)];
        grouped.push(bits(5.0), 1, 1, 1, [0; 3]);
        grouped.push(bits(6.0), 0, 2, 1, [1, 0, 0]);
        let runs = grouped.regroup(&mut draws, &mut pages, 1).to_vec();
        assert_eq!(draws, [[9; 5], [8, 0, 0, 0, 0], [7, 0, 0, 0, 0]]);
        assert_eq!(pages, [(5, None), (0, None), (1, None)]);
        assert_eq!((runs[0].tag[0], runs[0].start), (1, 1));
        // Empty runs are left out.
        grouped.clear();
        grouped.push(bits(1.0), 0, 0, 0, [0; 3]);
        assert!(grouped.regroup(&mut [], &mut [], 0).is_empty());
    }
}
