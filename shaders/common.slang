//------------------------------------------------------------------------------
// Shared by every voidui pipeline: the uniform block, the rounded-rect signed
// distance field, and clip evaluation.
//------------------------------------------------------------------------------

// Vulkan uses push constants so changing a clip does not allocate or update a
// descriptor. D3D11 and Metal bind the same block at buffer slot 1; slot 0 is
// kept for the instanced vertex stream on Metal.
#ifdef VOIDUI_TARGET_SPIRV
[[vk::push_constant]]
cbuffer SceneUniforms
#else
cbuffer SceneUniforms : register(b1)
#endif
{
    float2 u_viewport;      // logical size of the render target
    float  u_pixel;         // logical units per device pixel (1 / display_scale)
    float  u_clip_count;    // 0, 1 or 2 rounded clips are active

    float4 u_clip_rect[2];  // x, y, width, height
    float4 u_clip_radii[2]; // top-left, top-right, bottom-right, bottom-left
};

float4 to_clip_space(float2 logical)
{
    return float4(logical.x / u_viewport.x * 2.0 - 1.0,
                  1.0 - logical.y / u_viewport.y * 2.0,
                  0.0, 1.0);
}

float4 premultiply(float4 c)
{
    return float4(c.rgb * c.a, c.a);
}

float select_radius(float2 p, float4 radii)
{
    float top    = p.x < 0.0 ? radii.x : radii.y;
    float bottom = p.x < 0.0 ? radii.w : radii.z;
    return p.y < 0.0 ? top : bottom;
}

float rounded_rect_sdf(float2 p, float2 half_size, float4 radii)
{
    float r = select_radius(p, radii);
    float2 q = abs(p) - half_size + r;
    return min(max(q.x, q.y), 0.0) + length(max(q, 0.0)) - r;
}

/// Unit normal of the rounded-rect field at `p`, in the shape's own space.
///
/// The field is a true Euclidean distance, so this is exactly the direction a
/// coverage ramp runs along, and mapping it through the instance's transform is
/// what turns "one device pixel" into a width in shape space. Taken
/// analytically rather than from ddx/ddy of the distance, which blows up along
/// the quadrant boundaries that select_radius() introduces whenever the four
/// corner radii differ.
float2 rounded_rect_gradient(float2 p, float2 half_size, float4 radii)
{
    float r = select_radius(p, radii);
    float2 s = float2(p.x < 0.0 ? -1.0 : 1.0, p.y < 0.0 ? -1.0 : 1.0);
    float2 q = abs(p) - half_size + r;

    // Past the inner box the nearest feature is a corner arc, or an edge -- one
    // component of q falls to zero there and the normalise picks out the axis.
    if (max(q.x, q.y) > 0.0) {
        float2 m = max(q, 0.0);
        return (m / max(length(m), 1e-8)) * s;
    }

    // Inside, the distance is the larger of the two edge distances.
    return q.x > q.y ? float2(s.x, 0.0) : float2(0.0, s.y);
}

float clip_coverage(float2 p, float4 rect, float4 radii, float aa)
{
    float2 half_size = rect.zw * 0.5;
    float2 centre = rect.xy + half_size;
    return saturate(0.5 - rounded_rect_sdf(p - centre, half_size, radii) / aa);
}

/// Product of every rounded clip in force. The scissor half of the clip is
/// applied by the hardware and never reaches the shader.
float apply_clips(float2 p, float aa)
{
    float coverage = 1.0;
    if (u_clip_count > 0.5)
        coverage *= clip_coverage(p, u_clip_rect[0], u_clip_radii[0], aa);
    if (u_clip_count > 1.5)
        coverage *= clip_coverage(p, u_clip_rect[1], u_clip_radii[1], aa);
    return coverage;
}
