#pragma once

#include <cmath>

#include "voidui/core/geometry.h"
#include "voidui/core/transform.h"

namespace voidui {

/// Rounding policy for putting geometry on the device pixel grid.
///
/// This lives apart from `geometry.h` on purpose: those are pure value types
/// that every translation unit pulls in, and rounding is policy.

/// Round to nearest, halves upward.
///
/// Not `std::round`, which is ties-away-from-zero: that is neither a single
/// instruction nor translation-equivariant across the origin. This form
/// satisfies `round_half_up(x + n) == round_half_up(x) + n` for integral `n`,
/// which is the property that keeps a scrolled block rigid instead of merely
/// deterministic -- every line in it shifts on the same frame.
///
/// Nor is it plain `floor`, which is what the renderer used to do. Floor's
/// decision boundary lies exactly on the integers a snapped coordinate is
/// trying to land on, so one ulp of accumulated error moves a whole line by a
/// pixel. Rounding puts that boundary half a pixel away, where nothing lands.
inline float round_half_up(float v) { return std::floor(v + 0.5f); }

/// The nearest whole device pixel, expressed back in logical units.
///
/// A non-positive or NaN scale leaves the value alone.
inline float snap_to_pixel(float logical, float device_scale) {
  return device_scale > 0.0f ? round_half_up(logical * device_scale) / device_scale
                             : logical;
}

/// A whole device pixel coordinate expressed in logical units, biased so the
/// round trip cannot land below it.
///
/// The bias is not cosmetic. Geometry is snapped in device pixels but submitted
/// in logical units, and the vertex shader multiplies it back out against the
/// viewport. `device / scale` is inexact in float32 whenever the scale is not a
/// power of two, and for roughly a fifth of device rows at 1.25x that round
/// trip returns a value a hair *below* the integer it started from -- at which
/// point the rasteriser resolves the row one pixel early and the snapping was
/// for nothing. A thousandth of a pixel is orders of magnitude beneath anything
/// visible, and orders of magnitude above the ~1e-5 error it has to dominate.
inline float device_to_logical(float device, float device_scale) {
  constexpr float kBias = 1e-3f;
  return device_scale > 0.0f ? (device + kBias) / device_scale : device;
}

/// Snaps a rectangle's four *edges* to the device grid.
///
/// Never its origin and size independently: a 1px rule whose top edge rounds up
/// while its height rounds down disappears outright, and two abutting boxes
/// stop tiling. Snapping edges keeps adjacency exact, which is what a box model
/// needs more than it needs uniform hairlines.
///
/// An extent that was positive is held at one device pixel rather than being
/// allowed to collapse: a hairline thinner than a pixel should read as the
/// thinnest line the display can draw, not vanish.
inline Rect<float> snap_rect_to_pixel(Rect<float> rect, float device_scale) {
  if (!(device_scale > 0.0f))
    return rect;

  const float left = round_half_up(rect.origin.x * device_scale);
  const float top = round_half_up(rect.origin.y * device_scale);
  float right = round_half_up((rect.origin.x + rect.size.width) * device_scale);
  float bottom = round_half_up((rect.origin.y + rect.size.height) * device_scale);

  if (rect.size.width > 0.0f && right <= left)
    right = left + 1.0f;
  if (rect.size.height > 0.0f && bottom <= top)
    bottom = top + 1.0f;

  // Position carries the bias; the extents are differences of whole pixels, so
  // both edges end up biased identically and abutting boxes still tile exactly.
  const float pixel = 1.0f / device_scale;
  return Rect<float>(device_to_logical(left, device_scale),
                     device_to_logical(top, device_scale), (right - left) * pixel,
                     (bottom - top) * pixel);
}

/// Quantises a transform's translation to whole device pixels.
///
/// A translation applies to a whole subtree, and every item in that subtree
/// snaps its own final position to the grid independently. Rounding survives
/// that only for shifts of a whole device pixel -- the equivariance property
/// `round_half_up(x + n) == round_half_up(x) + n` above holds for integral `n`
/// and nothing else. A fractional shift therefore lands each item on its own
/// side of its own rounding boundary: `translateY(1px)` at 1.25x is 1.25 device
/// pixels, so a button's background moves one pixel and its label two, and
/// under a transition they cross on different frames -- the button visibly
/// comes apart mid-press.
///
/// Quantising the shift once, where it enters the tree, restores the property
/// for everything below it: the subtree moves by the same whole number of
/// pixels, on the same frame, and stays rigid.
///
/// Only a pure translation is quantised. A scale or a rotation puts the whole
/// subtree off the grid anyway -- nothing under it snaps -- so there is no
/// rounding to keep consistent and no reason to make its motion any coarser.
inline Transform snap_translation_to_pixel(Transform transform,
                                           float device_scale) {
  if (!(device_scale > 0.0f) || !transform.is_translation())
    return transform;

  transform.e = round_half_up(transform.e * device_scale) / device_scale;
  transform.f = round_half_up(transform.f * device_scale) / device_scale;
  return transform;
}

/// A coordinate placed on the device grid and displaced by a translation, with
/// the two rounded apart.
///
/// `round_half_up(y * s + t * s)` is the same number whenever the arithmetic is
/// exact, and a different one when it is not. Ties are where it differs, and
/// they are not rare: at 1.25x every even logical coordinate is one -- a label
/// at y=10 is device 12.5, and the row one pixel below it, 13.5, is another.
/// On a tie the answer is decided by the last bit of `y + t`, which for a
/// quantised `t` is a value like 0.8f that no float represents exactly. The
/// button's background rounds one way, its label the other, and the label sits
/// still while the button underneath it moves.
///
/// Rounded apart, the shift is an integer and has nowhere to round to.
inline float snap_with_shift(float logical, float shift, float device_scale) {
  if (!(device_scale > 0.0f))
    return logical + shift;
  return round_half_up(logical * device_scale) +
         round_half_up(shift * device_scale);
}

} // namespace voidui
