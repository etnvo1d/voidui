//------------------------------------------------------------------------------
// voidui — textured quad.
//
// One pipeline serves three things that all reduce to "sample an atlas and tint
// it": CPU-rasterised path coverage masks, glyph coverage from FreeType, and
// images. Keeping them together means the whole renderer is two pipelines
// rather than four, and a run of glyphs batches with the icons beside it.
//------------------------------------------------------------------------------

#include "common.slang"

static const uint FLAG_MASK = 1u << 0;  // sample .r as coverage and tint it

// Vulkan exposes one combined descriptor at set 0. D3D11 and Metal use their
// native texture and sampler slots directly.
#ifdef VOIDUI_TARGET_SPIRV
[[vk::binding(0, 0)]]
Sampler2D g_atlas;
float4 sample_atlas(float2 uv) { return g_atlas.Sample(uv); }
#else
Texture2D    g_atlas_texture : register(t0);
SamplerState g_atlas_sampler : register(s0);
float4 sample_atlas(float2 uv) { return g_atlas_texture.Sample(g_atlas_sampler, uv); }
#endif

// The quad is stated the same way the analytic pipeline states one: a centre
// that is already transformed, half extents in the source's own space, and the
// linear part of the transform. An axis-aligned rect used to be enough, which
// is exactly why an image or a run of text inside a rotated subtree came out
// upright and in the wrong place -- the transform had nowhere to go.
struct VSIn
{
    uint   vertex_id : SV_VertexID;
    float2 center    : TEXCOORD0;  // transformed centre, logical units
    float2 half_size : TEXCOORD1;  // half extents before the transform
    float4 transform : TEXCOORD2;  // a, b, c, d; translation is in center
    float4 uv        : TEXCOORD3;  // u0, v0, u1, v1
    float4 color     : TEXCOORD4;  // tint
    uint   flags     : TEXCOORD5;
};

struct VSOut
{
    float4 position : SV_Position;
                    float2 world : TEXCOORD0;
                    float2 uv    : TEXCOORD1;
    nointerpolation float4 color : TEXCOORD2;
    nointerpolation uint   flags : TEXCOORD3;
};

VSOut vs_main(VSIn input)
{
    VSOut o;

    float2 corner = float2(input.vertex_id & 1u, (input.vertex_id >> 1u) & 1u);
    float2 local = (corner * 2.0 - 1.0) * input.half_size;
    float2 pos = input.center + float2(
        input.transform.x * local.x + input.transform.z * local.y,
        input.transform.y * local.x + input.transform.w * local.y);

    o.position = to_clip_space(pos);
    o.world    = pos;
    o.uv       = lerp(input.uv.xy, input.uv.zw, corner);
    o.color    = input.color;
    o.flags    = input.flags;
    return o;
}

float4 fs_main(VSOut i) : SV_Target
{
    float aa = max(u_pixel, 1e-5);
    float clip = apply_clips(i.world, aa);

    float4 texel = sample_atlas(i.uv);

    // A mask carries coverage in one channel; an image carries premultiplied
    // colour already.
    float4 color = ((i.flags & FLAG_MASK) != 0u)
                 ? premultiply(i.color) * texel.r
                 : texel * i.color.a;

    return color * clip;
}
