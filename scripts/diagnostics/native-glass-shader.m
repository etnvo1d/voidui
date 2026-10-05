// Execute Apple's installed fragment shader against controlled textures.
// This is a macOS-only reference harness, never a VoidUI rendering dependency.
// Build: xcrun clang -fobjc-arc -framework Foundation -framework Metal \
//   scripts/diagnostics/native-glass-shader.m -o target/native-glass-shader
// Run: target/native-glass-shader library function uniforms.bin output.rgba32f
#import <Foundation/Foundation.h>
#import <Metal/Metal.h>

int main(int argc, char **argv) { @autoreleasepool {
    if (argc != 5 && argc != 6) { fprintf(stderr,"usage: native-glass-shader library function uniforms.bin output.rgba32f\n"); return 2; }
    NSError *error=nil;
    id<MTLDevice> device=MTLCreateSystemDefaultDevice();
    id<MTLLibrary> apple=[device newLibraryWithURL:[NSURL fileURLWithPath:@(argv[1])] error:&error];
    if (!apple) { NSLog(@"Apple library: %@",error); return 1; }
    id<MTLFunction> fragment=[apple newFunctionWithName:@(argv[2])];
    if (!fragment) { fprintf(stderr,"missing Apple function\n"); return 1; }
    NSString *vertexSource=@"#include <metal_stdlib>\nusing namespace metal;\n"
        "struct V { float4 position [[position]]; float2 texcoord0; float2 texcoord1; };\n"
        "vertex V reference_vertex(uint i [[vertex_id]]) {\n"
        "float2 uv=float2(float(i&1),float(i>>1));\n"
        "return V {float4(uv.x*2-1,1-uv.y*2,0,1),uv,uv}; }\n";
    id<MTLLibrary> helper=[device newLibraryWithSource:vertexSource options:nil error:&error];
    if (!helper) { NSLog(@"Vertex library: %@",error); return 1; }
    MTLRenderPipelineDescriptor *desc=[MTLRenderPipelineDescriptor new];
    desc.vertexFunction=[helper newFunctionWithName:@"reference_vertex"];
    desc.fragmentFunction=fragment;
    desc.colorAttachments[0].pixelFormat=MTLPixelFormatRGBA32Float;
    MTLRenderPipelineReflection *reflection=nil;
    id<MTLRenderPipelineState> pipeline=[device newRenderPipelineStateWithDescriptor:desc
        options:MTLPipelineOptionBindingInfo | MTLPipelineOptionBufferTypeInfo reflection:&reflection error:&error];
    if (!pipeline) { NSLog(@"Pipeline: %@",error); return 1; }
    for (id<MTLBinding> binding in reflection.fragmentBindings)
        printf("BIND %s index=%lu type=%ld\n",binding.name.UTF8String,(unsigned long)binding.index,(long)binding.type);
    NSDictionary *fixture=argc==6 ? [NSJSONSerialization JSONObjectWithData:[NSData dataWithContentsOfFile:[@(argv[5]) stringByAppendingPathComponent:@"input.json"]] options:0 error:&error] : nil;
    NSUInteger width=fixture ? [fixture[@"width"] unsignedIntegerValue] : 256;
    NSUInteger height=fixture ? [fixture[@"height"] unsignedIntegerValue] : 64;
    NSUInteger mipCount=fixture ? [fixture[@"mip_count"] unsignedIntegerValue] : 1;
    MTLTextureDescriptor *td=[MTLTextureDescriptor texture2DDescriptorWithPixelFormat:MTLPixelFormatRGBA32Float width:width height:height mipmapped:mipCount>1];
    td.mipmapLevelCount=mipCount;
    td.storageMode=MTLStorageModeShared;
    td.usage=MTLTextureUsageShaderRead | MTLTextureUsageRenderTarget;
    id<MTLTexture> source=[device newTextureWithDescriptor:td];
    td.mipmapLevelCount=1;
    id<MTLTexture> sdf=[device newTextureWithDescriptor:td];
    id<MTLTexture> target=[device newTextureWithDescriptor:td];
    float *pixels=calloc(width*height*4,sizeof(float));
    for(NSUInteger y=0;y<height;y++) for(NSUInteger x=0;x<width;x++) {
        NSUInteger i=(y*width+x)*4;
        pixels[i]=(x+0.5f)/width; pixels[i+1]=(y+0.5f)/height; pixels[i+2]=0.25f; pixels[i+3]=1.f;
    }
    [source replaceRegion:MTLRegionMake2D(0,0,width,height) mipmapLevel:0 withBytes:pixels bytesPerRow:width*16];
    for(NSUInteger y=0;y<height;y++) for(NSUInteger x=0;x<width;x++) {
        NSUInteger i=(y*width+x)*4;
        pixels[i]=-(y+0.5f)/height*40.f; pixels[i+1]=1.f; pixels[i+2]=0.f; pixels[i+3]=1.f;
    }
    [sdf replaceRegion:MTLRegionMake2D(0,0,width,height) mipmapLevel:0 withBytes:pixels bytesPerRow:width*16];
    if(fixture) {
        for(NSUInteger level=0;level<mipCount;level++) {
            NSUInteger w=MAX(1,width>>level),h=MAX(1,height>>level);
            NSData *data=[NSData dataWithContentsOfFile:[@(argv[5]) stringByAppendingPathComponent:[NSString stringWithFormat:@"source-%lu.bin",(unsigned long)level]]];
            if(data.length!=w*h*16){fprintf(stderr,"invalid fixture mip size\n");return 2;}
            [source replaceRegion:MTLRegionMake2D(0,0,w,h) mipmapLevel:level withBytes:data.bytes bytesPerRow:w*16];
        }
        NSData *data=[NSData dataWithContentsOfFile:[@(argv[5]) stringByAppendingPathComponent:@"sdf.bin"]];
        if(data.length!=width*height*16){fprintf(stderr,"invalid fixture field size\n");return 2;}
        [sdf replaceRegion:MTLRegionMake2D(0,0,width,height) mipmapLevel:0 withBytes:data.bytes bytesPerRow:width*16];
    }
    NSData *uniforms=[NSData dataWithContentsOfFile:@(argv[3])];
    if (!uniforms) { fprintf(stderr,"missing uniforms\n"); free(pixels); return 1; }
    MTLRenderPassDescriptor *pass=[MTLRenderPassDescriptor renderPassDescriptor];
    pass.colorAttachments[0].texture=target;
    pass.colorAttachments[0].loadAction=MTLLoadActionClear;
    pass.colorAttachments[0].clearColor=MTLClearColorMake(0,0,0,0);
    pass.colorAttachments[0].storeAction=MTLStoreActionStore;
    id<MTLCommandQueue> queue=[device newCommandQueue];
    id<MTLCommandBuffer> command=[queue commandBuffer];
    id<MTLRenderCommandEncoder> encoder=[command renderCommandEncoderWithDescriptor:pass];
    [encoder setRenderPipelineState:pipeline];
    [encoder setFragmentTexture:source atIndex:3];
    [encoder setFragmentTexture:sdf atIndex:4];
    [encoder setFragmentBytes:uniforms.bytes length:uniforms.length atIndex:1];
    float edr=fixture ? [fixture[@"edr"] floatValue] : 1.f;
    if ([fragment.name hasSuffix:@"lph"]) { __fp16 halfEdr=edr; [encoder setFragmentBytes:&halfEdr length:sizeof(halfEdr) atIndex:6]; }
    else [encoder setFragmentBytes:&edr length:sizeof(edr) atIndex:6];
    [encoder drawPrimitives:MTLPrimitiveTypeTriangleStrip vertexStart:0 vertexCount:4];
    [encoder endEncoding]; [command commit]; [command waitUntilCompleted];
    if (command.error) { NSLog(@"GPU: %@",command.error); free(pixels); return 1; }
    [target getBytes:pixels bytesPerRow:width*16 fromRegion:MTLRegionMake2D(0,0,width,height) mipmapLevel:0];
    BOOL saved=[[NSData dataWithBytesNoCopy:pixels length:width*height*16 freeWhenDone:YES] writeToFile:@(argv[4]) atomically:YES];
    printf("REFERENCE device=%s function=%s size=%lux%lu uniforms=%lu output=%s\n",device.name.UTF8String,argv[2],(unsigned long)width,(unsigned long)height,(unsigned long)uniforms.length,argv[4]);
    return saved ? 0 : 1;
} }
