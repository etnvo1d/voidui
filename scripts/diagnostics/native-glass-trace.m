// In-process Metal trace of a generic copy of this diagnostic window's layers.
// This reproduces local Core Animation rendering, not a WindowServer capture.
// Build together with native-glass-probe.m and -DGLASS_TRACE -framework Metal.
#import <AppKit/AppKit.h>
#import <QuartzCore/QuartzCore.h>
#import <Metal/Metal.h>
#import <objc/runtime.h>

static const char PipelineNameKey, EncoderNameKey;
static NSString *outputDirectory;
static NSUInteger serial;
static void (*originalState)(id,SEL,id);
static void (*originalBytes)(id,SEL,const void *,NSUInteger,NSUInteger);
static void (*originalBuffer)(id,SEL,id,NSUInteger,NSUInteger);
static void (*originalTexture)(id,SEL,id,NSUInteger);
static id (*originalPipeline)(id,SEL,id,NSUInteger,id*,NSError**);
static id (*originalSimplePipeline)(id,SEL,id,NSError**);
static void saveBytes(id encoder,const void *bytes,NSUInteger length,NSUInteger index) {
    if (!bytes || length==0) return;
    NSString *name=objc_getAssociatedObject(encoder,&EncoderNameKey) ?: @"unknown";
    NSUInteger captured=MIN(length,4096);
    if(index==1 && [name hasPrefix:@"glass_background"])
        captured=MIN(length,[name hasSuffix:@"lph"] ? 224 : 272);
    NSString *file=[NSString stringWithFormat:@"%04lu-%@-slot%lu.bin",(unsigned long)serial++,name,(unsigned long)index];
    [[NSData dataWithBytes:bytes length:captured] writeToFile:[outputDirectory stringByAppendingPathComponent:file] atomically:YES];
    printf("TRACE %s bytes=%lu\n",file.UTF8String,(unsigned long)length);
}
static void state(id self,SEL sel,id pipeline) {
    objc_setAssociatedObject(self,&EncoderNameKey,objc_getAssociatedObject(pipeline,&PipelineNameKey),OBJC_ASSOCIATION_COPY_NONATOMIC);
    originalState(self,sel,pipeline);
}
static void bytes(id self,SEL sel,const void *data,NSUInteger length,NSUInteger index) {
    saveBytes(self,data,length,index);originalBytes(self,sel,data,length,index);
}
static void buffer(id self,SEL sel,id<MTLBuffer> data,NSUInteger offset,NSUInteger index) {
    if(data && data.storageMode!=MTLStorageModePrivate && offset<data.length)
        saveBytes(self,(const char*)data.contents+offset,data.length-offset,index);
    originalBuffer(self,sel,data,offset,index);
}
static void texture(id self,SEL sel,id<MTLTexture> data,NSUInteger index) {
    // Record the actual source dimensions; CAFilter radius alone cannot explain
    // the loss of fine text in a downsampled backdrop.
    NSString *name=objc_getAssociatedObject(self,&EncoderNameKey) ?: @"unknown";
    if(data) printf("TEXTURE %s slot=%lu size=%lux%lu mips=%lu format=%lu\n",
        name.UTF8String,(unsigned long)index,(unsigned long)data.width,
        (unsigned long)data.height,(unsigned long)data.mipmapLevelCount,
        (unsigned long)data.pixelFormat);
    originalTexture(self,sel,data,index);
}
static id pipeline(id self,SEL sel,MTLRenderPipelineDescriptor *desc,NSUInteger options,id *reflection,NSError **error) {
    id result=originalPipeline(self,sel,desc,options,reflection,error);
    objc_setAssociatedObject(result,&PipelineNameKey,desc.fragmentFunction.name,OBJC_ASSOCIATION_COPY_NONATOMIC);
    printf("PIPELINE %s\n",desc.fragmentFunction.name.UTF8String);return result;
}
static id simplePipeline(id self,SEL sel,MTLRenderPipelineDescriptor *desc,NSError **error) {
    id result=originalSimplePipeline(self,sel,desc,error);
    objc_setAssociatedObject(result,&PipelineNameKey,desc.fragmentFunction.name,OBJC_ASSOCIATION_COPY_NONATOMIC);
    printf("PIPELINE %s\n",desc.fragmentFunction.name.UTF8String);return result;
}
static IMP replace(Class cls,SEL selector,IMP replacement) {
    Method m=class_getInstanceMethod(cls,selector);
    if(!m) { fprintf(stderr,"missing trace selector %s\n",sel_getName(selector)); exit(2); }
    IMP previous=method_getImplementation(m);
    class_replaceMethod(cls,selector,replacement,method_getTypeEncoding(m));return previous;
}
static CALayer *copyTree(CALayer *layer, NSMapTable *mapping) {
    NSString *name=NSStringFromClass(layer.class);
    Class kind=CALayer.class;
    if ([name isEqualToString:@"CABackdropLayer"] || [name isEqualToString:@"CASDFLayer"] || [name isEqualToString:@"CASDFElementLayer"]) kind=layer.class;
    if ([name containsString:@"PortalLayer"]) kind=NSClassFromString(@"CAPortalLayer");
    CALayer *copy=[kind layer];
    copy.delegate=nil;
    copy.bounds=layer.bounds;copy.position=layer.position;copy.anchorPoint=layer.anchorPoint;
    copy.transform=layer.transform;copy.sublayerTransform=layer.sublayerTransform;
    copy.opacity=layer.opacity;copy.hidden=layer.hidden;copy.masksToBounds=layer.masksToBounds;
    // NSView backing contents have separate ownership/format state. Retain the
    // generic layer geometry/effects, excluding text contents from this fixture.
    copy.contents=nil;copy.contentsScale=layer.contentsScale;
    // Only this explicitly supplied, owned fixture has transferable image contents.
    if([layer.name isEqualToString:@"glass-probe-backdrop"]) copy.contents=layer.contents;
    copy.backgroundColor=layer.backgroundColor;copy.cornerRadius=layer.cornerRadius;copy.cornerCurve=layer.cornerCurve;
    copy.filters=layer.filters;copy.backgroundFilters=layer.backgroundFilters;copy.compositingFilter=layer.compositingFilter;
    copy.geometryFlipped=layer.geometryFlipped;copy.name=layer.name;
    for (NSString *key in @[@"effect",@"smoothness",@"gaussianRadius",@"effectOffset",@"mode",@"operation",@"gradientOvalization",@"scale"]) {
        @try { [copy setValue:[layer valueForKey:key] forKey:key]; } @catch(NSException *e) {}
    }
    [mapping setObject:copy forKey:layer];
    NSMutableArray *children=[NSMutableArray array];
    for (CALayer *child in layer.sublayers) [children addObject:copyTree(child,mapping)];
    copy.sublayers=children;
    return copy;
}
void TraceGlassLayer(CALayer *layer) {
    const char *output=getenv("GLASS_TRACE_OUTPUT");
    outputDirectory=output ? @(output) : @"target/liquid-glass/native-trace";
    [NSFileManager.defaultManager createDirectoryAtPath:outputDirectory withIntermediateDirectories:YES attributes:nil error:nil];
    id<MTLDevice> device=MTLCreateSystemDefaultDevice();
    id<MTLCommandQueue> queue=[device newCommandQueue];
    NSUInteger width=MAX(1,(NSUInteger)layer.bounds.size.width),height=MAX(1,(NSUInteger)layer.bounds.size.height);
    MTLTextureDescriptor *td=[MTLTextureDescriptor texture2DDescriptorWithPixelFormat:MTLPixelFormatBGRA8Unorm width:width height:height mipmapped:NO];
    td.usage=MTLTextureUsageRenderTarget|MTLTextureUsageShaderRead;td.storageMode=MTLStorageModeShared;
    id<MTLTexture> target=[device newTextureWithDescriptor:td];
    MTLRenderPassDescriptor *pd=[MTLRenderPassDescriptor renderPassDescriptor];pd.colorAttachments[0].texture=target;
    id<MTLCommandBuffer> cb=[queue commandBuffer];id<MTLRenderCommandEncoder> encoder=[cb renderCommandEncoderWithDescriptor:pd];
    Class cls=[encoder class];
    originalState=(void*)replace(cls,@selector(setRenderPipelineState:),(IMP)state);
    originalBytes=(void*)replace(cls,@selector(setFragmentBytes:length:atIndex:),(IMP)bytes);
    originalBuffer=(void*)replace(cls,@selector(setFragmentBuffer:offset:atIndex:),(IMP)buffer);
    originalTexture=(void*)replace(cls,@selector(setFragmentTexture:atIndex:),(IMP)texture);
    originalPipeline=(void*)replace([device class],@selector(newRenderPipelineStateWithDescriptor:options:reflection:error:),(IMP)pipeline);
    originalSimplePipeline=(void*)replace([device class],@selector(newRenderPipelineStateWithDescriptor:error:),(IMP)simplePipeline);
    [encoder endEncoding];[cb commit];[cb waitUntilCompleted];
    CARenderer *renderer=[CARenderer rendererWithMTLTexture:target options:@{kCARendererMetalCommandQueue:queue}];
    NSMapTable *mapping=[NSMapTable strongToStrongObjectsMapTable];
    CALayer *copy=copyTree(layer,mapping);
    for (CALayer *original in mapping) {
        if ([original respondsToSelector:NSSelectorFromString(@"sourceLayer")]) {
            id source=[original valueForKey:@"sourceLayer"];
            id target=[mapping objectForKey:source];
            if (target) [[mapping objectForKey:original] setValue:target forKey:@"sourceLayer"];
        }
    }
    CALayer *root=[CALayer layer];root.frame=CGRectMake(0,0,width,height);
    root.backgroundColor=NSColor.systemRedColor.CGColor;
    [root addSublayer:copy];
    renderer.layer=root;renderer.bounds=root.bounds;
    printf("COPY bounds=%s sublayers=%lu\n",NSStringFromRect(copy.bounds).UTF8String,(unsigned long)copy.sublayers.count);
    fflush(stdout);
    [CATransaction flush];
    [renderer beginFrameAtTime:CACurrentMediaTime() timeStamp:NULL];
    [renderer addUpdateRect:renderer.bounds];[renderer render];[renderer endFrame];
    cb=[queue commandBuffer];[cb commit];[cb waitUntilCompleted];
    NSMutableData *pixels=[NSMutableData dataWithLength:width*height*4];
    [target getBytes:pixels.mutableBytes bytesPerRow:width*4 fromRegion:MTLRegionMake2D(0,0,width,height) mipmapLevel:0];
    [pixels writeToFile:[outputDirectory stringByAppendingPathComponent:@"frame.bgra8"] atomically:YES];
    CGDataProviderRef provider=CGDataProviderCreateWithCFData((__bridge CFDataRef)pixels);
    CGColorSpaceRef colorSpace=CGColorSpaceCreateDeviceRGB();
    CGImageRef image=CGImageCreate(width,height,8,32,width*4,colorSpace,
        kCGBitmapByteOrder32Little|kCGImageAlphaPremultipliedFirst,provider,NULL,NO,kCGRenderingIntentDefault);
    NSBitmapImageRep *bitmap=[[NSBitmapImageRep alloc] initWithCGImage:image];
    [[bitmap representationUsingType:NSBitmapImageFileTypePNG properties:@{}]
        writeToFile:[outputDirectory stringByAppendingPathComponent:@"frame.png"] atomically:YES];
    CGImageRelease(image);CGColorSpaceRelease(colorSpace);CGDataProviderRelease(provider);
    printf("TRACE_FRAME %lu %lu\n",(unsigned long)width,(unsigned long)height);
    NSMutableDictionary *summary=[NSMutableDictionary dictionary];
    summary[@"width"]=@(width);summary[@"height"]=@(height);
    summary[@"scope"]=@"Generic clone of native probe layers; NSView backing contents omitted, supplied backdrop fixture retained; not WindowServer capture";
    NSData *json=[NSJSONSerialization dataWithJSONObject:summary options:NSJSONWritingPrettyPrinted error:nil];
    [json writeToFile:[outputDirectory stringByAppendingPathComponent:@"frame.json"] atomically:YES];
}
