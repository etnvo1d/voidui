#pragma once

#include "voidui/core/cursor.h"
#include "voidui/core/selection.h"
#include "voidui/core/typography.h"

/// Umbrella header for the style system.
///
/// The pieces, bottom up:
///
///   value.h        type-erased values, plus the equality/hash/parse hooks a
///                  value type has to answer
///   easing.h       the CSS easing functions, by value and without state
///   property.h     the open property registry and the declaration macros
///   selector.h     compound selectors, combinators, the ancestor Bloom filter
///                  and the style tree node
///   declaration.h  a rule's property assignments, literal or theme token
///   stylesheet.h   rules, cascade origins and the candidate index
///   theme.h        the token layer, kept apart from the rule layer so that
///                  switching theme never re-parses anything
///   computed.h     the resolved style of a node, shared between equal results
///   resolver.h     matching, cascade, inheritance, incremental re-resolve
///   svg.h          the SVG presentation properties (fill, stroke, ...), which
///                  are ordinary CSS properties and go through the same cascade
///   parser.h       the .vss and .vtheme readers
///   hot_reload.h   file watching, compiled out of release builds
///
/// A component author needs two macros and nothing else from this list; see
/// VOIDUI_STYLE_SCOPE and VOIDUI_STYLE_PROPERTY in property.h.

#include "voidui/core/style/animation.h"
#include "voidui/core/style/computed.h"
#include "voidui/core/style/declaration.h"
#include "voidui/core/style/easing.h"
#include "voidui/core/style/hot_reload.h"
#include "voidui/core/style/parser.h"
#include "voidui/core/style/property.h"
#include "voidui/core/style/resolver.h"
#include "voidui/core/style/selector.h"
#include "voidui/core/style/stylesheet.h"
#include "voidui/core/style/svg.h"
#include "voidui/core/style/theme.h"
#include "voidui/core/style/value.h"

namespace voidui {

class FontStack;

/// The properties the framework itself defines. Everything else in the system
/// treats these exactly like a third-party component's properties -- they are
/// declared with the same macro and live in the same registry, which is the
/// check that the extension path is a real one and not a second-class one.
namespace styles {

VOIDUI_GLOBAL_STYLE_PROPERTY(Cursor, CursorShape, "cursor", Inherited, None,
                             CursorShape::Auto);

VOIDUI_GLOBAL_STYLE_PROPERTY(UserSelect, voidui::UserSelect, "user-select",
                             Inherited, Paint, voidui::UserSelect::Auto);

VOIDUI_GLOBAL_STYLE_PROPERTY(Animation, AnimationList, "animation",
                             NotInherited, Paint, AnimationList{});

// The `transition` shorthand is expanded by the parser into these five
// longhands, exactly as CSS defines it. Keeping them apart is what makes
//
//   .card    { transition: opacity 200ms ease; }
//   .card.on { transition-duration: 60ms; }
//
// override only the duration, and it is also what lets the shorter lists
// repeat to the length of transition-property.
//
// None of the five invalidates anything on its own: declaring motion changes
// no pixel until a value it governs actually moves.
VOIDUI_GLOBAL_STYLE_PROPERTY(TransitionProperty, TransitionPropertyList,
                             "transition-property", NotInherited, None,
                             TransitionPropertyList{});

VOIDUI_GLOBAL_STYLE_PROPERTY(TransitionDuration, StyleTimeList,
                             "transition-duration", NotInherited, None,
                             StyleTimeList{});

VOIDUI_GLOBAL_STYLE_PROPERTY(TransitionDelay, StyleTimeList, "transition-delay",
                             NotInherited, None, StyleTimeList{});

VOIDUI_GLOBAL_STYLE_PROPERTY(TransitionTimingFunction, EasingList,
                             "transition-timing-function", NotInherited, None,
                             EasingList{});

VOIDUI_GLOBAL_STYLE_PROPERTY(TransitionBehavior, TransitionBehaviorList,
                             "transition-behavior", NotInherited, None,
                             TransitionBehaviorList{});

VOIDUI_GLOBAL_STYLE_PROPERTY(Transform, VisualTransform, "transform",
                             NotInherited, Paint, VisualTransform{});

// A list, not a single shadow: CSS stacks them, and a control as ordinary as a
// focused input wants two at once -- a spread ring plus the resting drop
// shadow underneath it.
VOIDUI_GLOBAL_STYLE_PROPERTY(BoxShadow, ShadowList, "box-shadow", NotInherited,
                             Paint, ShadowList{});

VOIDUI_GLOBAL_STYLE_PROPERTY(Opacity, float, "opacity", NotInherited, Paint,
                             1.0f);

VOIDUI_GLOBAL_STYLE_PROPERTY(Width, Length, "width", NotInherited, Layout,
                             Length(Length::Auto{}));

VOIDUI_GLOBAL_STYLE_PROPERTY(Height, Length, "height", NotInherited, Layout,
                             Length(Length::Auto{}));

VOIDUI_GLOBAL_STYLE_PROPERTY(Foreground, Brush, "color", Inherited, Paint,
                             Color(0, 0, 0));

VOIDUI_GLOBAL_STYLE_PROPERTY(Background, Brush, "background", NotInherited,
                             Paint, Color::TRANSPARENT);

VOIDUI_GLOBAL_STYLE_PROPERTY(Padding, Spacing<float>, "padding", NotInherited,
                             Layout, Spacing<float>(0.0f));

// Margin is stored per edge so shorthand and longhand declarations cascade
// independently, exactly like their CSS counterparts. ComputedStyle folds
// these four cold properties into one hot Spacing value during finalization.
VOIDUI_GLOBAL_STYLE_PROPERTY(MarginTop, MarginValue, "margin-top", NotInherited,
                             Layout, MarginValue{});

VOIDUI_GLOBAL_STYLE_PROPERTY(MarginRight, MarginValue, "margin-right",
                             NotInherited, Layout, MarginValue{});

VOIDUI_GLOBAL_STYLE_PROPERTY(MarginBottom, MarginValue, "margin-bottom",
                             NotInherited, Layout, MarginValue{});

VOIDUI_GLOBAL_STYLE_PROPERTY(MarginLeft, MarginValue, "margin-left",
                             NotInherited, Layout, MarginValue{});

VOIDUI_GLOBAL_STYLE_PROPERTY(BorderColor, Brush, "border-color", NotInherited,
                             Paint, Color::TRANSPARENT);

VOIDUI_GLOBAL_STYLE_PROPERTY(BorderRadius, Radius, "border-radius",
                             NotInherited, Paint, Radius(0.0f));

VOIDUI_GLOBAL_STYLE_PROPERTY(BorderWidth, float, "border-width",
                             NotInherited, Layout, 0.0f);

VOIDUI_GLOBAL_STYLE_PROPERTY(FontSize, float, "font-size", Inherited, Layout,
                             14.0f);

// Zero uses the natural line box reported by the selected face. Positive
// values are absolute logical pixels, matching the rest of VSS's dimensions.
VOIDUI_GLOBAL_STYLE_PROPERTY(LineHeight, float, "line-height", Inherited,
                             Layout, 0.0f);

VOIDUI_GLOBAL_STYLE_PROPERTY(FontWeight, voidui::FontWeight, "font-weight",
                             Inherited, Layout, voidui::FontWeight::Normal);

// Empty means the platform's UI font, which is also what a list resolves to
// when none of its families can be found.
VOIDUI_GLOBAL_STYLE_PROPERTY(FontFamily, FontFamilyList, "font-family",
                             Inherited, Layout, FontFamilyList{});

VOIDUI_GLOBAL_STYLE_PROPERTY(SelectionColor, Color, "selection-color",
                             Inherited, Paint, Color(51, 144, 255, 128));

} // namespace styles

using Padding = Spacing<float>;

/// Recomputes the ancestor Bloom filters of a whole subtree.
///
/// Call after building or restructuring the tree, before resolving. Linear,
/// and the only thing that has to happen in tree order.
void style_rebuild_blooms(StyleNode &root);

} // namespace voidui
