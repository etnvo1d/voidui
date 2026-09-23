// Derived from Zed/GPUI, revision e2534d2357a80795d2c372d31268748e7ee992e5.
// Licensed under Apache-2.0; see LICENSE-APACHE and UPSTREAM.md.
// Modified for the standalone voidui rendering package.

use crate::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, fmt::Debug, ops};

// Adapted from https://github.com/microsoft/terminal/blob/1283c0f5b99a2961673249fa77c6b986efb5086c/src/renderer/atlas/dwrite.cpp
// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.
/// Compute gamma correction ratios for subpixel text rendering.
#[allow(dead_code)]
pub fn get_gamma_correction_ratios(gamma: f32) -> [f32; 4] {
    const GAMMA_INCORRECT_TARGET_RATIOS: [[f32; 4]; 13] = [
        [0.0000 / 4.0, 0.0000 / 4.0, 0.0000 / 4.0, 0.0000 / 4.0], // gamma = 1.0
        [0.0166 / 4.0, -0.0807 / 4.0, 0.2227 / 4.0, -0.0751 / 4.0], // gamma = 1.1
        [0.0350 / 4.0, -0.1760 / 4.0, 0.4325 / 4.0, -0.1370 / 4.0], // gamma = 1.2
        [0.0543 / 4.0, -0.2821 / 4.0, 0.6302 / 4.0, -0.1876 / 4.0], // gamma = 1.3
        [0.0739 / 4.0, -0.3963 / 4.0, 0.8167 / 4.0, -0.2287 / 4.0], // gamma = 1.4
        [0.0933 / 4.0, -0.5161 / 4.0, 0.9926 / 4.0, -0.2616 / 4.0], // gamma = 1.5
        [0.1121 / 4.0, -0.6395 / 4.0, 1.1588 / 4.0, -0.2877 / 4.0], // gamma = 1.6
        [0.1300 / 4.0, -0.7649 / 4.0, 1.3159 / 4.0, -0.3080 / 4.0], // gamma = 1.7
        [0.1469 / 4.0, -0.8911 / 4.0, 1.4644 / 4.0, -0.3234 / 4.0], // gamma = 1.8
        [0.1627 / 4.0, -1.0170 / 4.0, 1.6051 / 4.0, -0.3347 / 4.0], // gamma = 1.9
        [0.1773 / 4.0, -1.1420 / 4.0, 1.7385 / 4.0, -0.3426 / 4.0], // gamma = 2.0
        [0.1908 / 4.0, -1.2652 / 4.0, 1.8650 / 4.0, -0.3476 / 4.0], // gamma = 2.1
        [0.2031 / 4.0, -1.3864 / 4.0, 1.9851 / 4.0, -0.3501 / 4.0], // gamma = 2.2
    ];

    const NORM13: f32 = ((0x10000 as f64) / (255.0 * 255.0) * 4.0) as f32;
    const NORM24: f32 = ((0x100 as f64) / (255.0) * 4.0) as f32;

    let index = ((gamma * 10.0).round() as usize).clamp(10, 22) - 10;
    let ratios = GAMMA_INCORRECT_TARGET_RATIOS[index];

    [
        ratios[0] * NORM13,
        ratios[1] * NORM24,
        ratios[2] * NORM13,
        ratios[3] * NORM24,
    ]
}

#[derive(PartialEq, Eq, Hash, Clone)]
pub enum AtlasKey {
    Glyph(RenderGlyphParams),
    Svg(RenderSvgParams),
    Image(RenderImageParams),
    ManagedImage(ImageTextureKey),
}

impl AtlasKey {
    /// Returns the texture kind for this atlas key.
    pub fn texture_kind(&self) -> AtlasTextureKind {
        match self {
            AtlasKey::Glyph(params) => {
                if params.is_emoji {
                    AtlasTextureKind::Polychrome
                } else if params.subpixel_rendering {
                    AtlasTextureKind::Subpixel
                } else {
                    AtlasTextureKind::Monochrome
                }
            }
            AtlasKey::Svg(_) => AtlasTextureKind::Monochrome,
            AtlasKey::Image(_) | AtlasKey::ManagedImage(_) => AtlasTextureKind::Polychrome,
        }
    }
}

impl From<RenderGlyphParams> for AtlasKey {
    fn from(params: RenderGlyphParams) -> Self {
        Self::Glyph(params)
    }
}

impl From<RenderSvgParams> for AtlasKey {
    fn from(params: RenderSvgParams) -> Self {
        Self::Svg(params)
    }
}

impl From<RenderImageParams> for AtlasKey {
    fn from(params: RenderImageParams) -> Self {
        Self::Image(params)
    }
}

pub trait PlatformAtlas {
    /// A retained scene pins its glyph tiles until the scene is replaced. GPU
    /// command buffers separately retain the textures they already reference.
    fn pin(&self, _key: &AtlasKey) -> Option<std::sync::Arc<()>> {
        None
    }

    fn get_or_insert_with<'a>(
        &self,
        key: &AtlasKey,
        build: &mut dyn FnMut() -> Result<Option<(Size<DevicePixels>, Cow<'a, [u8]>)>>,
    ) -> Result<Option<AtlasTile>>;
    fn remove(&self, key: &AtlasKey);

    #[cfg(test)]
    fn contains(&self, _key: &AtlasKey) -> bool {
        false
    }
}

#[doc(hidden)]
pub struct AtlasTextureList<T> {
    pub textures: Vec<Option<T>>,
    pub free_list: Vec<usize>,
}

impl<T> Default for AtlasTextureList<T> {
    fn default() -> Self {
        Self {
            textures: Vec::default(),
            free_list: Vec::default(),
        }
    }
}

impl<T> ops::Index<usize> for AtlasTextureList<T> {
    type Output = Option<T>;

    fn index(&self, index: usize) -> &Self::Output {
        &self.textures[index]
    }
}

impl<T> AtlasTextureList<T> {
    #[allow(unused)]
    pub fn drain(&mut self) -> std::vec::Drain<'_, Option<T>> {
        self.free_list.clear();
        self.textures.drain(..)
    }

    #[allow(dead_code)]
    pub fn iter_mut(&mut self) -> impl DoubleEndedIterator<Item = &mut T> {
        self.textures.iter_mut().flatten()
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct AtlasTile {
    /// The texture this tile belongs to.
    pub texture_id: AtlasTextureId,
    /// The unique ID of this tile within its texture.
    pub tile_id: TileId,
    /// Padding around the tile content in pixels.
    pub padding: u32,
    /// The bounds of this tile within the texture.
    pub bounds: Bounds<DevicePixels>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct AtlasTextureId {
    // We use u32 instead of usize for Metal Shader Language compatibility
    /// The index of this texture in the atlas.
    pub index: u32,
    /// The kind of content stored in this texture.
    pub kind: AtlasTextureKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(C)]
pub enum AtlasTextureKind {
    Monochrome = 0,
    Polychrome = 1,
    Subpixel = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(C)]
pub struct TileId(pub u32);

impl From<etagere::AllocId> for TileId {
    fn from(id: etagere::AllocId) -> Self {
        Self(id.serialize())
    }
}

impl From<TileId> for etagere::AllocId {
    fn from(id: TileId) -> Self {
        Self::deserialize(id.0)
    }
}
/// Indicates which region of the window is visible. Content falling outside of this mask will not be
/// rendered. Currently, only rectangular content masks are supported, but we give the mask its own type
/// to leave room to support more complex shapes in the future.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct ContentMask<P: Clone + Debug + Default + PartialEq> {
    /// The bounds
    pub bounds: Bounds<P>,
}

impl ContentMask<Pixels> {
    /// Scale the content mask's pixel units by the given scaling factor.
    pub fn scale(&self, factor: f32) -> ContentMask<ScaledPixels> {
        ContentMask {
            bounds: self.bounds.scale(factor),
        }
    }

    /// Intersect the content mask with the given content mask.
    pub fn intersect(&self, other: &Self) -> Self {
        let bounds = self.bounds.intersect(&other.bounds);
        ContentMask { bounds }
    }
}
/// How to align text within the element
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum TextAlign {
    /// Align the text to the left of the element
    #[default]
    Left,

    /// Center the text within the element
    Center,

    /// Align the text to the right of the element
    Right,
}

/// The properties that can be applied to an underline.
#[derive(Copy, Clone, Default, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct UnderlineStyle {
    /// The thickness of the underline.
    pub thickness: Pixels,

    /// The color of the underline.
    pub color: Option<Hsla>,

    /// Whether the underline should be wavy, like in a spell checker.
    pub wavy: bool,
}

/// The properties that can be applied to a strikethrough.
#[derive(Copy, Clone, Default, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct StrikethroughStyle {
    /// The thickness of the strikethrough.
    pub thickness: Pixels,

    /// The color of the strikethrough.
    pub color: Option<Hsla>,
}
/// The text rendering mode to use for drawing glyphs.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum TextRenderingMode {
    /// Use the platform's default text rendering mode.
    #[default]
    PlatformDefault,
    /// Use subpixel (ClearType-style) text rendering.
    Subpixel,
    /// Use grayscale text rendering.
    Grayscale,
}

/// Information about the GPU GPUI is running on.
#[derive(Default, Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct GpuSpecs {
    /// Whether the GPU is really a fake (like `llvmpipe`) running on the CPU.
    pub is_software_emulated: bool,
    /// The name of the device, as reported by Vulkan.
    pub device_name: String,
    /// The name of the driver, as reported by Vulkan.
    pub driver_name: String,
    /// Further information about the driver, as reported by Vulkan.
    pub driver_info: String,
}
