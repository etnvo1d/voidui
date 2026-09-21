//! Shared immutable image resources. Load files or decode bytes before mounting a
//! widget (or in a scoped task); painting never performs file or network I/O.
#![doc = include_str!("../../docs/media.md")]

mod image;
mod svg_cache;
pub use image::{Image, MediaLimits, SvgOptions};
pub(crate) use image::{PreparedSvg, Raster};
pub use svg_cache::SvgRenderPolicy;
pub(crate) use svg_cache::{SvgRenderCache, SvgSource};

/// Cumulative work counters. Repainting a cached image does not increment them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MediaStats {
    pub image_decodes: u64,
    pub svg_parses: u64,
    pub svg_rasterizations: u64,
}
static IMAGE_DECODES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static SVG_PARSES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static SVG_RASTERS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub fn stats() -> MediaStats {
    use std::sync::atomic::Ordering::Relaxed;
    MediaStats {
        image_decodes: IMAGE_DECODES.load(Relaxed),
        svg_parses: SVG_PARSES.load(Relaxed),
        svg_rasterizations: SVG_RASTERS.load(Relaxed),
    }
}

/// Load font bytes explicitly for SVG text without introducing another dependency.
pub use resvg::usvg::fontdb;

pub(crate) mod view;
