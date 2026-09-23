//! File-backed image descriptions with bounded decoded variants and weak identity.
use super::{Image, MediaLimits};
use crate::cache::{BudgetCache, CacheBudget, CacheStats};
use anyhow::Result;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak},
    time::SystemTime,
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct SourceKey {
    path: PathBuf,
    len: u64,
    modified: Option<SystemTime>,
}
struct Asset {
    key: SourceKey,
    size: [f32; 2],
    limits: MediaLimits,
}
/// Cheap metadata. Pixel decoding happens only when a mounted asset image asks
/// for a physical-size variant. File revision is part of identity. A modified file gets a new description.
#[derive(Clone)]
pub struct ImageAsset(Arc<Asset>);
impl PartialEq for ImageAsset {
    fn eq(&self, other: &Self) -> bool {
        self.0.key == other.0.key && self.0.limits == other.0.limits
    }
}
type VariantKey = (SourceKey, u32, u32, MediaLimits);
struct Variants {
    live: HashMap<VariantKey, Weak<Image>>,
    recent: BudgetCache<VariantKey, Arc<Image>>,
}
fn variants() -> &'static Mutex<Variants> {
    static CACHE: OnceLock<Mutex<Variants>> = OnceLock::new();
    CACHE.get_or_init(|| {
        Mutex::new(Variants {
            live: HashMap::new(),
            recent: BudgetCache::new(CacheBudget::default()),
        })
    })
}
impl ImageAsset {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_limits(path, MediaLimits::default())
    }
    pub fn open_with_limits(path: impl AsRef<Path>, limits: MediaLimits) -> Result<Self> {
        let path = path.as_ref();
        let metadata = std::fs::metadata(path)?;
        anyhow::ensure!(
            metadata.len() <= limits.max_input_bytes as u64,
            "image source exceeds MediaLimits"
        );
        let key = SourceKey {
            path: path.to_owned(),
            len: metadata.len(),
            modified: metadata.modified().ok(),
        };
        // Only metadata keys are stored globally. Dead source handles are swept before
        // insertion, so cycling through a vault cannot retain its image pixels.
        struct Sources {
            values: HashMap<(SourceKey, MediaLimits), Weak<Asset>>,
            sweep_at: usize,
        }
        impl Default for Sources {
            fn default() -> Self {
                Self {
                    values: HashMap::new(),
                    sweep_at: 128,
                }
            }
        }
        static SOURCES: OnceLock<Mutex<Sources>> = OnceLock::new();
        let mut sources = SOURCES.get_or_init(Default::default).lock().unwrap();
        if let Some(asset) = sources
            .values
            .get(&(key.clone(), limits))
            .and_then(Weak::upgrade)
        {
            return Ok(Self(asset));
        }
        // Sweep on misses only. Revisiting many live images must not perform
        // an all-source scan for each metadata cache hit.
        if sources.values.len() >= sources.sweep_at {
            sources.values.retain(|_, asset| asset.strong_count() > 0);
            sources.sweep_at = sources.values.len().max(64).saturating_mul(2);
        }
        let size = Image::file_dimensions(path, limits)?;
        let asset = Arc::new(Asset {
            key: key.clone(),
            size,
            limits,
        });
        sources.values.insert((key, limits), Arc::downgrade(&asset));
        Ok(Self(asset))
    }
    pub fn intrinsic_size(&self) -> [f32; 2] {
        self.0.size
    }
    /// Call on a worker. Serial decode admission bounds transient full-source
    /// buffers in addition to the retained-byte budget. Active images can exceed
    /// that budget; offscreen views must release their handles.
    pub fn load(&self, width: u32, height: u32) -> Result<Arc<Image>> {
        self.0.limits.check_size(width, height)?;
        let key = (self.0.key.clone(), width, height, self.0.limits);
        static DECODE: Mutex<()> = Mutex::new(());
        let _decode = DECODE.lock().unwrap();
        {
            let mut cache = variants().lock().unwrap();
            cache.live.retain(|_, image| image.strong_count() > 0);
            if let Some(image) = cache.live.get(&key).and_then(Weak::upgrade) {
                return Ok(image);
            }
            if let Some(image) = cache.recent.get(&key) {
                return Ok(image.clone());
            }
        }
        let current = std::fs::metadata(&self.0.key.path)?;
        anyhow::ensure!(
            current.len() == self.0.key.len && current.modified().ok() == self.0.key.modified,
            "image changed while loading"
        );
        let image = Arc::new(Image::from_file_resized(
            &self.0.key.path,
            width,
            height,
            self.0.limits,
        )?);
        let mut cache = variants().lock().unwrap();
        cache.live.insert(key.clone(), Arc::downgrade(&image));
        let bytes = image.retained_bytes()
            + self.0.key.path.as_os_str().len()
            + std::mem::size_of::<VariantKey>();
        cache.recent.insert(key, image.clone(), bytes);
        Ok(image)
    }
    pub fn set_cache_budget(budget: CacheBudget) {
        variants().lock().unwrap().recent.set_budget(budget);
    }
    pub fn cache_stats() -> CacheStats {
        variants().lock().unwrap().recent.stats()
    }
}
