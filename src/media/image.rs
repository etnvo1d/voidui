use crate::render::{self, ImageTexture};
use anyhow::{Context, Result, ensure};
use std::{
    borrow::Cow,
    io::{Cursor, Read},
    path::Path,
    sync::{Arc, Mutex, OnceLock, Weak},
};

/// Allocation limits apply before decoding and before each SVG rasterization.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MediaLimits {
    pub max_input_bytes: usize,
    pub max_pixels: u64,
    pub max_dimension: u32,
    pub max_svg_nodes: usize,
    pub max_svg_depth: usize,
}
impl Default for MediaLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 16 * 1024 * 1024,
            max_pixels: 16 * 1024 * 1024,
            max_dimension: 8192,
            max_svg_nodes: 100_000,
            max_svg_depth: 256,
        }
    }
}
impl MediaLimits {
    pub(crate) fn check_size(self, w: u32, h: u32) -> Result<()> {
        ensure!(
            w > 0
                && h > 0
                && w <= i32::MAX as u32
                && h <= i32::MAX as u32
                && w <= self.max_dimension
                && h <= self.max_dimension
                && u64::from(w) * u64::from(h) <= self.max_pixels,
            "image dimensions exceed MediaLimits"
        );
        Ok(())
    }
    pub(crate) fn read(self, path: &Path) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take((self.max_input_bytes as u64).saturating_add(1))
            .read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() <= self.max_input_bytes,
            "image source exceeds MediaLimits"
        );
        Ok(bytes)
    }
}
/// Fonts are explicit and shared. Icon-only applications do not scan system fonts.
#[derive(Clone)]
pub struct SvgOptions {
    pub limits: MediaLimits,
    pub fontdb: Arc<resvg::usvg::fontdb::Database>,
}
impl Default for SvgOptions {
    fn default() -> Self {
        static EMPTY: OnceLock<Arc<resvg::usvg::fontdb::Database>> = OnceLock::new();
        Self {
            limits: MediaLimits::default(),
            fontdb: EMPTY.get_or_init(|| Arc::new(Default::default())).clone(),
        }
    }
}
impl SvgOptions {
    pub(crate) fn options(&self) -> resvg::usvg::Options<'static> {
        let limits = self.limits;
        let resolver = resvg::usvg::ImageHrefResolver::default_data_resolver();
        resvg::usvg::Options {
            fontdb: self.fontdb.clone(),
            default_size: resvg::usvg::Size::from_wh(300., 150.).unwrap(),
            image_href_resolver: resvg::usvg::ImageHrefResolver {
                // Embedded images are allowed, but SVG assets cannot open arbitrary files.
                resolve_string: Box::new(|_, _| None),
                resolve_data: Box::new(move |mime, data, options| {
                    if data.len() > limits.max_input_bytes {
                        return None;
                    }
                    // Nested SVG data documents have their own resource tree; keep this
                    // boundary bounded by accepting embedded raster formats only.
                    let reader = ::image::ImageReader::new(Cursor::new(data.as_slice()))
                        .with_guessed_format()
                        .ok()?;
                    let (w, h) = reader.into_dimensions().ok()?;
                    limits.check_size(w, h).ok()?;
                    resolver(mime, data, options)
                }),
            },
            ..Default::default()
        }
    }
}

pub(crate) struct Raster {
    pub texture: ImageTexture,
    pub width: u32,
    pub height: u32,
    image_viewport: OnceLock<Arc<PreparedSvg>>,
}
impl Raster {
    pub(crate) fn size(&self) -> render::Size<render::DevicePixels> {
        render::size(
            render::DevicePixels(self.width as i32),
            render::DevicePixels(self.height as i32),
        )
    }

    fn new(width: u32, height: u32) -> Self {
        Self {
            texture: ImageTexture::new(),
            width,
            height,
            image_viewport: OnceLock::new(),
        }
    }
}

/// A parsed SVG shares only live raster identities. Pixel buffers are temporary;
/// neither this cache nor the atlas keeps an unused size variant alive.
pub(crate) struct PreparedSvg {
    pub tree: resvg::usvg::Tree,
    pub(crate) source: Arc<str>,
    fontdb: Arc<resvg::usvg::fontdb::Database>,
    limits: MediaLimits,
    rasters: Mutex<Vec<Weak<Raster>>>,
}
impl PreparedSvg {
    pub(crate) fn parse(source: &str, options: &SvgOptions) -> Result<Arc<Self>> {
        ensure!(
            source.len() <= options.limits.max_input_bytes,
            "SVG source exceeds MediaLimits"
        );
        use std::hash::{Hash, Hasher};
        type Cache = std::collections::HashMap<u64, Vec<Weak<PreparedSvg>>>;
        static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        source.hash(&mut hasher);
        options.limits.hash(&mut hasher);
        Arc::as_ptr(&options.fontdb).hash(&mut hasher);
        let key = hasher.finish();
        {
            let mut cache = CACHE.get_or_init(Default::default).lock().unwrap();
            // Weak metadata is periodically swept too; cycling through documents
            // must not grow an otherwise empty cache without bound.
            if cache.len() >= 128 {
                cache.retain(|_, v| {
                    v.retain(|w| w.strong_count() > 0);
                    !v.is_empty()
                });
            }
            if let Some(values) = cache.get_mut(&key) {
                values.retain(|v| v.strong_count() > 0);
                for value in values.iter().filter_map(Weak::upgrade) {
                    if value.source.as_ref() == source
                        && value.limits == options.limits
                        && Arc::ptr_eq(&value.fontdb, &options.fontdb)
                    {
                        return Ok(value);
                    }
                }
            }
        }
        let doc = resvg::usvg::roxmltree::Document::parse(source)?;
        let mut ancestors = Vec::new();
        for (count, node) in doc.descendants().enumerate() {
            let parent = node.parent().map(|n| n.id());
            while ancestors.last().copied() != parent {
                ancestors.pop();
            }
            ancestors.push(node.id());
            ensure!(
                count < options.limits.max_svg_nodes
                    && ancestors.len() <= options.limits.max_svg_depth,
                "SVG graph exceeds MediaLimits"
            );
        }
        let tree = resvg::usvg::Tree::from_xmltree(&doc, &options.options())?;
        super::SVG_PARSES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let value = Arc::new(Self {
            tree,
            source: source.into(),
            fontdb: options.fontdb.clone(),
            limits: options.limits,
            rasters: Mutex::new(Vec::new()),
        });
        CACHE
            .get()
            .unwrap()
            .lock()
            .unwrap()
            .entry(key)
            .or_default()
            .push(Arc::downgrade(&value));
        Ok(value)
    }
    pub(crate) fn size(&self) -> [f32; 2] {
        let s = self.tree.size();
        [s.width(), s.height()]
    }
    pub(crate) fn check_size(&self, w: u32, h: u32) -> Result<()> {
        self.limits.check_size(w, h)
    }
    pub(crate) fn raster(&self, w: u32, h: u32) -> Result<Arc<Raster>> {
        self.limits.check_size(w, h)?;
        let mut cache = self.rasters.lock().unwrap();
        cache.retain(|r| r.strong_count() > 0);
        for raster in cache.iter().filter_map(Weak::upgrade) {
            if raster.width == w && raster.height == h {
                return Ok(raster);
            }
        }
        let raster = Arc::new(Raster::new(w, h));
        cache.push(Arc::downgrade(&raster));
        Ok(raster)
    }
    pub(crate) fn image_pixels(&self, raster: &Raster) -> Result<Vec<u8>> {
        if let Some(view) = raster.image_viewport.get() {
            return view.pixels(raster);
        }
        let doc = resvg::usvg::roxmltree::Document::parse(&self.source)?;
        let root = doc.root_element();
        if root.attribute("viewBox").is_none()
            || self.size() == [raster.width as f32, raster.height as f32]
        {
            return self.pixels(raster);
        }
        // An SVG image retains preserveAspectRatio when its CSS image viewport
        // changes aspect ratio. Stretching the original raster would bypass it.
        // Edit XML attribute ranges reported by the parser, never search/replace
        // arbitrary text (which could also occur in paths, text or nested SVGs).
        let mut edits: Vec<_> = root
            .attributes()
            .filter(|a| matches!(a.name(), "width" | "height"))
            .map(|a| (a.range(), String::new()))
            .collect();
        let name_start = root.range().start + 1;
        let start = name_start
            + self.source[name_start..]
                .find(|c: char| c.is_ascii_whitespace() || c == '/' || c == '>')
                .unwrap();
        edits.push((
            start..start,
            format!(" width=\"{}\" height=\"{}\"", raster.width, raster.height),
        ));
        edits.sort_by_key(|(r, _)| std::cmp::Reverse(r.start));
        let mut source = self.source.to_string();
        for (range, value) in edits {
            source.replace_range(range, &value);
        }
        let parsed = Self::parse(
            &source,
            &SvgOptions {
                limits: self.limits,
                fontdb: self.fontdb.clone(),
            },
        )?;
        let _ = raster.image_viewport.set(parsed);
        raster.image_viewport.get().unwrap().pixels(raster)
    }
    pub(crate) fn pixels(&self, raster: &Raster) -> Result<Vec<u8>> {
        self.limits.check_size(raster.width, raster.height)?;
        super::SVG_RASTERS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut pixmap = resvg::tiny_skia::Pixmap::new(raster.width, raster.height)
            .context("SVG pixmap allocation failed")?;
        let [w, h] = self.size();
        resvg::render(
            &self.tree,
            resvg::tiny_skia::Transform::from_scale(
                raster.width as f32 / w,
                raster.height as f32 / h,
            ),
            &mut pixmap.as_mut(),
        );
        let mut bytes = pixmap.take();
        // tiny-skia already premultiplies alpha; the atlas consumes BGRA8.
        for pixel in bytes.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
        }
        Ok(bytes)
    }
}

#[derive(Clone)]
pub struct Image(Arc<ImageData>);
struct ImageData {
    size: [f32; 2],
    kind: ImageKind,
}
enum ImageKind {
    Bitmap {
        pixels: Arc<[u8]>,
        raster: Arc<Raster>,
    },
    Svg(Arc<PreparedSvg>),
}
impl PartialEq for Image {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Image {
    /// Retained bitmap bytes, or a conservative source-based SVG tree estimate.
    pub fn retained_bytes(&self) -> usize {
        match &self.0.kind {
            ImageKind::Bitmap { pixels, .. } => pixels.len(),
            ImageKind::Svg(svg) => {
                svg.source.len().saturating_mul(4) + std::mem::size_of::<PreparedSvg>()
            }
        }
    }
    pub(crate) fn file_dimensions(path: &Path, limits: MediaLimits) -> Result<[f32; 2]> {
        let reader = ::image::ImageReader::open(path)?.with_guessed_format()?;
        if reader.format().is_none() {
            return Ok(Self::from_bytes_with_options(
                &limits.read(path)?,
                &SvgOptions {
                    limits,
                    ..Default::default()
                },
            )?
            .intrinsic_size());
        }
        let (w, h) = reader.into_dimensions()?;
        limits.check_size(w, h)?;
        Ok([w as f32, h as f32])
    }
    pub(crate) fn from_file_resized(
        path: &Path,
        width: u32,
        height: u32,
        limits: MediaLimits,
    ) -> Result<Self> {
        let bytes = limits.read(path)?;
        let reader = ::image::ImageReader::new(Cursor::new(&bytes)).with_guessed_format()?;
        if reader.format().is_none() {
            return Self::from_bytes_with_options(
                &bytes,
                &SvgOptions {
                    limits,
                    ..Default::default()
                },
            );
        }
        let dimensions = ::image::ImageReader::new(Cursor::new(&bytes))
            .with_guessed_format()?
            .into_dimensions()?;
        limits.check_size(dimensions.0, dimensions.1)?;
        let decoded = reader.decode()?;
        // Keep the source aspect ratio and never upscale the stored pixels.
        // The widget applies CSS object-fit independently of this resolution.
        let scaled = decoded.thumbnail(width.min(dimensions.0), height.min(dimensions.1));
        let pixels = scaled.to_rgba8();
        super::IMAGE_DECODES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self::from_rgba_with_limits(pixels.width(), pixels.height(), pixels.into_raw(), limits)
    }

    /// Load and validate once. Clone the returned handle for repeated instances.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_bytes(&MediaLimits::default().read(path.as_ref())?)
    }
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        Self::from_bytes_with_options(bytes, &SvgOptions::default())
    }
    pub fn from_bytes_with_options(bytes: &[u8], options: &SvgOptions) -> Result<Self> {
        ensure!(
            bytes.len() <= options.limits.max_input_bytes,
            "image source exceeds MediaLimits"
        );
        let mut reader = ::image::ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
        if reader.format().is_none() {
            let source = std::str::from_utf8(bytes).context("unrecognized image format")?;
            let svg = PreparedSvg::parse(source, options)?;
            return Ok(Self(Arc::new(ImageData {
                size: svg.size(),
                kind: ImageKind::Svg(svg),
            })));
        }
        let (w, h) = ::image::ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()?
            .into_dimensions()?;
        options.limits.check_size(w, h)?;
        let mut limits = ::image::Limits::default();
        limits.max_image_width = Some(options.limits.max_dimension);
        limits.max_image_height = Some(options.limits.max_dimension);
        limits.max_alloc = Some(options.limits.max_pixels.saturating_mul(8));
        reader.limits(limits);
        let rgba = reader.decode()?.to_rgba8();
        super::IMAGE_DECODES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self::from_rgba_with_limits(w, h, rgba.into_raw(), options.limits)
    }
    /// Supply tightly packed straight-alpha RGBA8. Pixels are shared by all views.
    pub fn from_rgba(width: u32, height: u32, bytes: Vec<u8>) -> Result<Self> {
        Self::from_rgba_with_limits(width, height, bytes, MediaLimits::default())
    }
    pub fn from_rgba_with_limits(
        width: u32,
        height: u32,
        mut bytes: Vec<u8>,
        limits: MediaLimits,
    ) -> Result<Self> {
        limits.check_size(width, height)?;
        ensure!(
            u64::from(width) * u64::from(height) * 4 == bytes.len() as u64,
            "RGBA byte length does not match dimensions"
        );
        for p in bytes.as_chunks_mut::<4>().0 {
            let a = u16::from(p[3]);
            for c in &mut p[..3] {
                *c = ((u16::from(*c) * a + 127) / 255) as u8;
            }
            p.swap(0, 2);
        }
        Ok(Self(Arc::new(ImageData {
            size: [width as f32, height as f32],
            kind: ImageKind::Bitmap {
                pixels: bytes.into(),
                raster: Arc::new(Raster::new(width, height)),
            },
        })))
    }
    pub fn intrinsic_size(&self) -> [f32; 2] {
        self.0.size
    }
    pub(crate) fn svg_source(&self) -> Option<super::SvgSource> {
        match &self.0.kind {
            ImageKind::Svg(svg) => Some(super::SvgSource::Image(svg.clone())),
            ImageKind::Bitmap { .. } => None,
        }
    }
    pub(crate) fn raster(&self, w: u32, h: u32) -> Result<Arc<Raster>> {
        match &self.0.kind {
            ImageKind::Bitmap { raster, .. } => Ok(raster.clone()),
            ImageKind::Svg(svg) => svg.raster(w, h),
        }
    }
    pub(crate) fn pixels<'a>(
        &'a self,
        raster: &Raster,
    ) -> Result<(render::Size<render::DevicePixels>, Cow<'a, [u8]>)> {
        let bytes = match &self.0.kind {
            ImageKind::Bitmap { pixels, .. } => Cow::Borrowed(pixels.as_ref()),
            ImageKind::Svg(svg) => Cow::Owned(svg.image_pixels(raster)?),
        };
        Ok((
            render::size(
                render::DevicePixels(raster.width as i32),
                render::DevicePixels(raster.height as i32),
            ),
            bytes,
        ))
    }
}
