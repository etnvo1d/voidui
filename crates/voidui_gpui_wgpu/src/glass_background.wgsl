// FP32 glass_background_base recovered from QuartzCore 26.5 AIR. Coordinate
// generation and pyramid construction are deliberately outside this function.
// CPU packing rounds all matrix coefficients to the native half representation.
struct GlassBackgroundParams {
    displacement: vec4<f32>,
    refraction: vec4<f32>, // inner amount/inverse height, outer amount/inverse height
    refraction_mix: vec4<f32>, // distance0, distance1, opacity, complex flag
    blur: vec4<f32>, // face radius, bleed radius, shadow radius, face opacity
    blur_weights: vec4<f32>,
    blur_distances: vec4<f32>,
    face0: vec4<f32>, face1: vec4<f32>, face2: vec4<f32>,
    bleed0: vec4<f32>, bleed1: vec4<f32>, bleed2: vec4<f32>,
    shadow0: vec4<f32>, shadow1: vec4<f32>, shadow2: vec4<f32>,
    bleed: vec4<f32>, // amount, inverse height, distance0, distance1
    bleed_mix: vec4<f32>, // opacity, darken slope, darken offset, enabled
    shadow: vec4<f32>, // amount, inverse height, opacity, vibrancy contribution
    shadow_geometry: vec4<f32>, // local offset x/y, inverse radius, distance offset
    holding: vec4<f32>, // opacity, white value, distance0, distance1
    limits: vec4<f32>, // clamp limit, preserve hue, EDR scale, shadow face opacity
}
fn glass_clean(color: vec3<f32>) -> vec3<f32> {
    return select(color, vec3<f32>(0.0), abs(color) < vec3<f32>(0.000001));
}
fn glass_matrix(color: vec3<f32>, r0: vec4<f32>, r1: vec4<f32>, r2: vec4<f32>) -> vec3<f32> {
    return vec3<f32>(dot(r0.xyz,color)+r0.w, dot(r1.xyz,color)+r1.w, dot(r2.xyz,color)+r2.w);
}
fn glass_ramp(distance: f32, start: f32, end: f32) -> f32 {
    // Preserve the measured M3 result of the native affine expression at
    // coincident stops: 1/delta is +inf; only positive distance and a negative
    // start produce +inf without cancellation. All tested NaNs saturate to zero.
    // Tests cover coincident negative, zero and positive stop pairs.
    if start == end { return select(0.0,1.0,start<0.0 && distance>0.0); }
    let delta=end-start;
    return saturate(fma(distance,1.0/delta,-start/delta));
}
fn glass_distance_blur(distance: f32, weights: vec4<f32>, stops: vec4<f32>) -> f32 {
    let t=vec3<f32>(glass_ramp(distance,stops.x,stops.y),
        glass_ramp(distance,stops.y,stops.z),glass_ramp(distance,stops.z,stops.w));
    let terms=weights.yzw*t;
    return weights.x-((terms.x+terms.y)+terms.z);
}
fn glass_background_sample(source: texture_2d<f32>, sampling: sampler, uv: vec2<f32>, radius: f32) -> vec4<f32> {
    let sample=textureSampleLevel(source,sampling,uv,glass_blur_lod(radius));
    return vec4<f32>(glass_clean(sample.rgb),sample.a);
}
fn glass_shadow_coverage(distance: f32, inverse_radius: f32) -> f32 {
    // Even the FP32 native shader uses half operations in its shadow polynomial.
    let v=glass_round_half(distance*inverse_radius);
    let a=glass_round_half(glass_round_half(v*0.25)+0.5);
    let x=glass_round_half(glass_round_half(saturate(a)*4.0)-2.0);
    let squared=glass_round_half(x*x);
    var p=glass_round_half(fma(0.0029544830322265625,squared,-0.034454345703125));
    p=glass_round_half(fma(p,squared,0.168212890625));
    p=glass_round_half(fma(p,squared,-0.560546875));
    return glass_round_half(fma(p,x,0.5));
}
fn glass_background(source: texture_2d<f32>, sampling: sampler, uv: vec2<f32>,
    field: vec4<f32>, coverage: f32, shadow_field: vec2<f32>, p: GlassBackgroundParams) -> vec4<f32> {
    let distance=field.x;
    let direction=vec2<f32>(dot(field.yz,p.displacement.xy),dot(field.yz,p.displacement.zw));
    var shadow=vec4<f32>(0.0);
    if coverage<1.0 {
        let shift=glass_refraction(distance+p.shadow_geometry.w,p.shadow.x,p.shadow.y);
        let weight=p.shadow.z*shadow_field.y*glass_shadow_coverage(shadow_field.x,p.shadow_geometry.z);
        if weight<0.000001 && coverage==0.0 { return vec4<f32>(0.0); }
        let offset=vec3<f32>(p.shadow0.w,p.shadow1.w,p.shadow2.w);
        var color=vec4<f32>(offset,p.limits.w);
        if p.shadow.w>0.000001 {
            let sample=glass_background_sample(source,sampling,uv+direction*shift,p.blur.z);
            let rgb=glass_clean(sample.rgb/max(sample.a,0.000001));
            let transformed=vec3<f32>(dot(rgb,p.shadow0.xyz),dot(rgb,p.shadow1.xyz),dot(rgb,p.shadow2.xyz));
            color=vec4<f32>(transformed*p.shadow.w+offset,mix(p.limits.w,1.0,p.shadow.w));
        }
        shadow=color*weight;
    }
    var face=vec4<f32>(0.0);
    if coverage>0.0 {
        var sample:vec4<f32>;
        if p.refraction_mix.w!=0.0 {
            let inner=glass_refraction(distance,p.refraction.x,p.refraction.y);
            let radius=glass_distance_blur(distance+inner,p.blur_weights,p.blur_distances)*p.blur.x;
            sample=glass_background_sample(source,sampling,uv+direction*inner,radius);
            if p.refraction_mix.z>0.0 {
                let outer=glass_refraction(distance,p.refraction.z,p.refraction.w);
                let outer_radius=glass_distance_blur(distance+outer,p.blur_weights,p.blur_distances)*p.blur.x;
                let refracted=glass_background_sample(source,sampling,uv+direction*outer,outer_radius);
                let weight=glass_ramp(distance,p.refraction_mix.x,p.refraction_mix.y)*p.refraction_mix.z;
                sample=mix(sample,refracted,weight);
            }
        } else {
            sample=glass_background_sample(source,sampling,uv,p.blur.x);
        }
        let rgb=sample.rgb/max(sample.a,0.000001);
        var face_rgb=rgb;
        if p.blur.w>0.0 { face_rgb=mix(rgb,glass_matrix(rgb,p.face0,p.face1,p.face2),p.blur.w); }
        if p.bleed_mix.w!=0.0 {
            let shift=glass_refraction(distance,p.bleed.x,p.bleed.y);
            let bleed_sample=glass_background_sample(source,sampling,uv+direction*shift,p.blur.y);
            let bleed_rgb=glass_matrix(glass_clean(bleed_sample.rgb/max(bleed_sample.a,0.000001)),p.bleed0,p.bleed1,p.bleed2);
            let ramp=glass_ramp(distance,p.bleed.z,p.bleed.w);
            let luma=saturate(dot(face_rgb,vec3<f32>(0.21250000596046448,0.715399980545044,0.07209999859333038)));
            let modulation=p.bleed_mix.y*luma+p.bleed_mix.z;
            let response=modulation*modulation*ramp;
            face_rgb=mix(face_rgb,bleed_rgb,response*response*p.bleed_mix.x);
        }
        // Native glass creates an opaque face from the unpremultiplied backdrop.
        face=vec4<f32>(face_rgb,1.0);
    }
    var result=mix(shadow,face,coverage);
    if p.holding.x>0.0 {
        var weight=0.0;
        if distance<p.holding.z { weight=1.0; }
        else if distance<p.holding.w { weight=1.0-(distance-p.holding.z)/(p.holding.w-p.holding.z); }
        let alpha=saturate(result.a);
        let held=vec4<f32>(p.holding.y*result.rgb*alpha/max(result.a,0.000001),alpha);
        result=mix(result,held,weight*p.holding.x);
    }
    if p.limits.x>0.0 {
        var rgb=result.rgb/max(result.a,0.000001);
        if p.limits.y>0.0 {
            let peak=max(max(rgb.x,rgb.y),rgb.z);
            if peak>p.limits.x { rgb*=p.limits.x/peak; }
        } else { rgb=clamp(rgb,vec3<f32>(-0.75),vec3<f32>(p.limits.x)); }
        result=vec4<f32>(rgb*result.a,result.a);
    }
    return vec4<f32>(result.rgb*p.limits.z,result.a);
}
