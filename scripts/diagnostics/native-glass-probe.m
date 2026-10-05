// Inspect only a newly created diagnostic window on macOS 26 or later.
// Private properties are read for research; none are used by the VoidUI renderer.
// Build: xcrun clang -fobjc-arc -framework AppKit -framework QuartzCore \
//   scripts/diagnostics/native-glass-probe.m -o target/native-glass-probe
// Run: target/native-glass-probe [regular|clear] [dark|light] [active|inactive] [spacing gap]
#import <AppKit/AppKit.h>
#import <QuartzCore/QuartzCore.h>
#import <objc/runtime.h>

static NSString *describe(id value) {
    if (!value) return @"null";
    if ([value isKindOfClass:NSData.class]) {
        NSData *data = value;
        // CAColorMatrix is twenty native float32 values on this build.
        if (data.length == 80) {
            float entries[20]; [data getBytes:entries length:80];
            NSMutableArray *numbers = [NSMutableArray array];
            for (unsigned i=0;i<20;i++) [numbers addObject:@(entries[i])];
            return numbers.description;
        }
    }
    if (CFGetTypeID((__bridge CFTypeRef)value) == CGColorGetTypeID()) {
        CGColorRef color = (__bridge CGColorRef)value;
        const CGFloat *components = CGColorGetComponents(color);
        NSMutableArray *numbers = [NSMutableArray array];
        for (size_t i=0;i<CGColorGetNumberOfComponents(color);i++) [numbers addObject:@(components[i])];
        return [NSString stringWithFormat:@"color %@", numbers];
    }
    if ([value isKindOfClass:NSObject.class] && ![value isKindOfClass:NSString.class]
        && ![value isKindOfClass:NSNumber.class] && ![value isKindOfClass:NSArray.class]
        && ![value isKindOfClass:NSValue.class]) return NSStringFromClass([value class]);
    return [value description];
}
static void printValue(NSString *indent, NSString *key, id value) {
    NSString *line = [NSString stringWithFormat:@"%@%@ = %@", indent, key, describe(value)];
    // Single-line values make active/inactive diffs easier to read.
    line = [[line componentsSeparatedByCharactersInSet:NSCharacterSet.newlineCharacterSet] componentsJoinedByString:@" "];
    printf("%s\n", line.UTF8String);
}
static void dumpObject(id object, NSString *indent) {
    if (!object) return;
    printf("%s%s\n", indent.UTF8String, class_getName([object class]));
    if ([object respondsToSelector:NSSelectorFromString(@"inputKeys")]) {
        for (NSString *key in [object valueForKey:@"inputKeys"]) {
            @try { printValue(indent,key,[object valueForKey:key]); } @catch(NSException *e) {}
        }
    }
    NSString *name = NSStringFromClass([object class]);
    if ([name hasPrefix:@"CASDF"] || [name isEqualToString:@"CABackdropLayer"]) {
        unsigned count=0; objc_property_t *props=class_copyPropertyList([object class], &count);
        for (unsigned i=0;i<count;i++) {
            NSString *key=@(property_getName(props[i]));
            @try { printValue(indent,key,[object valueForKey:key]); } @catch(NSException *e) {}
        }
        free(props);
        if ([object isKindOfClass:CALayer.class] && [object respondsToSelector:NSSelectorFromString(@"effect")])
            dumpObject([object valueForKey:@"effect"], [indent stringByAppendingString:@"  effect "]);
    }
}
static void dumpLayer(CALayer *layer, NSString *indent) {
    dumpObject(layer, indent);
    if (layer.name) printValue(indent,@"name",layer.name);
    if ([NSStringFromClass(layer.class) isEqualToString:@"CASDFElementLayer"]) {
        printValue(indent,@"bounds",[NSValue valueWithRect:layer.bounds]);
        printValue(indent,@"cornerRadius",@(layer.cornerRadius));
        printValue(indent,@"cornerCurve",layer.cornerCurve);
    }
    for (id filter in layer.filters) dumpObject(filter,[indent stringByAppendingString:@"  filter "]);
    for (id filter in layer.backgroundFilters) dumpObject(filter,[indent stringByAppendingString:@"  background "]);
    for (CALayer *child in layer.sublayers) dumpLayer(child,[indent stringByAppendingString:@"  "]);
}
int main(int argc, char **argv) { @autoreleasepool {
    BOOL clear=argc>1 && strcmp(argv[1],"clear")==0;
    BOOL light=argc>2 && strcmp(argv[2],"light")==0;
    BOOL inactive=argc>3 && strcmp(argv[3],"inactive")==0;
    NSApplication *app=NSApplication.sharedApplication;
    [app setActivationPolicy:inactive ? NSApplicationActivationPolicyAccessory : NSApplicationActivationPolicyRegular];
    NSWindow *window=[[NSWindow alloc] initWithContentRect:NSMakeRect(0,0,640,420)
        styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskClosable backing:NSBackingStoreBuffered defer:NO];
    window.appearance=[NSAppearance appearanceNamed:light ? NSAppearanceNameAqua : NSAppearanceNameDarkAqua];
    window.contentView.wantsLayer=YES;
    window.contentView.layer.backgroundColor=NSColor.systemBlueColor.CGColor;
    NSGlassEffectView *glass=[[NSGlassEffectView alloc] initWithFrame:NSMakeRect(70,80,400,230)];
    glass.cornerRadius=40;
    glass.style=clear ? NSGlassEffectViewStyleClear : NSGlassEffectViewStyleRegular;
    glass.contentView=[NSTextField labelWithString:@"Native glass runtime probe"];
    if (argc > 5) {
        CGFloat spacing=strtod(argv[4],NULL), gap=strtod(argv[5],NULL);
        NSGlassEffectContainerView *container=[[NSGlassEffectContainerView alloc] initWithFrame:NSMakeRect(0,0,640,420)];
        container.spacing=spacing;
        NSView *content=[[NSView alloc] initWithFrame:container.bounds];
        container.contentView=content;
        glass.frame=NSMakeRect(80,120,140,140);
        glass.cornerRadius=40;
        NSGlassEffectView *second=[[NSGlassEffectView alloc] initWithFrame:NSMakeRect(220+gap,120,140,140)];
        second.cornerRadius=40;
        second.style=glass.style;
        second.contentView=[NSTextField labelWithString:@"Second glass"];
        [content addSubview:glass];
        [content addSubview:second];
        [window.contentView addSubview:container];
        printf("CONTAINER spacing=%g gap=%g\n",(double)spacing,(double)gap);
    } else [window.contentView addSubview:glass];
    if (!inactive) [app finishLaunching];
    if (inactive) [window orderFront:nil];
    else { [window makeKeyAndOrderFront:nil]; [app activateIgnoringOtherApps:YES]; }
    if (inactive) { [window resignKeyWindow]; [app deactivate]; }
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW,2*NSEC_PER_SEC),dispatch_get_main_queue(), ^{
        printf("OS %s\n",NSProcessInfo.processInfo.operatingSystemVersionString.UTF8String);
        printf("STATE style=%s active=%d key=%d reducedTransparency=%d appearance=%s\n",clear?"clear":"regular",
            app.active,window.keyWindow,NSWorkspace.sharedWorkspace.accessibilityDisplayShouldReduceTransparency,
            glass.effectiveAppearance.name.UTF8String);
        for (NSString *name in @[@"NSGlassEffectView",@"CAFilter",@"CABackdropLayer",@"CASDFLayer",@"CASDFKeyFillHighlightEffect"]) {
            Class cls=NSClassFromString(name);
            printf("IMAGE %s %s\n",name.UTF8String,cls ? class_getImageName(cls) : "unavailable");
        }
        dumpLayer(window.contentView.layer,@"");
#ifdef GLASS_TRACE
        extern void TraceGlassLayer(CALayer *);
        TraceGlassLayer(window.contentView.layer);
#endif
        // Optional inspection interval for comparing the live native window.
        const char *hold=getenv("GLASS_PROBE_HOLD_SECONDS");
        if (hold) {
            fflush(stdout);
            [NSRunLoop.currentRunLoop runUntilDate:[NSDate dateWithTimeIntervalSinceNow:fmax(0.,strtod(hold,NULL))]];
        }
        [window orderOut:nil];
        exit(0);
    });
    if (inactive) [NSRunLoop.currentRunLoop runUntilDate:[NSDate dateWithTimeIntervalSinceNow:3]];
    else [app run];
} return 0; }
