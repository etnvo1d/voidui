// Recovered variable-blur downsample on a reduced backdrop. All levels share a padded region;
// clamping keeps unwritten scratch texels out of samples near viewport edges.
struct FilterParams {
    viewport: vec2<f32>,
    sigma: f32,
    padding: f32,
    region: vec4<f32>,
}
@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var linear_sampler: sampler;
@group(0) @binding(2) var<uniform> params: FilterParams;
@vertex fn vertex_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}
@fragment fn downsample(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = p.xy / params.viewport;
    let offset = 0.25 / params.viewport;
    // Four bilinear samples cover the source footprint at quarter resolution.
    return 0.25 * (textureSampleLevel(source, linear_sampler, uv + offset, 0.0)
        + textureSampleLevel(source, linear_sampler, uv - offset, 0.0)
        + textureSampleLevel(source, linear_sampler, uv + offset * vec2<f32>(1.0, -1.0), 0.0)
        + textureSampleLevel(source, linear_sampler, uv + offset * vec2<f32>(-1.0, 1.0), 0.0));
}
fn half4(v: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(glass_round_half(v.x),glass_round_half(v.y),glass_round_half(v.z),glass_round_half(v.w));
}
fn source_sample(uv: vec2<f32>, offset: vec2<f32>) -> vec4<f32> {
    let dimensions=vec2<f32>(textureDimensions(source));
    let position=clamp((uv+offset)*dimensions,params.region.xy+0.5,params.region.zw-0.5);
    return half4(textureSampleLevel(source,linear_sampler,position/dimensions,0.0));
}
@fragment fn pyramid(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let uv=p.xy/params.viewport;
    let delta=1.0/params.viewport;
    // Recovered 13-tap variable-blur kernel. Preserve half-rounded additions,
    // multiplication constants and accumulation order from the general AIR path.
    let north=source_sample(uv,vec2<f32>(0.0,-delta.y));
    let west=source_sample(uv,vec2<f32>(-delta.x,0.0));
    let east=source_sample(uv,vec2<f32>(delta.x,0.0));
    let south=source_sample(uv,vec2<f32>(0.0,delta.y));
    let near_sum=half4(half4(half4(west+north)+east)+south);
    let nw=source_sample(uv,-delta);
    let ne=source_sample(uv,delta*vec2<f32>(1.0,-1.0));
    let sw=source_sample(uv,delta*vec2<f32>(-1.0,1.0));
    let se=source_sample(uv,delta);
    let corners=half4(half4(half4(nw+ne)+sw)+se);
    let far_n=source_sample(uv,vec2<f32>(0.0,-2.0*delta.y));
    let far_w=source_sample(uv,vec2<f32>(-2.0*delta.x,0.0));
    let far_e=source_sample(uv,vec2<f32>(2.0*delta.x,0.0));
    let far_s=source_sample(uv,vec2<f32>(0.0,2.0*delta.y));
    let far_sum=half4(half4(half4(far_w+far_n)+far_e)+far_s);
    let center=half4(source_sample(uv,vec2<f32>(0.0))*0.10546875);
    let weighted_corners=half4(corners*0.07708740234375);
    let weighted_near=half4(near_sum*0.0902099609375);
    let weighted_far=half4(far_sum*0.05633544921875);
    let result=half4(half4(half4(weighted_corners+center)+weighted_near)+weighted_far);
    return vec4<f32>(select(result.rgb,vec3<f32>(0.0),abs(result.rgb)<vec3<f32>(0.0001)),result.a);
}
