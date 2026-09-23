// Exact pixel copy: retained targets already contain composited, premultiplied ink.
@group(0) @binding(0) var source: texture_2d<f32>;
@vertex fn vertex_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let vertices = array<vec2<f32>,3>(vec2<f32>(-1.,-1.),vec2<f32>(3.,-1.),vec2<f32>(-1.,3.));
    return vec4<f32>(vertices[i],0.,1.);
}
@fragment fn fragment_main(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    return textureLoad(source,vec2<i32>(p.xy),0);
}
