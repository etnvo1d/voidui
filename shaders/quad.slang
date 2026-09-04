//------------------------------------------------------------------------------
// voidui — analytic rounded-rect primitive.
//
// One instance = one quad. Fill and stroke are separate commands at the API
// level; when the batcher proves they describe the same geometry it fuses them
// into a single instance so the pixel is shaded exactly once. Shading twice and
// letting the fixed-function blender combine the two coverages is what produced
// the conflation artifact in the old GL backend (a 1px border whose arcs went
// muddy red while its axis-aligned edges stayed pure black).
//------------------------------------------------------------------------------

#include "common.slang"

static const uint FLAG_FILL     = 1u << 0;
static const uint FLAG_STROKE   = 1u << 1;
static const uint FLAG_GRADIENT = 1u << 2;
static const uint FLAG_SHADOW   = 1u << 3;
static const uint FLAG_CONIC    = 1u << 4;
static const uint FLAG_STROKE_GRADIENT = 1u << 5;
static const uint FLAG_SHADOW_INSET    = 1u << 6;

static const float INV_TWO_PI = 0.15915494309189535;

// Which space a gradient's stops were written into, mirroring
// voidui::ColorInterpolationSpace. The stop table already holds the components
// converted into that space and premultiplied by alpha, so the fragment does
// one conversion for the mixed result rather than two for the endpoints -- and
// the sRGB case, which is every gradient that predates this, does none at all.
static const int SPACE_SRGB        = 0;
static const int SPACE_SRGB_LINEAR = 1;
static const int SPACE_DISPLAY_P3  = 2;
static const int SPACE_OKLAB       = 3;
static const int SPACE_OKLCH       = 4;

// Gradient stop table. Two texels of a row carry its eight positions, then one
// texel per colour. Keeping the stops here instead of in the vertex stream is
// what lets an instance stay lean: they were 64 bytes and ten attributes that
// every solid fill, stroke and shadow paid for and none of them used.
static const int   GRADIENT_POSITION_TEXELS = 2;
static const float GRADIENT_TABLE_WIDTH     = 10.0;

#ifdef VOIDUI_TARGET_SPIRV
[[vk::binding(0, 0)]]
Sampler2D g_gradients;
float4 sample_gradients(float2 uv) { return g_gradients.Sample(uv); }
#else
Texture2D    g_gradient_texture : register(t0);
SamplerState g_gradient_sampler : register(s0);
float4 sample_gradients(float2 uv)
{
    return g_gradient_texture.Sample(g_gradient_sampler, uv);
}
#endif

struct VSIn
{
    uint   vertex_id    : SV_VertexID;
    float2 center       : TEXCOORD0;
    float2 half_size    : TEXCOORD1;
    float4 radii        : TEXCOORD2;
    // Gradient line endpoints, box-local 0..1 -- or, on a shadow, the second
    // rounded rect: a = its centre relative to this one, b = its half extents,
    // with its radii in gradient_meta.
    float2 gradient_a   : TEXCOORD3;
    float2 gradient_b   : TEXCOORD4;
    float4 transform    : TEXCOORD5;   // a, b, c, d; translation is in center
    float4 fill_color   : TEXCOORD6;
    float4 stroke_color : TEXCOORD7;
    float2 stroke_blur  : TEXCOORD8;   // x = stroke width, y = shadow sigma
    float4 gradient_meta: TEXCOORD9;   // conic angle, stop count, opacity, space
    float  gradient_v   : TEXCOORD10;  // row of the stop table
    uint   flags        : TEXCOORD11;
};

struct VSOut
{
    float4 position     : SV_Position;
                    float2 world        : TEXCOORD0;
                    float2 shape        : TEXCOORD1;
    nointerpolation float2 half_size    : TEXCOORD2;
    nointerpolation float4 radii        : TEXCOORD3;
    nointerpolation float2 gradient_a   : TEXCOORD4;
    nointerpolation float2 gradient_b   : TEXCOORD5;
    nointerpolation float4 linear_map   : TEXCOORD6;   // a, b, c, d
    nointerpolation float4 fill_color   : TEXCOORD7;
    nointerpolation float4 stroke_color : TEXCOORD8;
    nointerpolation float2 stroke_blur  : TEXCOORD9;
    nointerpolation float4 gradient_meta: TEXCOORD10;
    nointerpolation float  gradient_v   : TEXCOORD11;
    nointerpolation uint   flags        : TEXCOORD12;
};

VSOut vs_main(VSIn input)
{
    VSOut o;

    // Triangle-strip corners from the vertex id: (0,0) (1,0) (0,1) (1,1).
    float2 corner = float2(input.vertex_id & 1u, (input.vertex_id >> 1u) & 1u);

    // Expand past the shape so the antialiasing falloff has somewhere to land.
    // The old backend emitted the rect exactly, which clipped the outer half of
    // every edge's coverage ramp and made the result depend on subpixel phase.
    //
    // The two shape axes are padded independently. One scalar taken from the
    // transform's determinant is the geometric mean of the two, so under
    // something like scale(4, 1) it over-pads one axis and under-pads the
    // other, and under-padding clips the ramp straight back off again.
    float2 axis = float2(length(float2(input.transform.x, input.transform.y)),
                         length(float2(input.transform.z, input.transform.w)));
    axis = max(axis, float2(1e-4, 1e-4));
    float2 pad = 2.0 * max(u_pixel, 1.0) / axis;

    // An outer shadow needs far more slack: its blur reaches about three sigma,
    // and sigma is already expressed in the shape's own units. An inner one
    // needs none -- its quad is the box it is clipped to, and the blur only
    // ever reaches inward.
    if ((input.flags & (FLAG_SHADOW | FLAG_SHADOW_INSET)) == FLAG_SHADOW)
        pad += 3.0 * input.stroke_blur.y;

    float2 shape = (corner * 2.0 - 1.0) * (input.half_size + pad);
    float2 pos = input.center + float2(
        input.transform.x * shape.x + input.transform.z * shape.y,
        input.transform.y * shape.x + input.transform.w * shape.y);

    o.position     = to_clip_space(pos);
    o.world        = pos;
    o.shape        = shape;
    o.half_size    = input.half_size;
    o.radii        = input.radii;
    o.gradient_a   = input.gradient_a;
    o.gradient_b   = input.gradient_b;
    o.linear_map   = input.transform;
    o.fill_color   = input.fill_color;
    o.stroke_color = input.stroke_color;
    o.stroke_blur  = input.stroke_blur;
    o.gradient_meta= input.gradient_meta;
    o.gradient_v   = input.gradient_v;
    o.flags        = input.flags;
    return o;
}

// Abramowitz & Stegun 7.1.26, good to ~5e-4 -- far tighter than a shadow needs.
float2 erf_approx(float2 x)
{
    float2 s = sign(x);
    float2 a = abs(x);
    float2 t = 1.0 + (0.278393 + (0.230389 + 0.078108 * (a * a)) * a) * a;
    t *= t;
    return s - s / (t * t);
}

/// How far the silhouette reaches sideways from the centre line at row `y`,
/// for a corner of radius `corner`. Away from the corners this is simply half
/// the width.
float edge_offset(float y, float corner, float2 half_size)
{
    float delta = min(half_size.y - corner - abs(y), 0.0);
    return half_size.x - corner + sqrt(max(corner * corner - delta * delta, 0.0));
}

/// Coverage of a Gaussian-blurred rounded rect along one scanline. Integrating
/// this over y gives the shadow without any blur passes or intermediate
/// textures -- a shadow costs exactly one quad.
///
/// Each side is measured against its own corner radius. Collapsing all four
/// radii to their maximum, which is what this used to do, put round corners on
/// the shadow of every shape whose corners were not all equal -- a card cut
/// only across the top cast a fully rounded shadow.
float blur_along_x(float x, float y, float sigma, float4 radii, float2 half_size)
{
    // Radii run top-left, top-right, bottom-right, bottom-left, and negative y
    // is the top half -- the convention select_radius() already uses.
    float left  = y < 0.0 ? radii.x : radii.w;
    float right = y < 0.0 ? radii.y : radii.z;

    float2 bounds = float2(-edge_offset(y, left, half_size),
                            edge_offset(y, right, half_size));
    float2 integral = 0.5 + 0.5 * erf_approx((x - bounds) * (0.70710678 / sigma));
    return integral.x - integral.y;
}

float shadow_coverage(float2 p, float2 half_size, float4 radii, float sigma,
                      float aa)
{
    if (sigma < 1e-3)
        return saturate(0.5 - rounded_rect_sdf(p, half_size, radii) / aa);

    // Six taps across the vertical extent resolve the integral; the horizontal
    // direction is already exact via the error function.
    float low = p.y - half_size.y;
    float high = p.y + half_size.y;
    float start = clamp(-3.0 * sigma, low, high);
    float end = clamp(3.0 * sigma, low, high);

    float step_size = (end - start) / 6.0;
    float y = start + step_size * 0.5;
    float value = 0.0;

    for (int i = 0; i < 6; i++) {
        value += blur_along_x(p.x, p.y - y, sigma, radii, half_size)
               * exp(-0.5 * (y * y) / (sigma * sigma)) * step_size;
        y += step_size;
    }

    return saturate(value / (sigma * 2.5066283));  // sqrt(2*pi)
}

float3 srgb_to_linear(float3 c)
{
    float3 low  = c / 12.92;
    float3 high = pow((max(c, 0.0) + 0.055) / 1.055, 2.4);
    return float3(c.x <= 0.04045 ? low.x : high.x,
                  c.y <= 0.04045 ? low.y : high.y,
                  c.z <= 0.04045 ? low.z : high.z);
}

float3 linear_to_srgb(float3 c)
{
    float3 low  = c * 12.92;
    float3 high = 1.055 * pow(max(c, 0.0), 1.0 / 2.4) - 0.055;
    return float3(c.x <= 0.0031308 ? low.x : high.x,
                  c.y <= 0.0031308 ? low.y : high.y,
                  c.z <= 0.0031308 ? low.z : high.z);
}

float3 oklab_to_linear_srgb(float3 lab)
{
    float l_ = lab.x + 0.3963377774 * lab.y + 0.2158037573 * lab.z;
    float m_ = lab.x - 0.1055613458 * lab.y - 0.0638541728 * lab.z;
    float s_ = lab.x - 0.0894841775 * lab.y - 1.2914855480 * lab.z;

    float3 lms = float3(l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);

    return float3(+4.0767416621 * lms.x - 3.3077115913 * lms.y + 0.2309699292 * lms.z,
                  -1.2684380046 * lms.x + 2.6097574011 * lms.y - 0.3413193965 * lms.z,
                  -0.0041960863 * lms.x - 0.7034186147 * lms.y + 1.7076147010 * lms.z);
}

float3 linear_p3_to_linear_srgb(float3 c)
{
    return float3(1.2249401762 * c.x - 0.2249401762 * c.y,
                 -0.0420569547 * c.x + 1.0420569547 * c.y,
                 -0.0196375546 * c.x - 0.0786360454 * c.y + 1.0982736000 * c.z);
}

float4 gradient_texel(float v, int column)
{
    return sample_gradients(
        float2((float(column) + 0.5) / GRADIENT_TABLE_WIDTH, v));
}

/// Shader model 5 forbids dynamically indexing a vector's components, so the
/// eight positions still come out of a branch ladder -- but over two fetched
/// texels rather than two interpolated ones, and the colours below need no
/// ladder at all because the column folds straight into the sample coordinate.
float stop_position(float4 low, float4 high, int index)
{
    if (index <= 0) return low.x;
    if (index == 1) return low.y;
    if (index == 2) return low.z;
    if (index == 3) return low.w;
    if (index == 4) return high.x;
    if (index == 5) return high.y;
    if (index == 6) return high.z;
    return high.w;
}

/// One stop, as stored: components in the gradient's own interpolation space,
/// premultiplied by alpha, with alpha itself in w. Two of these lerp directly
/// -- which is the whole point of keeping the table in that space.
float4 stop_value(float v, int index)
{
    return gradient_texel(v, GRADIENT_POSITION_TEXELS + index);
}

/// Turns a mixed stop value into the premultiplied sRGB the blender wants.
///
/// Opacity rides on the instance rather than on the row so that the same stops
/// shared by two elements at different opacities still share one row. The
/// branch is uniform across an instance, so it costs a scalar compare and the
/// default sRGB path stays exactly the multiply it always was.
float4 resolve_brush(float4 value, int space, float opacity)
{
    if (space == SPACE_SRGB)
        return value * opacity;

    float alpha = value.w;
    float scale = alpha > 1e-5 ? 1.0 / alpha : 0.0;
    float3 c = value.xyz * scale;

    float3 linear;
    if (space == SPACE_OKLCH) {
        // Hue is an angle and was never premultiplied, so it comes back raw.
        float hue = radians(value.z);
        linear = oklab_to_linear_srgb(float3(c.x, c.y * cos(hue), c.y * sin(hue)));
    } else if (space == SPACE_OKLAB) {
        linear = oklab_to_linear_srgb(c);
    } else if (space == SPACE_DISPLAY_P3) {
        linear = linear_p3_to_linear_srgb(srgb_to_linear(c));
    } else {
        linear = c;
    }

    // Mixing inside a wider space can leave the sRGB gamut between two stops
    // that are both inside it; clipping is the cheap answer and the excursion
    // is small, the endpoints having been mapped on the way in.
    float3 rgb = linear_to_srgb(saturate(linear));
    float a = alpha * opacity;
    return float4(rgb * a, a);
}

float4 linear_brush(VSOut i, float position)
{
    int count = clamp((int)i.gradient_meta.y, 1, 8);
    float opacity = i.gradient_meta.z;
    int space = (int)i.gradient_meta.w;
    if (count <= 1)
        return resolve_brush(stop_value(i.gradient_v, 0), space, opacity);

    float4 low = gradient_texel(i.gradient_v, 0);
    float4 high = gradient_texel(i.gradient_v, 1);

    // Positions are non-decreasing after CSS stop fixup, so each test that
    // passes implies the ones before it and the last one wins.
    int index = 0;
    if (count > 2 && position >= stop_position(low, high, 1)) index = 1;
    if (count > 3 && position >= stop_position(low, high, 2)) index = 2;
    if (count > 4 && position >= stop_position(low, high, 3)) index = 3;
    if (count > 5 && position >= stop_position(low, high, 4)) index = 4;
    if (count > 6 && position >= stop_position(low, high, 5)) index = 5;
    if (count > 7 && position >= stop_position(low, high, 6)) index = 6;

    int next = min(index + 1, count - 1);
    float from = stop_position(low, high, index);
    float to = stop_position(low, high, next);
    float span = to - from;
    float local = span > 1e-6 ? saturate((position - from) / span)
                              : (position < to ? 0.0 : 1.0);
    return resolve_brush(lerp(stop_value(i.gradient_v, index),
                              stop_value(i.gradient_v, next), local),
                         space, opacity);
}

/// Stops sit around the turn and the last one closes back onto the first, so
/// there is always one more arc than there are gaps between listed positions.
/// Handing the last stop the 360-degree mark instead, which is what this did,
/// leaves a hard seam at zero degrees on every gradient whose author did not
/// happen to repeat the opening colour at the end -- and the shape this
/// primitive exists for is a spinner ring.
float4 conic_brush(VSOut i, float2 p)
{
    int count = clamp((int)i.gradient_meta.y, 1, 8);
    float opacity = i.gradient_meta.z;
    int space = (int)i.gradient_meta.w;
    if (count <= 1)
        return resolve_brush(stop_value(i.gradient_v, 0), space, opacity);

    float4 low = gradient_texel(i.gradient_v, 0);
    float4 high = gradient_texel(i.gradient_v, 1);

    // Zero degrees points upward and positive angles turn clockwise, matching
    // the author-facing VSS angle convention.
    float turn = atan2(p.x, -p.y) - i.gradient_meta.x;
    float position = frac(turn * INV_TWO_PI);

    // -1 means the turn has not reached the first stop yet, which puts it on
    // the closing arc that wrapped past zero.
    int index = -1;
    if (position >= stop_position(low, high, 0)) index = 0;
    if (count > 1 && position >= stop_position(low, high, 1)) index = 1;
    if (count > 2 && position >= stop_position(low, high, 2)) index = 2;
    if (count > 3 && position >= stop_position(low, high, 3)) index = 3;
    if (count > 4 && position >= stop_position(low, high, 4)) index = 4;
    if (count > 5 && position >= stop_position(low, high, 5)) index = 5;
    if (count > 6 && position >= stop_position(low, high, 6)) index = 6;
    if (count > 7 && position >= stop_position(low, high, 7)) index = 7;

    int from_index = count - 1;
    int to_index = 0;
    float from = stop_position(low, high, count - 1);
    float to = stop_position(low, high, 0);
    if (index < 0) {
        // Before the first stop: the arc started a full turn back.
        from -= 1.0;
    } else if (index >= count - 1) {
        // After the last stop: the arc ends a full turn on.
        to += 1.0;
    } else {
        from_index = index;
        to_index = index + 1;
        from = stop_position(low, high, index);
        to = stop_position(low, high, index + 1);
    }

    float span = to - from;
    float local = span > 1e-6 ? saturate((position - from) / span) : 0.0;
    return resolve_brush(lerp(stop_value(i.gradient_v, from_index),
                              stop_value(i.gradient_v, to_index), local),
                         space, opacity);
}

float4 fs_main(VSOut i) : SV_Target
{
    // `shape` is interpolated in the rectangle's local coordinate space while
    // `world` follows its transformed screen position. The SDF therefore sees
    // the original rounded rectangle and clipping sees the final geometry.
    float2 p = i.shape;

    // One device pixel expressed in the shape's own units, measured along the
    // direction the coverage ramp actually runs. One scalar cannot express this
    // under a non-uniform scale: the same shape-space distance then covers a
    // different number of device pixels on each axis, and a stretched box came
    // out with one pair of edges soft and the other hard. Taking the direction
    // from the field's analytic gradient also sidesteps the derivative blow-up
    // that select_radius() causes along the quadrant boundaries when the four
    // corner radii differ.
    float2 normal = rounded_rect_gradient(p, i.half_size, i.radii);
    float2 screen = float2(i.linear_map.x * normal.x + i.linear_map.z * normal.y,
                           i.linear_map.y * normal.x + i.linear_map.w * normal.y);
    float aa = max(u_pixel / max(length(screen), 1e-4), 1e-5);

    // Clips are stated in world space and are unaffected by this transform.
    float clip = apply_clips(i.world, max(u_pixel, 1e-5));

    if ((i.flags & FLAG_SHADOW) != 0u) {
        // The instance's second rounded rect, in this one's coordinates. Its
        // own coverage ramp runs along its own normal, so it gets its own `aa`
        // -- reusing the outer shape's leaves one of the two edges soft and the
        // other hard wherever the transform is not a uniform scale.
        float2 other_p     = p - i.gradient_a;
        float2 other_half  = i.gradient_b;
        float4 other_radii = i.gradient_meta;

        float2 other_normal = rounded_rect_gradient(other_p, other_half, other_radii);
        float2 other_screen = float2(
            i.linear_map.x * other_normal.x + i.linear_map.z * other_normal.y,
            i.linear_map.y * other_normal.x + i.linear_map.w * other_normal.y);
        float other_aa = max(u_pixel / max(length(other_screen), 1e-4), 1e-5);
        float other_cover = saturate(0.5 - rounded_rect_sdf(other_p, other_half,
                                                            other_radii) / other_aa);

        float coverage;
        if ((i.flags & FLAG_SHADOW_INSET) != 0u) {
            // Inside the box, everywhere the blurred hole is not. The quad is
            // the box, so its own coverage is the clip css-backgrounds-3 asks
            // for, and the second shape is the hole.
            float inside = saturate(0.5 - rounded_rect_sdf(p, i.half_size, i.radii) / aa);
            float hole = shadow_coverage(other_p, other_half, other_radii,
                                         i.stroke_blur.y, other_aa);
            coverage = inside * (1.0 - hole);
        } else {
            // The blurred silhouette with the casting box taken out of it. An
            // outer shadow is never painted under the shape that casts it, so a
            // translucent background does not reveal it.
            float blurred = shadow_coverage(p, i.half_size, i.radii,
                                            i.stroke_blur.y, aa);
            coverage = blurred * (1.0 - other_cover);
        }
        return premultiply(i.fill_color) * coverage * clip;
    }

    float d = rounded_rect_sdf(p, i.half_size, i.radii);

    // Coverage of the shape's outer edge — one clean antialiased transition.
    float outer = saturate(0.5 - d / aa);

    float4 gradient_brush = float4(0.0, 0.0, 0.0, 0.0);
    if ((i.flags & FLAG_GRADIENT) != 0u) {
        if ((i.flags & FLAG_CONIC) != 0u) {
            gradient_brush = conic_brush(i, p);
        } else {
            // The endpoints are box-local 0..1, but the projection has to run
            // in the shape's own units. Normalising the sample point instead --
            // x by the width, y by the height -- skews every direction that is
            // not an axis, so a 45-degree gradient on a 3:1 box came out at
            // about 6 degrees and the corners CSS puts at 0% and 100% did not
            // land there. `p` is already shape-centred, so the endpoints come
            // out of 0..1 rather than the point going in.
            float2 size = i.half_size * 2.0;
            float2 from = (i.gradient_a - 0.5) * size;
            float2 axis = (i.gradient_b - i.gradient_a) * size;
            float denom = dot(axis, axis);
            float t = denom > 1e-6 ? saturate(dot(p - from, axis) / denom) : 0.0;
            gradient_brush = linear_brush(i, t);
        }
    }

    // A fused solid-fill/gradient-stroke instance carries both brushes. Two
    // gradients are intentionally not fused because one payload cannot encode
    // two unrelated axes or stop sets.
    float4 fill_brush = ((i.flags & FLAG_GRADIENT) != 0u &&
                         (i.flags & FLAG_STROKE_GRADIENT) == 0u)
        ? gradient_brush : premultiply(i.fill_color);
    float4 fill = (i.flags & FLAG_FILL) != 0u
        ? fill_brush : float4(0.0, 0.0, 0.0, 0.0);

    float4 color = fill;

    if ((i.flags & FLAG_STROKE) != 0u) {
        // Inner stroke occupies d in [-width, 0]. `inner` is the coverage of the
        // region past the stroke's inner edge, so one lerp yields both the
        // stroke/fill boundary and (via `outer`) the shape's outer boundary —
        // two independent transitions, never stacked on top of each other.
        float inner = saturate(0.5 - (d + i.stroke_blur.x) / aa);
        float4 stroke = (i.flags & FLAG_STROKE_GRADIENT) != 0u
            ? gradient_brush : premultiply(i.stroke_color);
        color = lerp(stroke, fill, inner);
    }

    return color * outer * clip;
}
