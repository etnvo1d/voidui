// Derived from Zed/GPUI, revision e2534d2357a80795d2c372d31268748e7ee992e5.
// Licensed under Apache-2.0; see LICENSE-APACHE and UPSTREAM.md.
// Modified for the standalone voidui rendering package.

use crate::{
    AtlasKey, AtlasTextureId, AtlasTextureKind, AtlasTextureList, AtlasTile, Bounds, DevicePixels,
    PlatformAtlas, Point, Size,
};
use anyhow::{Context as _, Result};
use etagere::{BucketedAtlasAllocator, size2};
use parking_lot::Mutex;
use rustc_hash::FxHashMap;
use std::{borrow::Cow, ops, sync::Arc};

use crate::WgpuContext;

fn device_size_to_etagere(size: Size<DevicePixels>) -> etagere::Size {
    size2(size.width.0, size.height.0)
}

fn etagere_point_to_device(point: etagere::Point) -> Point<DevicePixels> {
    Point {
        x: DevicePixels(point.x),
        y: DevicePixels(point.y),
    }
}

pub struct WgpuAtlas(Mutex<WgpuAtlasState>);

struct PendingUpload {
    id: AtlasTextureId,
    bounds: Bounds<DevicePixels>,
    data: Vec<u8>,
}

struct WgpuAtlasState {
    glyphs: FxHashMap<AtlasKey, (Arc<()>, u64, usize)>,
    frame: u64,
    glyph_budget: usize,
    glyph_bytes: usize,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    max_texture_size: u32,
    color_texture_format: wgpu::TextureFormat,
    storage: WgpuAtlasStorage,
    tiles_by_key: FxHashMap<AtlasKey, AtlasTile>,
    pending_uploads: Vec<PendingUpload>,
    managed_images: Vec<crate::ImageTextureKey>,
}

pub struct WgpuTextureInfo {
    pub view: wgpu::TextureView,
}

impl WgpuAtlas {
    pub fn new(
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        color_texture_format: wgpu::TextureFormat,
    ) -> Self {
        let max_texture_size = device.limits().max_texture_dimension_2d;
        WgpuAtlas(Mutex::new(WgpuAtlasState {
            glyphs: Default::default(),
            frame: 0,
            glyph_budget: 16 * 1024 * 1024,
            glyph_bytes: 0,
            device,
            queue,
            max_texture_size,
            color_texture_format,
            storage: WgpuAtlasStorage::default(),
            tiles_by_key: Default::default(),
            pending_uploads: Vec::new(),
            managed_images: Vec::new(),
        }))
    }

    pub fn from_context(context: &WgpuContext) -> Self {
        Self::new(
            context.device.clone(),
            context.queue.clone(),
            context.color_texture_format(),
        )
    }

    pub fn before_frame(&self) {
        let mut lock = self.0.lock();
        lock.frame += 1;
        if lock.glyph_bytes > lock.glyph_budget {
            let mut unused: Vec<_> = lock
                .glyphs
                .iter()
                .filter(|(_, (owner, _, _))| Arc::strong_count(owner) == 1)
                .map(|(key, (_, age, _))| (key.clone(), *age))
                .collect();
            unused.sort_unstable_by_key(|(_, age)| *age);
            for (key, _) in unused {
                if lock.glyph_bytes <= lock.glyph_budget {
                    break;
                }
                lock.remove(&key);
            }
        }
        // Scan image allocations only; glyph atlases can contain many thousands of keys.
        let mut keys = std::mem::take(&mut lock.managed_images);
        keys.retain(|key| {
            if key.is_alive() {
                true
            } else {
                lock.remove(&AtlasKey::ManagedImage(key.clone()));
                false
            }
        });
        lock.managed_images = keys;
        lock.flush_uploads();
    }

    pub fn get_texture_info(&self, id: AtlasTextureId) -> WgpuTextureInfo {
        let lock = self.0.lock();
        let texture = &lock.storage[id];
        WgpuTextureInfo {
            view: texture.view.clone(),
        }
    }

    /// Clears all cached textures and tiles, forcing them to be recreated.
    /// Use this for incremental recovery when the device is still valid.
    pub fn clear(&self) {
        let mut lock = self.0.lock();
        lock.storage = WgpuAtlasStorage::default();
        lock.tiles_by_key.clear();
        lock.glyphs.clear();
        lock.glyph_bytes = 0;
        lock.pending_uploads.clear();
        lock.managed_images.clear();
    }

    /// Handles device lost by clearing all textures and cached tiles.
    /// The atlas will lazily recreate textures as needed on subsequent frames.
    pub fn handle_device_lost(&self, context: &WgpuContext) {
        let mut lock = self.0.lock();
        lock.device = context.device.clone();
        lock.queue = context.queue.clone();
        lock.color_texture_format = context.color_texture_format();
        lock.storage = WgpuAtlasStorage::default();
        lock.tiles_by_key.clear();
        lock.glyphs.clear();
        lock.glyph_bytes = 0;
        lock.pending_uploads.clear();
        lock.managed_images.clear();
    }
}

impl WgpuAtlas {
    pub fn set_glyph_budget(&self, bytes: usize) {
        self.0.lock().glyph_budget = bytes;
    }
    /// Tile payload excludes page fragmentation and includes pinned tiles.
    /// Allocated texture payload including page slack, unlike glyph_bytes().
    pub fn allocated_bytes(&self) -> u64 {
        let state = self.0.lock();
        [
            &state.storage.monochrome_textures,
            &state.storage.subpixel_textures,
            &state.storage.polychrome_textures,
        ]
        .into_iter()
        .flat_map(|list| list.textures.iter().flatten())
        .map(|t| {
            u64::from(t.texture.width())
                * u64::from(t.texture.height())
                * u64::from(t.bytes_per_pixel())
        })
        .sum()
    }
    pub fn glyph_bytes(&self) -> usize {
        self.0.lock().glyph_bytes
    }
}
impl PlatformAtlas for WgpuAtlas {
    fn pin(&self, key: &AtlasKey) -> Option<Arc<()>> {
        let mut state = self.0.lock();
        let frame = state.frame;
        let (owner, age, _) = state.glyphs.get_mut(key)?;
        *age = frame;
        Some(owner.clone())
    }
    fn get_or_insert_with<'a>(
        &self,
        key: &AtlasKey,
        build: &mut dyn FnMut() -> Result<Option<(Size<DevicePixels>, Cow<'a, [u8]>)>>,
    ) -> Result<Option<AtlasTile>> {
        let mut lock = self.0.lock();
        if let Some(tile) = lock.tiles_by_key.get(key) {
            Ok(Some(*tile))
        } else {
            profiling::scope!("new tile");
            let Some((size, bytes)) = build()? else {
                return Ok(None);
            };
            anyhow::ensure!(
                size.width.0 > 0 && size.height.0 > 0,
                "atlas dimensions must be positive"
            );
            anyhow::ensure!(
                size.width.0 as u32 <= lock.max_texture_size
                    && size.height.0 as u32 <= lock.max_texture_size,
                "atlas image exceeds the GPU texture limit"
            );
            let channels = if key.texture_kind() == AtlasTextureKind::Monochrome {
                1usize
            } else {
                4usize
            };
            let expected = (size.width.0 as usize)
                .checked_mul(size.height.0 as usize)
                .and_then(|pixels| pixels.checked_mul(channels));
            anyhow::ensure!(
                expected == Some(bytes.len()),
                "atlas pixel byte length does not match dimensions/format"
            );
            let tile = lock
                .allocate(size, key.texture_kind())
                .context("failed to allocate")?;
            lock.upload_texture(tile.texture_id, tile.bounds, &bytes);
            lock.tiles_by_key.insert(key.clone(), tile);
            if matches!(key, AtlasKey::Glyph(_)) {
                let frame = lock.frame;
                lock.glyph_bytes += bytes.len();
                lock.glyphs
                    .insert(key.clone(), (Arc::new(()), frame, bytes.len()));
            }
            if let AtlasKey::ManagedImage(key) = key {
                lock.managed_images.push(key.clone());
            }
            Ok(Some(tile))
        }
    }

    fn remove(&self, key: &AtlasKey) {
        let mut state = self.0.lock();
        if let AtlasKey::ManagedImage(key) = key {
            state.managed_images.retain(|v| v != key);
        }
        state.remove(key);
    }
}

impl WgpuAtlasState {
    fn remove(&mut self, key: &AtlasKey) {
        let lock = self;
        if let Some((_, _, bytes)) = lock.glyphs.remove(key) {
            lock.glyph_bytes -= bytes;
        }
        let Some(tile) = lock.tiles_by_key.remove(key) else {
            return;
        };
        let id = tile.texture_id;

        let Some(texture_slot) = lock.storage[id.kind].textures.get_mut(id.index as usize) else {
            return;
        };

        if let Some(mut texture) = texture_slot.take() {
            texture.allocator.deallocate(tile.tile_id.into());
            texture.decrement_ref_count();
            if texture.is_unreferenced() {
                lock.pending_uploads
                    .retain(|upload| upload.id != texture.id);
                lock.storage[id.kind]
                    .free_list
                    .push(texture.id.index as usize);
            } else {
                *texture_slot = Some(texture);
            }
        }
    }

    fn allocate(
        &mut self,
        size: Size<DevicePixels>,
        texture_kind: AtlasTextureKind,
    ) -> Option<AtlasTile> {
        {
            let textures = &mut self.storage[texture_kind];

            if let Some(tile) = textures
                .iter_mut()
                .rev()
                .find_map(|texture| texture.allocate(size))
            {
                return Some(tile);
            }
        }

        let texture = self.push_texture(size, texture_kind);
        texture.allocate(size)
    }

    fn push_texture(
        &mut self,
        min_size: Size<DevicePixels>,
        kind: AtlasTextureKind,
    ) -> &mut WgpuAtlasTexture {
        const DEFAULT_ATLAS_SIZE: Size<DevicePixels> = Size {
            width: DevicePixels(1024),
            height: DevicePixels(1024),
        };
        let max_texture_size = self.max_texture_size as i32;
        let max_atlas_size = Size {
            width: DevicePixels(max_texture_size),
            height: DevicePixels(max_texture_size),
        };

        let size = min_size.min(&max_atlas_size).max(&DEFAULT_ATLAS_SIZE);
        let format = match kind {
            AtlasTextureKind::Monochrome => wgpu::TextureFormat::R8Unorm,
            AtlasTextureKind::Subpixel | AtlasTextureKind::Polychrome => self.color_texture_format,
        };

        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("atlas"),
            size: wgpu::Extent3d {
                width: size.width.0 as u32,
                height: size.height.0 as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let texture_list = &mut self.storage[kind];
        let index = texture_list.free_list.pop();

        let atlas_texture = WgpuAtlasTexture {
            id: AtlasTextureId {
                index: index.unwrap_or(texture_list.textures.len()) as u32,
                kind,
            },
            allocator: BucketedAtlasAllocator::new(device_size_to_etagere(size)),
            format,
            texture,
            view,
            live_atlas_keys: 0,
        };

        if let Some(ix) = index {
            texture_list.textures[ix] = Some(atlas_texture);
            texture_list
                .textures
                .get_mut(ix)
                .and_then(|t| t.as_mut())
                .expect("texture must exist")
        } else {
            texture_list.textures.push(Some(atlas_texture));
            texture_list
                .textures
                .last_mut()
                .and_then(|t| t.as_mut())
                .expect("texture must exist")
        }
    }

    fn upload_texture(&mut self, id: AtlasTextureId, bounds: Bounds<DevicePixels>, bytes: &[u8]) {
        let data = self
            .storage
            .get(id)
            .map(|texture| swizzle_upload_data(bytes, texture.format))
            .unwrap_or_else(|| bytes.to_vec());

        self.pending_uploads
            .push(PendingUpload { id, bounds, data });
    }

    fn flush_uploads(&mut self) {
        for upload in self.pending_uploads.drain(..) {
            let Some(texture) = self.storage.get(upload.id) else {
                continue;
            };
            let bytes_per_pixel = texture.bytes_per_pixel();

            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: upload.bounds.origin.x.0 as u32,
                        y: upload.bounds.origin.y.0 as u32,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &upload.data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(upload.bounds.size.width.0 as u32 * bytes_per_pixel as u32),
                    rows_per_image: None,
                },
                wgpu::Extent3d {
                    width: upload.bounds.size.width.0 as u32,
                    height: upload.bounds.size.height.0 as u32,
                    depth_or_array_layers: 1,
                },
            );
        }
    }
}

#[derive(Default)]
struct WgpuAtlasStorage {
    monochrome_textures: AtlasTextureList<WgpuAtlasTexture>,
    subpixel_textures: AtlasTextureList<WgpuAtlasTexture>,
    polychrome_textures: AtlasTextureList<WgpuAtlasTexture>,
}

impl ops::Index<AtlasTextureKind> for WgpuAtlasStorage {
    type Output = AtlasTextureList<WgpuAtlasTexture>;
    fn index(&self, kind: AtlasTextureKind) -> &Self::Output {
        match kind {
            AtlasTextureKind::Monochrome => &self.monochrome_textures,
            AtlasTextureKind::Subpixel => &self.subpixel_textures,
            AtlasTextureKind::Polychrome => &self.polychrome_textures,
        }
    }
}

impl ops::IndexMut<AtlasTextureKind> for WgpuAtlasStorage {
    fn index_mut(&mut self, kind: AtlasTextureKind) -> &mut Self::Output {
        match kind {
            AtlasTextureKind::Monochrome => &mut self.monochrome_textures,
            AtlasTextureKind::Subpixel => &mut self.subpixel_textures,
            AtlasTextureKind::Polychrome => &mut self.polychrome_textures,
        }
    }
}

impl WgpuAtlasStorage {
    fn get(&self, id: AtlasTextureId) -> Option<&WgpuAtlasTexture> {
        self[id.kind]
            .textures
            .get(id.index as usize)
            .and_then(|t| t.as_ref())
    }
}

impl ops::Index<AtlasTextureId> for WgpuAtlasStorage {
    type Output = WgpuAtlasTexture;
    fn index(&self, id: AtlasTextureId) -> &Self::Output {
        let textures = match id.kind {
            AtlasTextureKind::Monochrome => &self.monochrome_textures,
            AtlasTextureKind::Subpixel => &self.subpixel_textures,
            AtlasTextureKind::Polychrome => &self.polychrome_textures,
        };
        textures[id.index as usize]
            .as_ref()
            .expect("texture must exist")
    }
}

struct WgpuAtlasTexture {
    id: AtlasTextureId,
    allocator: BucketedAtlasAllocator,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    format: wgpu::TextureFormat,
    live_atlas_keys: u32,
}

impl WgpuAtlasTexture {
    fn allocate(&mut self, size: Size<DevicePixels>) -> Option<AtlasTile> {
        let allocation = self.allocator.allocate(device_size_to_etagere(size))?;
        let tile = AtlasTile {
            texture_id: self.id,
            tile_id: allocation.id.into(),
            padding: 0,
            bounds: Bounds {
                origin: etagere_point_to_device(allocation.rectangle.min),
                size,
            },
        };
        self.live_atlas_keys += 1;
        Some(tile)
    }

    fn bytes_per_pixel(&self) -> u8 {
        match self.format {
            wgpu::TextureFormat::R8Unorm => 1,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm => 4,
            _ => 4,
        }
    }

    fn decrement_ref_count(&mut self) {
        self.live_atlas_keys -= 1;
    }

    fn is_unreferenced(&self) -> bool {
        self.live_atlas_keys == 0
    }
}

fn swizzle_upload_data(bytes: &[u8], format: wgpu::TextureFormat) -> Vec<u8> {
    match format {
        wgpu::TextureFormat::Rgba8Unorm => {
            let mut data = bytes.to_vec();
            for pixel in data.chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
            data
        }
        _ => bytes.to_vec(),
    }
}

#[cfg(all(test, not(target_family = "wasm")))]
mod tests {
    use super::*;
    use crate::block_on;
    use crate::{ImageId, RenderImageParams};
    use std::sync::Arc;

    fn test_device_and_queue() -> anyhow::Result<(Arc<wgpu::Device>, Arc<wgpu::Queue>)> {
        block_on(async {
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends: wgpu::Backends::all(),
                flags: wgpu::InstanceFlags::default(),
                backend_options: wgpu::BackendOptions::default(),
                memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
                display: None,
            });
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::LowPower,
                    compatible_surface: None,
                    force_fallback_adapter: false,
                })
                .await
                .map_err(|error| anyhow::anyhow!("failed to request adapter: {error}"))?;
            let (device, queue) = adapter
                .request_device(&wgpu::DeviceDescriptor {
                    label: Some("wgpu_atlas_test_device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::downlevel_defaults()
                        .using_resolution(adapter.limits())
                        .using_alignment(adapter.limits()),
                    memory_hints: wgpu::MemoryHints::MemoryUsage,
                    trace: wgpu::Trace::Off,
                    experimental_features: wgpu::ExperimentalFeatures::disabled(),
                })
                .await
                .map_err(|error| anyhow::anyhow!("failed to request device: {error}"))?;
            Ok((Arc::new(device), Arc::new(queue)))
        })
    }

    #[test]
    fn pinned_glyphs_survive_budget_pressure_until_the_scene_releases_them() -> anyhow::Result<()> {
        let (device, queue) = test_device_and_queue()?;
        let atlas = WgpuAtlas::new(device, queue, wgpu::TextureFormat::Bgra8Unorm);
        let key = AtlasKey::Glyph(crate::RenderGlyphParams {
            font_id: crate::FontId(0),
            glyph_id: crate::GlyphId(1),
            font_size: crate::px(16.),
            subpixel_variant: Default::default(),
            scale_factor: 1.,
            is_emoji: false,
            subpixel_rendering: false,
            dilation: 0,
        });
        atlas.get_or_insert_with(&key, &mut || {
            Ok(Some((
                crate::size(DevicePixels(8), DevicePixels(8)),
                Cow::Owned(vec![255; 64]),
            )))
        })?;
        let lease = atlas.pin(&key).unwrap();
        atlas.set_glyph_budget(0);
        atlas.before_frame();
        assert_eq!(atlas.glyph_bytes(), 64);
        drop(lease);
        atlas.before_frame();
        assert_eq!(atlas.glyph_bytes(), 0);
        assert!(!atlas.0.lock().tiles_by_key.contains_key(&key));
        Ok(())
    }
    #[test]
    fn image_allocations_follow_scene_lifetime() -> anyhow::Result<()> {
        let (device, queue) = test_device_and_queue()?;
        let atlas = WgpuAtlas::new(device, queue, wgpu::TextureFormat::Bgra8Unorm);
        let texture = crate::ImageTexture::new();
        let key = AtlasKey::ManagedImage(texture.key());
        let mut scene = crate::Scene::default();
        let text = Arc::new(crate::TextSystem::new(Arc::new(
            crate::ParleyTextSystem::new_without_system_fonts("unused"),
        )));
        let bounds = crate::Bounds::new(
            crate::point(crate::px(0.), crate::px(0.)),
            crate::size(crate::px(8.), crate::px(8.)),
        );
        let mut painter = crate::Painter::new(&mut scene, &atlas, text, bounds.size, 1.)?;
        painter.paint_image(
            &texture,
            crate::PaintImage {
                bounds,
                clip_bounds: bounds,
                corner_radii: Default::default(),
                opacity: 1.,
                sampling: Default::default(),
            },
            &mut || {
                Ok((
                    crate::size(DevicePixels(1), DevicePixels(1)),
                    Cow::Owned(vec![0, 0, 255, 255]),
                ))
            },
        )?;
        drop(painter);
        drop(texture);
        atlas.before_frame();
        assert!(
            atlas.0.lock().tiles_by_key.contains_key(&key),
            "a retained scene must keep its atlas tile valid"
        );
        let mut replay = crate::Scene::default();
        replay.replay(0..scene.len(), &scene);
        scene.clear();
        atlas.before_frame();
        assert!(
            atlas.0.lock().tiles_by_key.contains_key(&key),
            "replay must retain image ownership"
        );
        replay.clear();
        atlas.before_frame();
        let state = atlas.0.lock();
        assert!(!state.tiles_by_key.contains_key(&key));
        assert!(state.managed_images.is_empty());
        assert!(
            state
                .storage
                .polychrome_textures
                .textures
                .iter()
                .all(Option::is_none)
        );
        Ok(())
    }

    #[test]
    fn before_frame_skips_uploads_for_removed_texture() -> anyhow::Result<()> {
        let (device, queue) = test_device_and_queue()?;

        let atlas = WgpuAtlas::new(device, queue, wgpu::TextureFormat::Bgra8Unorm);
        let key = AtlasKey::Image(RenderImageParams {
            image_id: ImageId(1),
            frame_index: 0,
        });
        let size = Size {
            width: DevicePixels(1),
            height: DevicePixels(1),
        };
        let mut build = || Ok(Some((size, Cow::Owned(vec![0, 0, 0, 255]))));

        // Regression test: before the fix, this panicked in flush_uploads
        atlas
            .get_or_insert_with(&key, &mut build)?
            .expect("tile should be created");
        atlas.remove(&key);
        atlas.before_frame();
        Ok(())
    }

    #[test]
    fn remove_deallocates_tile_space_for_reuse() -> anyhow::Result<()> {
        let (device, queue) = test_device_and_queue()?;
        let atlas = WgpuAtlas::new(device, queue, wgpu::TextureFormat::Bgra8Unorm);

        let small = Size {
            width: DevicePixels(64),
            height: DevicePixels(64),
        };
        let big = Size {
            width: DevicePixels(700),
            height: DevicePixels(700),
        };

        let make_key = |image_id: usize| {
            AtlasKey::Image(RenderImageParams {
                image_id: ImageId(image_id as u64),
                frame_index: 0,
            })
        };
        let insert = |key: &AtlasKey, size: Size<DevicePixels>| {
            let byte_count = (size.width.0 as usize) * (size.height.0 as usize) * 4;
            atlas
                .get_or_insert_with(key, &mut || {
                    Ok(Some((size, Cow::Owned(vec![0u8; byte_count]))))
                })
                .expect("allocation should succeed")
                .expect("callback returns Some")
        };

        let keeper_key = make_key(1);
        let big_key_a = make_key(2);
        let big_key_b = make_key(3);

        let keeper_tile = insert(&keeper_key, small);
        let tile_a = insert(&big_key_a, big);
        assert_eq!(keeper_tile.texture_id, tile_a.texture_id);

        atlas.remove(&big_key_a);
        let tile_b = insert(&big_key_b, big);
        assert_eq!(tile_b.texture_id, keeper_tile.texture_id);
        Ok(())
    }

    #[test]
    fn swizzle_upload_data_preserves_bgra_uploads() {
        let input = vec![0x10, 0x20, 0x30, 0x40];
        assert_eq!(
            swizzle_upload_data(&input, wgpu::TextureFormat::Bgra8Unorm),
            input
        );
    }

    #[test]
    fn swizzle_upload_data_converts_bgra_to_rgba() {
        let input = vec![0x10, 0x20, 0x30, 0x40, 0xAA, 0xBB, 0xCC, 0xDD];
        assert_eq!(
            swizzle_upload_data(&input, wgpu::TextureFormat::Rgba8Unorm),
            vec![0x30, 0x20, 0x10, 0x40, 0xCC, 0xBB, 0xAA, 0xDD]
        );
    }
}
