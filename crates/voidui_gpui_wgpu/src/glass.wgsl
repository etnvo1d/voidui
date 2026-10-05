// Optical composition around the recovered SDF helpers. The source filtering
// and color policy remain separately testable; see the parity status document.
struct GlassParams {
    optics: vec4<f32>, // inward amount magnitude, height, saturation, highlight
    tint: vec4<f32>,
    coverage: vec4<f32>, // opacity, blur, continuous corners, ovalization
    bounds: vec4<f32>,
    group: vec4<f32>, // shape count, smoothness, padding, padding
    key: vec4<f32>,
    fill: vec4<f32>,
    lighting: vec4<f32>,
    face0: vec4<f32>,
    face1: vec4<f32>,
    face2: vec4<f32>,
    background: GlassBackgroundParams,
}
@group(2) @binding(2) var<uniform> glass: GlassParams;
@group(2) @binding(3) var glass_sharp: texture_2d<f32>;

@vertex fn vs_glass(@builtin(vertex_index) vertex: u32, @builtin(instance_index) instance: u32) -> QuadVarying {
    let uv = vec2<f32>(f32(vertex & 1u), 0.5 * f32(vertex & 2u));
    let quad = load_quad(instance);
    var out: QuadVarying;
    var bounds = Bounds(glass.bounds.xy, glass.bounds.zw);
    if glass.group.z == 0.0 {
        bounds = quad.bounds;
        if glass.group.w != 0.0 && glass.background.shadow.z > 0.0 {
            let geometry=glass.background.shadow_geometry;
            let padding=2.0/geometry.z+max(abs(geometry.x),abs(geometry.y))+abs(geometry.w);
            bounds=Bounds(bounds.origin-vec2<f32>(padding),bounds.size+vec2<f32>(2.0*padding));
        }
    }
    out.local_position = bounds.origin + uv * bounds.size;
    out.position = spatial_position(out.local_position, quad.spatial_id);
    out.spatial_id = quad.spatial_id;
    out.quad_id = instance;
    out.clip_distances = distance_from_clip_rect(uv, bounds, quad.content_mask);
    return out;
}
fn glass_field(position: vec2<f32>, first: u32) -> vec4<f32> {
    var field = vec4<f32>(10000.0, 0.0, 0.0, 0.0);
    let count = select(1u, u32(glass.group.x), glass.group.z != 0.0);
    for (var i = 0u; i < count; i += 1u) {
        let quad = load_quad(first + i);
        let half_size = quad.bounds.size * 0.5;
        let point = position - quad.bounds.origin - half_size;
        let radius = min(pick_corner_radius(point, quad.corner_radii), min(half_size.x, half_size.y));
        let shape = glass_shape(point, half_size, radius, glass.coverage.w, glass.coverage.z != 0.0);
        if i == 0u { field = shape; }
        else { field = glass_union(shape, field, glass.group.y); }
    }
    return field;
}
@fragment fn fs_glass(input: QuadVarying) -> @location(0) vec4<f32> {
    let field = glass_field(input.local_position, input.quad_id);
    let aa = max(fwidth(field.x), 0.0001);
    let magnitude = length(field.yz);
    let depth = -field.x / max(magnitude, 0.000001);
    let highlight_aa = fwidth(depth);
    let clip = spatial_coverage(input.position.xy, input.spatial_id);
    if (any(input.clip_distances < vec4<f32>(0.0)) || clip == 0.0) { discard; }
    let shape_coverage=saturate(0.5-field.x/aa)*field.w;
    let coverage = shape_coverage * clip * glass.coverage.x;
    let light=glass_key_fill(field,highlight_aa,glass.key,glass.fill,glass.lighting,
        vec4<f32>(glass.optics.w),vec4<f32>(glass.optics.w));
    if glass.group.w != 0.0 {
        var shadow_field=vec2<f32>(0.0);
        if shape_coverage<1.0 && glass.background.shadow.z>0.0 {
            let shadow=glass_field(input.local_position+glass.background.shadow_geometry.xy,input.quad_id);
            shadow_field=shadow.xw;
        }
        var layer=glass_background(t_sprite,s_sprite,input.position.xy/globals.viewport_size,field,
            shape_coverage,shadow_field,glass.background);
        // Native glass is a premultiplied optical layer. Blend it over the
        // captured scene once; coverage already accounts for the SDF edge.
        if glass.optics.z!=1.0 {
            layer=vec4<f32>(mix(vec3<f32>(dot(layer.rgb,GRAYSCALE_FACTORS)),layer.rgb,glass.optics.z),layer.a);
        }
        layer=vec4<f32>(layer.rgb*(1.0-glass.tint.a)+glass.tint.rgb*glass.tint.a*layer.a,layer.a);
        layer=layer*(1.0-saturate(light.a))+light;
        // Source-over is performed by the optical pipeline. Returning a captured
        // replacement pixel here would erase earlier sibling lenses wherever
        // overlapping shadow bounds contain transparent padding.
        return layer*(clip*glass.coverage.x);
    }
    if coverage == 0.0 { discard; }

    let original=textureLoad(glass_sharp,vec2<i32>(input.position.xy),0);
    // Preserve the mixed gradient magnitude: normalizing a merged field before
    // refraction changes the liquid neck. Native highlights normalize separately.
    let displacement = glass_refraction(field.x, -glass.optics.x, 1.0 / glass.optics.y);
    let offset = field.yz * displacement;
    let screen_offset = spatial_point(input.local_position + offset, input.spatial_id)
        - spatial_point(input.local_position, input.spatial_id);
    let uv = clamp((input.position.xy + screen_offset) / globals.viewport_size,
        0.5 / globals.viewport_size, vec2<f32>(1.0) - 0.5 / globals.viewport_size);
    var backdrop = textureSampleLevel(glass_sharp, s_sprite, uv, 0.0);
    if glass.coverage.y > 0.0 { backdrop = textureSampleLevel(t_sprite, s_sprite, uv, glass_blur_lod(glass.coverage.y)); }

    var color = backdrop.rgb / max(backdrop.a, 0.000001);
    // Coefficients are rounded to native half precision once during CPU packing.
    color=vec3<f32>(dot(glass.face0.xyz,color)+glass.face0.w,
        dot(glass.face1.xyz,color)+glass.face1.w,dot(glass.face2.xyz,color)+glass.face2.w);
    color = max(mix(vec3<f32>(dot(color, GRAYSCALE_FACTORS)), color, glass.optics.z), vec3<f32>(0.0));
    var tint = glass.tint.rgb;
    if globals.srgb_framebuffer != 0u { tint = srgb_to_linear(tint); }
    let alpha = backdrop.a + glass.tint.a * (1.0 - backdrop.a);
    var material = color * backdrop.a * (1.0 - glass.tint.a) + tint * glass.tint.a;
    material = material * (1.0 - saturate(light.a)) + light.rgb * alpha;
    return mix(original, vec4<f32>(material, alpha), coverage);
}
