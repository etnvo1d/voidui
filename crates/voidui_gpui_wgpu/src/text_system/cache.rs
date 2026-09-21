//! Weak paragraph pool: identical content/style/width can share native layouts.
//! Other widths reuse shaping by cloning native data, never by reshaping text.
use crate::*;
use parking_lot::Mutex;
use rustc_hash::FxHashMap;
use std::{
    hash::{Hash, Hasher},
    ops::Deref,
    sync::{Arc, Weak},
};

/// Retain the caller's immutable runs, without allocating a second style array.
/// Equality must match bitwise float hashes even for NaN decoration thicknesses
/// or font weights; TextRun's ordinary PartialEq does not provide that guarantee.
#[derive(Clone)]
struct RunsKey(Arc<[TextRun]>);

impl PartialEq for RunsKey {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
            || (self.0.len() == other.0.len()
                && self.0.iter().zip(other.0.iter()).all(|(a, b)| {
                    a.len == b.len
                        && a.font.family == b.font.family
                        && a.font.features == b.font.features
                        && a.font.fallbacks == b.font.fallbacks
                        && a.font.weight.0.to_bits() == b.font.weight.0.to_bits()
                        && a.font.style == b.font.style
                        && a.font_size.map(f32::to_bits) == b.font_size.map(f32::to_bits)
                        && a.line_height.map(f32::to_bits) == b.line_height.map(f32::to_bits)
                        && a.color == b.color
                        && a.color_is_explicit == b.color_is_explicit
                        && a.background_color == b.background_color
                        && a.underline
                            .map(|s| (s.thickness.0.to_bits(), s.color, s.wavy))
                            == b.underline
                                .map(|s| (s.thickness.0.to_bits(), s.color, s.wavy))
                        && a.strikethrough.map(|s| (s.thickness.0.to_bits(), s.color))
                            == b.strikethrough.map(|s| (s.thickness.0.to_bits(), s.color))
                }))
    }
}
impl Eq for RunsKey {}

impl Hash for RunsKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.len().hash(state);
        for run in self.0.iter() {
            run.len.hash(state);
            run.font.hash(state);
            run.font_size.map(px).hash(state);
            run.line_height.map(px).hash(state);
            run.color.hash(state);
            run.color_is_explicit.hash(state);
            run.background_color.hash(state);
            run.underline.hash(state);
            run.strikethrough.hash(state);
        }
    }
}

/// The discriminant keeps uniform foreground overrides separate from rich runs.
#[derive(Clone, PartialEq, Eq, Hash)]
enum StyleKey {
    Uniform(Font),
    Rich(RunsKey),
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    text: SharedString,
    style: StyleKey,
    size: Pixels,
    height: Pixels,
    clamp: Option<usize>,
    revision: u64,
}
/// Widgets own the strong references. The pool holds neither obsolete paragraphs
/// nor an extra unwrapped glyph layout, and prunes dead variants each frame.
pub struct TextLayoutCache {
    system: Arc<TextSystem>,
    paragraphs: Mutex<FxHashMap<Key, Vec<Weak<Paragraph>>>>,
}
impl TextLayoutCache {
    pub fn new(system: Arc<TextSystem>) -> Self {
        Self {
            system,
            paragraphs: Default::default(),
        }
    }
    fn key(
        &self,
        text: SharedString,
        style: StyleKey,
        size: f32,
        height: f32,
        clamp: Option<usize>,
    ) -> Key {
        Key {
            text,
            style,
            size: px(size),
            height: px(height),
            clamp,
            revision: self.font_revision(),
        }
    }
    pub fn prepare(
        &self,
        text: SharedString,
        font: &Font,
        size: f32,
        height: f32,
        width: Option<f32>,
        clamp: Option<usize>,
    ) -> Result<Arc<Paragraph>> {
        Self::validate_dimensions(size, height, width)?;
        let key = self.key(
            text.clone(),
            StyleKey::Uniform(font.clone()),
            size,
            height,
            clamp,
        );
        self.prepare_key(key, width, || {
            self.system.shape_paragraph(
                text.clone(),
                &[TextRun {
                    len: text.len(),
                    font: font.clone(),
                    ..Default::default()
                }],
                size,
                height,
                width,
                clamp,
            )
        })
    }

    /// Share paragraphs with exactly matching runs, dimensions and font revision.
    /// Keep a clone of `runs` for `intern_runs` after width-only intrinsic probes;
    /// both calls retain the same allocation instead of copying the run array.
    pub fn prepare_runs(
        &self,
        text: SharedString,
        runs: Arc<[TextRun]>,
        size: f32,
        height: f32,
        width: Option<f32>,
        clamp: Option<usize>,
    ) -> Result<Arc<Paragraph>> {
        Self::validate_dimensions(size, height, width)?;
        Self::validate_runs(&runs)?;
        let key = self.key(
            text.clone(),
            StyleKey::Rich(RunsKey(runs.clone())),
            size,
            height,
            clamp,
        );
        self.prepare_key(key, width, || {
            self.system
                .shape_paragraph(text, &runs, size, height, width, clamp)
        })
    }

    fn validate_dimensions(size: f32, height: f32, width: Option<f32>) -> Result<()> {
        anyhow::ensure!(
            size.is_finite()
                && size > 0.0
                && height.is_finite()
                && height > 0.0
                && width.is_none_or(|w| w.is_finite() && w >= 0.0),
            "invalid text layout dimensions"
        );
        Ok(())
    }

    fn validate_runs(runs: &[TextRun]) -> Result<()> {
        anyhow::ensure!(
            runs.iter().all(|run| {
                run.font_size.is_none_or(|v| v.is_finite() && v > 0.0)
                    && run.line_height.is_none_or(|v| v.is_finite() && v > 0.0)
            }),
            "invalid text run dimensions"
        );
        Ok(())
    }

    /// Only a missing style shapes text. Other widths clone native layout data
    /// on demand, leaving every live shared variant unchanged.
    fn prepare_key(
        &self,
        key: Key,
        width: Option<f32>,
        shape: impl FnOnce() -> Result<Paragraph>,
    ) -> Result<Arc<Paragraph>> {
        let candidate = {
            let pool = self.paragraphs.lock();
            pool.get(&key).and_then(|variants| {
                let mut first = None;
                for p in variants.iter().filter_map(Weak::upgrade) {
                    if p.wrap_width() == width {
                        return Some(p);
                    }
                    first.get_or_insert(p);
                }
                first
            })
        };
        let mut paragraph = if let Some(p) = candidate {
            // An exact live hit is already interned. Skip a second lock, run
            // hash and variant scan on the common repeated-label path.
            if p.wrap_width() == width {
                return Ok(p);
            }
            p
        } else {
            Arc::new(shape()?)
        };
        if paragraph.wrap_width() != width {
            Arc::make_mut(&mut paragraph).reflow(width);
        }
        self.intern_key(key, &mut paragraph);
        Ok(paragraph)
    }
    fn intern_key(&self, key: Key, paragraph: &mut Arc<Paragraph>) {
        let mut pool = self.paragraphs.lock();
        let variants = pool.entry(key).or_default();
        variants.retain(|p| p.strong_count() > 0);
        if let Some(shared) = variants
            .iter()
            .filter_map(Weak::upgrade)
            .find(|p| p.wrap_width() == paragraph.wrap_width())
        {
            *paragraph = shared;
        } else {
            variants.push(Arc::downgrade(paragraph));
        }
    }
    /// Rejoin an existing identical variant after temporary intrinsic probes.
    /// This keeps repeated labels shared after copy-on-write width measurement.
    pub fn intern(
        &self,
        paragraph: &mut Arc<Paragraph>,
        font: &Font,
        size: f32,
        height: f32,
        clamp: Option<usize>,
    ) {
        let key = self.key(
            paragraph.shared_source(),
            StyleKey::Uniform(font.clone()),
            size,
            height,
            clamp,
        );
        self.intern_key(key, paragraph);
    }

    /// Rejoin a rich variant after width-only copy-on-write measurement.
    /// Pass the original runs, size, height and clamp used to shape the paragraph,
    /// and only rejoin while the font revision is unchanged. Invalid dimensions
    /// panic, as they do for `Paragraph::reflow`.
    pub fn intern_runs(
        &self,
        paragraph: &mut Arc<Paragraph>,
        runs: Arc<[TextRun]>,
        size: f32,
        height: f32,
        clamp: Option<usize>,
    ) {
        Self::validate_dimensions(size, height, paragraph.wrap_width())
            .expect("invalid text layout dimensions");
        Self::validate_runs(&runs).expect("invalid text run dimensions");
        let key = self.key(
            paragraph.shared_source(),
            StyleKey::Rich(RunsKey(runs)),
            size,
            height,
            clamp,
        );
        self.intern_key(key, paragraph);
    }
    pub fn finish_frame(&self) {
        self.paragraphs.lock().retain(|_, variants| {
            variants.retain(|p| p.strong_count() > 0);
            !variants.is_empty()
        });
    }
    pub fn font_revision(&self) -> u64 {
        self.system.backend.revision()
    }
    pub fn same_system(&self, system: &Arc<TextSystem>) -> bool {
        Arc::ptr_eq(&self.system, system)
    }
    pub fn system(&self) -> &Arc<TextSystem> {
        &self.system
    }
}
impl Deref for TextLayoutCache {
    type Target = TextSystem;
    fn deref(&self) -> &TextSystem {
        &self.system
    }
}
