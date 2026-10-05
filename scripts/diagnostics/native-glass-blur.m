// Execute the installed variable-blur kernel with an imageblock-backed tile.
// Build: xcrun clang -fobjc-arc -framework Foundation -framework Metal this.m -o probe
// Run: probe output-directory
#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
int main(int argc,char **argv) { @autoreleasepool {
    if(argc!=2)return 2;
    NSString *output=@(argv[1]);[NSFileManager.defaultManager createDirectoryAtPath:output withIntermediateDirectories:YES attributes:nil error:nil];
    id<MTLDevice> device=MTLCreateSystemDefaultDevice();NSError *error=nil;
    NSURL *url=[NSURL fileURLWithPath:@"/System/Library/Frameworks/QuartzCore.framework/Versions/A/Resources/default.metallib"];
    id<MTLLibrary> library=[device newLibraryWithURL:url error:&error];
    id<MTLFunction> function=[library newFunctionWithName:@"variable_blur_downsample_compute"];
    id<MTLComputePipelineState> pipeline=[device newComputePipelineStateWithFunction:function error:&error];
    if(!pipeline){NSLog(@"pipeline error %@",error);return 1;}
    const NSUInteger width=128,height=96;
    MTLTextureDescriptor *td=[MTLTextureDescriptor texture2DDescriptorWithPixelFormat:MTLPixelFormatRGBA16Float width:width height:height mipmapped:YES];
    td.storageMode=MTLStorageModeShared;td.usage=MTLTextureUsageShaderRead|MTLTextureUsageShaderWrite;
    id<MTLTexture> source=[device newTextureWithDescriptor:td],target=[device newTextureWithDescriptor:td];
    NSMutableData *input=[NSMutableData dataWithLength:width*height*8];__fp16 *values=input.mutableBytes;
    for(NSUInteger y=0;y<height;y++)for(NSUInteger x=0;x<width;x++) {
        NSUInteger i=(y*width+x)*4;
        values[i]=(x%11)/10.f;values[i+1]=(y%7)/6.f;values[i+2]=((x+y)%5)/4.f;values[i+3]=1.f;
    }
    [source replaceRegion:MTLRegionMake2D(0,0,width,height) mipmapLevel:0 withBytes:values bytesPerRow:width*8];
    struct {uint16_t sourceLevel,targetLevel,width,height;float invWidth,invHeight;} params={0,1,width/2,height/2,2.f/width,2.f/height};
    id<MTLCommandQueue> queue=[device newCommandQueue];id<MTLCommandBuffer> command=[queue commandBuffer];
    id<MTLComputeCommandEncoder> encoder=[command computeCommandEncoder];
    [encoder setComputePipelineState:pipeline];[encoder setTexture:source atIndex:0];[encoder setTexture:target atIndex:1];
    [encoder setBytes:&params length:sizeof(params) atIndex:0];
    [encoder setImageblockWidth:16 height:16];
    [encoder dispatchThreadgroups:MTLSizeMake(width/32,height/32,1) threadsPerThreadgroup:MTLSizeMake(16,16,1)];
    [encoder endEncoding];[command commit];[command waitUntilCompleted];
    if(command.error){NSLog(@"GPU error %@",command.error);return 1;}
    NSMutableData *result=[NSMutableData dataWithLength:width/2*height/2*8];
    [target getBytes:result.mutableBytes bytesPerRow:width/2*8 fromRegion:MTLRegionMake2D(0,0,width/2,height/2) mipmapLevel:1];
    [input writeToFile:[output stringByAppendingPathComponent:@"source.rgba16f"] atomically:YES];
    [result writeToFile:[output stringByAppendingPathComponent:@"native.rgba16f"] atomically:YES];
    printf("device=%s kernel=%s source=%lux%lu target=%lux%lu\n",device.name.UTF8String,function.name.UTF8String,(unsigned long)width,(unsigned long)height,(unsigned long)(width/2),(unsigned long)(height/2));
} return 0; }
