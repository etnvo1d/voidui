// Reconstructed from macOS 26.5 QuartzCore AIR, not visually fitted curves.
// Provenance and differential-test scope: docs/liquid-glass-research.md.
// Fields carry (signed distance, gradient.x, gradient.y, validity).

fn glass_round_half(x: f32) -> f32 {
    // The native FP32 supercircle explicitly rounds this intermediate to half.
    // Integer rounding keeps this path available on WebGL and GPUs without f16.
    let bits = bitcast<u32>(x);
    let sign_bit = bits & 0x80000000u;
    let magnitude = bits & 0x7fffffffu;
    if magnitude >= 0x7f800000u { return x; }
    if magnitude < 0x38800000u {
        let scaled = abs(x) * 16777216.0;
        let low = floor(scaled);
        let fraction = scaled - low;
        let up = fraction > 0.5 || (fraction == 0.5 && (u32(low) & 1u) != 0u);
        let rounded = (low + select(0.0, 1.0, up)) / 16777216.0;
        return bitcast<f32>(bitcast<u32>(rounded) | sign_bit);
    }
    let rounded = (magnitude + 4095u + ((magnitude >> 13u) & 1u)) & 0xffffe000u;
    if rounded >= 0x47800000u { return bitcast<f32>(sign_bit | 0x7f800000u); }
    return bitcast<f32>(rounded | sign_bit);
}
fn glass_normalize(v: vec2<f32>, fallback: vec2<f32>) -> vec2<f32> {
    let magnitude = dot(v, v);
    if magnitude == 0.0 { return fallback; }
    return v * inverseSqrt(magnitude);
}
fn glass_supercircle(p: vec2<f32>, half_size: vec2<f32>, radius: f32, blend: vec2<f32>) -> vec3<f32> {
    // Constants are exact float32 values recovered from supercircle_sdf AIR.
    // They approximate the native continuous-corner contour, not a circular arc.
    let r = abs(radius);
    let expanded = r * 1.5286649465560913;
    let effective = mix(expanded, r, max(blend.x, blend.y));
    let q = p - half_size + effective;
    let v = abs(max((p - half_size + expanded) / expanded, vec2<f32>(0.0)));
    let magnitude = length(v);
    let high = max(v.x, v.y);
    var ratio = 0.0;
    if high > 0.0 { ratio = saturate(min(v.x, v.y) / high); }
    let polynomial = ((((3.1560099124908447 - ratio * 0.9260540008544922) * ratio
        - 3.6412200927734375) * ratio + 1.268030047416687) * ratio + 0.2685309946537018);
    let curved = magnitude + 1.0 - 1.0 / (1.0 - ratio * ratio * saturate(magnitude) * polynomial);
    let circle = length(max(v * 1.5286649465560913 - 0.5286650061607361, vec2<f32>(0.0)))
        * 0.6541655659675598 + 0.3458344340324402;
    let direction = select(-1.0, 1.0, v.y > v.x);
    let weight = saturate(0.5 - direction + direction * ratio);
    let contour = mix(mix(curved, circle, blend.x), mix(curved, circle, blend.y), weight);
    let gradient = glass_normalize(max(q, vec2<f32>(0.0)),
        select(vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), q.x > q.y));
    let distance = min(max(q.x, q.y), 0.0) + expanded * glass_round_half(contour - 1.0);
    return vec3<f32>(distance, gradient);
}
fn glass_shape(p: vec2<f32>, half_size: vec2<f32>, radius: f32, ovalization: f32, continuous: bool) -> vec4<f32> {
    let signs = select(vec2<f32>(-1.0), vec2<f32>(1.0), p >= vec2<f32>(0.0));
    var shape: vec3<f32>;
    if radius > 0.0 {
        // SDFNode host packing uses 2.8915570351735034*(1-halfSize/(1.5286649465560913*r)).
        var blend = vec2<f32>(1.0);
        if continuous {
            blend = saturate(2.8915570351735034 * (vec2<f32>(1.0) - half_size / (radius * 1.5286649465560913)));
        }
        shape = glass_supercircle(abs(p), half_size, radius, blend);
    } else {
        let q = abs(p) - half_size;
        shape = vec3<f32>(max(q.x, q.y), select(vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), q.x > q.y));
    }
    let normal = shape.yz * signs;
    let oval = glass_normalize(vec2<f32>(p.x, half_size.x * p.y / half_size.y), normal);
    let gradient = glass_normalize(mix(normal, oval, ovalization), normal);
    return vec4<f32>(shape.x, gradient, 1.0);
}
fn glass_union(a: vec4<f32>, input_b: vec4<f32>, smoothing: f32) -> vec4<f32> {
    var b = input_b;
    if b.w == 0.0 { b = vec4<f32>(10000.0, 0.0, 0.0, 0.0); }
    let k = smoothing * saturate(0.5 - 0.5 * dot(a.yz, b.yz));
    // The native fast-math body divides by k. Explicit hard union defines the
    // zero-width/tie limit portably instead of relying on GPU NaN selection.
    if k <= 0.0 { return select(b, a, a.x <= b.x); }
    let h = saturate(0.5 + 0.5 * (b.x - a.x) / k);
    return vec4<f32>(mix(b.x, a.x, h) - k * h * (1.0 - h), mix(b.yz, a.yz, h), 1.0);
}
fn glass_refraction(distance: f32, amount: f32, inverse_height: f32) -> f32 {
    let t = saturate(-distance * inverse_height);
    return amount * (1.0 - saturate(sqrt(t * (2.0 - t))));
}
fn glass_blur_lod(radius: f32) -> f32 {
    return max(0.0, log2(select(radius, 1.0 + 0.5 * radius, radius < 2.0)));
}
fn glass_highlight_lobe(depth: f32, gradient: vec2<f32>, derivative: f32,
    height: f32, threshold: f32, direction: vec2<f32>, curvature: f32, contrast: f32) -> f32 {
    let t = saturate(depth / height);
    let face = mix(select(0.0, 1.0, t < 1.0), 1.0 - t, curvature);
    let aa = max(derivative, 0.0001);
    let edge = saturate(depth / aa + 0.5) * face * saturate((height - depth) / aa + 0.5);
    let angular = saturate((dot(direction, gradient) - threshold) / max(1.0 - threshold, 0.000001));
    let intensity = select(edge * angular, 0.0, depth < -5.0);
    return intensity / max(1.0 + (1.0 - intensity) * contrast, 0.000001);
}
// Packing follows SDFNode::apply: directions=(sin(angle),-cos(angle)),
// spread thresholds=cos(spread). Colors are supplied separately by the caller.
fn glass_key_fill(field: vec4<f32>, aa: f32, key: vec4<f32>, fill: vec4<f32>,
    lighting: vec4<f32>, key_color: vec4<f32>, fill_color: vec4<f32>) -> vec4<f32> {
    let magnitude = length(field.yz);
    if magnitude == 0.0 { return vec4<f32>(0.0); }
    let normal = field.yz / magnitude;
    let depth = -(field.x + lighting.w) / magnitude;
    let k = glass_highlight_lobe(depth, normal, aa, key.x, key.y, vec2<f32>(key.w, fill.x), lighting.z, key.z);
    let f = glass_highlight_lobe(depth, normal, aa, fill.y, fill.z, lighting.xy, lighting.z, fill.w);
    return key_color * k + fill_color * f;
}
