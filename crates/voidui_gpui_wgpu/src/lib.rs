//! Standalone rendering package: scene construction, CPU text layout, glyph atlas and WGPU.
//! No GPUI application, widget tree, window runtime or event loop is required.

mod bounds_tree;
mod color;
mod geometry;
mod painter;
mod parley_text_system;
mod path_builder;
mod scene;
mod shared_string;
mod text_system;
mod types;
mod wgpu_atlas;
mod wgpu_context;
mod wgpu_renderer;

pub use anyhow::Result;
pub use color::*;
pub use geometry::*;
pub use painter::*;
pub use parley_text_system::*;
pub use path_builder::*;
pub use pollster::block_on;
pub use scene::*;
pub use shared_string::SharedString;
pub use text_system::*;
pub use types::*;
pub use wgpu;
pub use wgpu_atlas::*;
pub use wgpu_context::*;
pub use wgpu_renderer::{
    GpuContext, RenderCacheOptions, RenderStats, WgpuRenderer, WgpuSurfaceConfig,
};

pub(crate) trait ResultExt<T> {
    fn log_err(self) -> Option<T>;
}
impl<T, E: std::fmt::Display> ResultExt<T> for std::result::Result<T, E> {
    fn log_err(self) -> Option<T> {
        self.map_err(|error| log::error!("{error}")).ok()
    }
}

/// Opaque image identity owned by the caller. Do not reuse while atlas entries are live.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ImageId(pub u64);
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RenderImageParams {
    pub image_id: ImageId,
    pub frame_index: usize,
}
/// Caller-rasterized SVG identity and dimensions, used only as an atlas key.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RenderSvgParams {
    pub id: u64,
    pub size: Size<DevicePixels>,
}

/// CPU text layout cache; no native window is required. Call finish_frame after each frame.
pub use parley;

mod gradient;
pub use gradient::*;

mod image_texture;
pub use image_texture::*;

mod spatial;
pub use spatial::{Affine, ClipChain, PaintSpace, SpatialClip};

mod frame_cache;

pub mod resource_cache;
