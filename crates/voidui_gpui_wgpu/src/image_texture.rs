//! Image allocations follow their owners and retained scenes. Atlas keys contain
//! only weak references, so a renderer cannot keep an unused image alive forever.
use std::{
    hash::{Hash, Hasher},
    sync::{Arc, Weak},
};

#[derive(Clone, Debug)]
pub struct ImageTexture(Arc<()>);
impl Default for ImageTexture {
    fn default() -> Self {
        Self::new()
    }
}
impl ImageTexture {
    pub fn new() -> Self {
        Self(Arc::new(()))
    }
    pub(crate) fn key(&self) -> ImageTextureKey {
        ImageTextureKey(Arc::downgrade(&self.0))
    }
}

/// Pointer identity stays unique while an atlas holds the weak allocation.
#[derive(Clone, Debug)]
pub struct ImageTextureKey(Weak<()>);
impl ImageTextureKey {
    pub(crate) fn is_alive(&self) -> bool {
        self.0.strong_count() != 0
    }
}
impl PartialEq for ImageTextureKey {
    fn eq(&self, other: &Self) -> bool {
        self.0.ptr_eq(&other.0)
    }
}
impl Eq for ImageTextureKey {}
impl Hash for ImageTextureKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.as_ptr().hash(state);
    }
}

/// Sampling uses standard CSS image-rendering behavior. Pixelated preserves an
/// integer nearest-neighbor enlargement before smoothing the remaining fraction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageSampling {
    #[default]
    Smooth,
    CrispEdges,
    Pixelated,
}

#[derive(Clone, Debug)]
pub struct PaintImage {
    pub bounds: crate::Bounds<crate::Pixels>,
    pub clip_bounds: crate::Bounds<crate::Pixels>,
    pub corner_radii: crate::Corners<crate::Pixels>,
    pub opacity: f32,
    pub sampling: ImageSampling,
}
/// Tightly packed, premultiplied BGRA8 pixels and their physical dimensions.
pub type ImagePixels<'a> = (crate::Size<crate::DevicePixels>, std::borrow::Cow<'a, [u8]>);

impl crate::Painter<'_> {
    pub fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    /// Upload premultiplied BGRA8 only on an atlas miss. CPU pixels need not be
    /// retained after this call; device recovery invokes the builder again.
    pub fn paint_image<'b>(
        &mut self,
        texture: &ImageTexture,
        image: PaintImage,
        build: &mut dyn FnMut() -> crate::Result<ImagePixels<'b>>,
    ) -> crate::Result<()> {
        use crate::*;
        if image.opacity <= 0.
            || image
                .bounds
                .intersect(&image.clip_bounds)
                .intersect(&self.mask.bounds)
                .is_empty()
        {
            return Ok(());
        }
        let key = AtlasKey::ManagedImage(texture.key());
        let Some(tile) = self
            .atlas
            .get_or_insert_with(&key, &mut || build().map(Some))?
        else {
            return Ok(());
        };
        // Scenes can outlive widgets and remain replayable until explicitly cleared.
        self.scene
            .image_textures
            .push((self.scene.len(), texture.clone()));
        self.scene.insert_primitive(PolychromeSprite {
            order: 0,
            spatial_id: 0,
            spatial_pad: 0,
            pad: 4 | match image.sampling {
                ImageSampling::Smooth => 0,
                ImageSampling::CrispEdges => 1,
                ImageSampling::Pixelated => 2,
            },
            grayscale: false.into(),
            opacity: image.opacity.clamp(0., 1.),
            bounds: image.bounds.scale(self.scale_factor),
            content_mask: self
                .mask
                .intersect(&ContentMask {
                    bounds: image.clip_bounds,
                })
                .scale(self.scale_factor),
            corner_radii: image.corner_radii.scale(self.scale_factor),
            rounded_bounds: image.clip_bounds.scale(self.scale_factor),
            tile,
        });
        Ok(())
    }
}
