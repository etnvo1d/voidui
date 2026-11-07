// UIKit-only iOS 26 probe. Layer properties belong to this newly created window.
// Build/install with build-ios-glass-probe.sh. Evidence is written to Documents.
#import <UIKit/UIKit.h>
#import <QuartzCore/QuartzCore.h>
#import <objc/runtime.h>

static NSString *argument(NSString *key, NSString *fallback) {
    NSArray *args = NSProcessInfo.processInfo.arguments;
    NSUInteger index = [args indexOfObject:key];
    return index != NSNotFound && index + 1 < args.count ? args[index + 1] : fallback;
}
static NSString *describe(id value) {
    if (!value) return @"null";
    if ([value isKindOfClass:NSValue.class] && strcmp([value objCType], @encode(CATransform3D)) == 0) {
        CATransform3D t; [value getValue:&t size:sizeof(t)];
        const CGFloat *v = (const CGFloat *)&t; NSMutableArray *a = [NSMutableArray array];
        for (NSUInteger i = 0; i < 16; i++) [a addObject:@(v[i])];
        return a.description;
    }
    if ([value isKindOfClass:NSData.class] && [value length] == 80) {
        float entries[20]; [value getBytes:entries length:80];
        NSMutableArray *result = [NSMutableArray array];
        for (NSUInteger i = 0; i < 20; i++) [result addObject:@(entries[i])];
        return result.description;
    }
    if (CFGetTypeID((__bridge CFTypeRef)value) == CGColorGetTypeID()) {
        CGColorRef color = (__bridge CGColorRef)value;
        NSMutableArray *components = [NSMutableArray array];
        for (NSUInteger i = 0; i < CGColorGetNumberOfComponents(color); i++)
            [components addObject:@(CGColorGetComponents(color)[i])];
        return [NSString stringWithFormat:@"color %@",components];
    }
    return [value description];
}
static void property(NSMutableString *log, id object, NSString *key, NSString *indent) {
    @try {
        NSString *value;
        if ([object isKindOfClass:CALayer.class] && ([key isEqualToString:@"transform"] || [key isEqualToString:@"sublayerTransform"])) {
            CATransform3D t = [key isEqualToString:@"transform"] ? [(CALayer *)object transform] : [(CALayer *)object sublayerTransform];
            const CGFloat *v = (const CGFloat *)&t; NSMutableArray *a = [NSMutableArray array];
            for (NSUInteger i = 0; i < 16; i++) [a addObject:@(v[i])]; value = a.description;
        } else value = describe([object valueForKey:key]);
        value = [[value componentsSeparatedByCharactersInSet:NSCharacterSet.newlineCharacterSet] componentsJoinedByString:@" "];
        [log appendFormat:@"%@%@ = %@\n",indent,key,value];
    } @catch(NSException *exception) {}
}
static void object(NSMutableString *log, id value, NSString *indent) {
    if (!value) return;
    [log appendFormat:@"%@%s\n",indent,class_getName([value class])];
    if ([value respondsToSelector:NSSelectorFromString(@"inputKeys")]) {
        for (NSString *key in [value valueForKey:@"inputKeys"]) property(log,value,key,indent);
    }
    NSString *name = NSStringFromClass([value class]);
    if ([name isEqualToString:@"CAFilter"]) {
        property(log,value,@"name",indent);
        // iOS omits inputKeys on some filters. Query only the parameter names
        // already observed in this QuartzCore shader family, without mutations.
        for (NSString *key in @[@"inputBlurRadius", @"inputAmount", @"inputRadius", @"inputNormalizeEdges",
            @"inputInnerRefractionAmount", @"inputInnerRefractionHeight", @"inputOuterRefractionAmount", @"inputOuterRefractionHeight",
            @"inputRefractionOpacity", @"inputRefractionDistance0", @"inputRefractionDistance1",
            @"inputFaceOpacity", @"inputFaceColorMatrixWhite", @"inputFaceColorMatrixBlack", @"inputFaceColorMatrixSaturation", @"inputFaceColorMatrixFillColor",
            @"inputBlurOpacity0", @"inputBlurOpacity1", @"inputBlurOpacity2", @"inputBlurOpacity3",
            @"inputBlurDistance0", @"inputBlurDistance1", @"inputBlurDistance2", @"inputBlurDistance3",
            @"inputBleedAmount", @"inputBleedHeight", @"inputBleedBlurRadius", @"inputBleedOpacity", @"inputShadowOpacity",
            @"inputShadowOffset", @"inputShadowAmount", @"inputShadowHeight", @"inputShadowBlurRadius", @"inputShadowRadius", @"inputShadowDistanceOffset",
            @"inputShadowColorMatrixWhite", @"inputShadowColorMatrixBlack", @"inputShadowColorMatrixSaturation", @"inputShadowColorMatrixFillColor",
            @"inputShadowContribution", @"inputHoldingToneOpacity", @"inputSDRWhiteLevel", @"inputSDRShadowOpacity", @"inputSDRShadowDistance0", @"inputSDRShadowDistance1", @"inputClamp", @"inputColorMatrix"])
            property(log,value,key,indent);
    }
    if ([name hasPrefix:@"CASDF"] || [name isEqualToString:@"CABackdropLayer"] || [name isEqualToString:@"CAPortalLayer"]) {
        unsigned count = 0;
        objc_property_t *props = class_copyPropertyList([value class], &count);
        for (unsigned i = 0; i < count; i++) property(log,value,@(property_getName(props[i])),indent);
        free(props);
        if ([value respondsToSelector:NSSelectorFromString(@"effect")])
            object(log,[value valueForKey:@"effect"],[indent stringByAppendingString:@" effect "]);
    }
}
static void layer(NSMutableString *log, CALayer *value, NSString *indent) {
    object(log,value,indent);
    for (NSString *key in @[@"transform",@"sublayerTransform",@"position",@"opacity",@"hidden",@"cornerRadius",@"cornerCurve"]) property(log,value,key,indent);
    if (value.delegate) [log appendFormat:@"%@delegateClass=%@\n",indent,NSStringFromClass([value.delegate class])];
    [log appendFormat:@"%@bounds=%@ contentsScale=%g opaque=%d\n",indent,NSStringFromCGRect(value.bounds),value.contentsScale,value.opaque];
    for (id filter in value.filters) object(log,filter,[indent stringByAppendingString:@" filter "]);
    for (id filter in value.backgroundFilters) object(log,filter,[indent stringByAppendingString:@" background "]);
    for (CALayer *child in value.sublayers) layer(log,child,[indent stringByAppendingString:@"  "]);
}

@interface Backdrop : UIView
@end
@implementation Backdrop
- (void)drawRect:(CGRect)rect {
    [UIColor.whiteColor setFill]; UIRectFill(rect);
    NSString *text = [@"The quick brown fox jumps over the lazy dog. " stringByPaddingToLength:6000
        withString:@"The quick brown fox jumps over the lazy dog. " startingAtIndex:0];
    [text drawInRect:self.bounds withAttributes:@{NSFontAttributeName:[UIFont systemFontOfSize:16],
        NSForegroundColorAttributeName:UIColor.blackColor}];
    // Device-pixel stems distinguish downsampling from a mere color transfer.
    CGFloat unit = 1 / self.contentScaleFactor;
    for (CGFloat x = 0; x < self.bounds.size.width; x += 4 * unit) {
        [UIColor.blackColor setFill]; UIRectFill(CGRectMake(x, 240, 2 * unit, 30));
        [UIColor.whiteColor setFill]; UIRectFill(CGRectMake(x + 2 * unit, 240, 2 * unit, 30));
    }
}
@end

@interface ProbeDelegate : UIResponder <UIApplicationDelegate>
@property(nonatomic, strong) UIWindow *window;
@property(nonatomic, strong) UITextLoupeSession *loupe;
@end
@implementation ProbeDelegate
- (BOOL)application:(UIApplication *)application didFinishLaunchingWithOptions:(NSDictionary *)options {
    self.window = [[UIWindow alloc] initWithFrame:UIScreen.mainScreen.bounds];
    UIViewController *controller = [[UIViewController alloc] init];
    controller.view.backgroundColor = UIColor.whiteColor;
    controller.overrideUserInterfaceStyle = [argument(@"--appearance",@"light") isEqualToString:@"dark"] ? UIUserInterfaceStyleDark : UIUserInterfaceStyleLight;
    self.window.rootViewController = controller;
    Backdrop *backdrop = [[Backdrop alloc] initWithFrame:controller.view.bounds];
    backdrop.autoresizingMask = UIViewAutoresizingFlexibleWidth | UIViewAutoresizingFlexibleHeight;
    [controller.view addSubview:backdrop];
    BOOL clear = [argument(@"--style",@"clear") isEqualToString:@"clear"];
    UIGlassEffect *effect = [UIGlassEffect effectWithStyle:clear ? UIGlassEffectStyleClear : UIGlassEffectStyleRegular];
    UIGlassContainerEffect *group = [[UIGlassContainerEffect alloc] init]; group.spacing = 32;
    UIVisualEffectView *container = [[UIVisualEffectView alloc] initWithEffect:group];
    container.frame = controller.view.bounds;
    container.autoresizingMask = UIViewAutoresizingFlexibleWidth | UIViewAutoresizingFlexibleHeight;
    [controller.view addSubview:container];
    CGFloat width = [argument(@"--width",@"300") doubleValue];
    CGFloat height = [argument(@"--height",@"260") doubleValue];
    UIVisualEffectView *glass = [[UIVisualEffectView alloc] initWithEffect:effect];
    glass.frame = CGRectMake(40,140,width,height);
    glass.layer.cornerRadius = [argument(@"--radius",@"60") doubleValue];
    glass.clipsToBounds = YES;
    [container.contentView addSubview:glass];
    [self.window makeKeyAndVisible];
    BOOL magnifier = [argument(@"--magnifier",@"no") isEqualToString:@"yes"];
    if (magnifier) {
        container.hidden = YES; backdrop.hidden = YES;
        UITextView *editor = [[UITextView alloc] initWithFrame:CGRectMake(24,140,350,260)];
        editor.font = [UIFont systemFontOfSize:24];
        editor.text = @"The quick brown fox jumps over the lazy dog.\nMagnifier detail: ABCDEFG 0123456789";
        editor.autocorrectionType = UITextAutocorrectionTypeNo;
        [controller.view addSubview:editor];
        [editor becomeFirstResponder];
        if ([argument(@"--hold-loupe",@"no") isEqualToString:@"yes"]) {
            dispatch_after(dispatch_time(DISPATCH_TIME_NOW,1*NSEC_PER_SEC),dispatch_get_main_queue(), ^{
                CGPoint point = CGPointMake(130,25);
                self.loupe = [UITextLoupeSession beginLoupeSessionAtPoint:point fromSelectionWidgetView:nil inView:editor];
                [self.loupe moveToPoint:point withCaretRect:CGRectNull trackingCaret:NO];
            });
        }
        // Capture only this app's windows for a bounded interval. Keeping the
        // loupe visible after capture does not need continuous diagnostic I/O.
        NSTimeInterval deadline = NSDate.timeIntervalSinceReferenceDate + MAX(1,[argument(@"--trace-seconds",@"30") doubleValue]);
        [NSTimer scheduledTimerWithTimeInterval:0.5 repeats:YES block:^(NSTimer *timer) {
            if (NSDate.timeIntervalSinceReferenceDate > deadline) { [timer invalidate]; return; }
            NSMutableString *report = [NSMutableString string];
            NSArray *windows = self.window.windowScene ? self.window.windowScene.windows : application.windows;
            for (UIWindow *window in windows) layer(report,window.layer,@"");
            NSString *directory = NSSearchPathForDirectoriesInDomains(NSDocumentDirectory,NSUserDomainMask,YES).firstObject;
            [report writeToFile:[directory stringByAppendingPathComponent:@"magnifier-latest.txt"] atomically:YES encoding:NSUTF8StringEncoding error:nil];
            if ([report containsString:@"Magnifier"] || [report containsString:@"magnifier"] || [report containsString:@"Loupe"])
                [report writeToFile:[directory stringByAppendingPathComponent:@"magnifier-active.txt"] atomically:YES encoding:NSUTF8StringEncoding error:nil];
        }];
    }
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW,3*NSEC_PER_SEC),dispatch_get_main_queue(), ^{
        NSMutableString *log = [NSMutableString stringWithFormat:@"OS %@\nSTYLE %@ scale=%g reduceTransparency=%d\n",
            NSProcessInfo.processInfo.operatingSystemVersionString,clear?@"clear":@"regular",self.window.screen.scale,UIAccessibilityIsReduceTransparencyEnabled()];
        layer(log,self.window.layer,@"");
        NSString *directory = NSSearchPathForDirectoriesInDomains(NSDocumentDirectory,NSUserDomainMask,YES).firstObject;
        [log writeToFile:[directory stringByAppendingPathComponent:@"layers.txt"] atomically:YES encoding:NSUTF8StringEncoding error:nil];
        // Export only the source, not a purported screenshot of the compositor.
        UIGraphicsImageRenderer *renderer = [[UIGraphicsImageRenderer alloc] initWithBounds:backdrop.bounds];
        UIImage *source = [renderer imageWithActions:^(UIGraphicsImageRendererContext *context) {
            [backdrop.layer renderInContext:context.CGContext];
        }];
        [UIImagePNGRepresentation(source) writeToFile:[directory stringByAppendingPathComponent:@"source.png"] atomically:YES];
        NSLog(@"VOIDUI_GLASS_EVIDENCE %@",directory);
    });
    return YES;
}
@end
int main(int argc,char **argv) { @autoreleasepool {
    return UIApplicationMain(argc,argv,nil,NSStringFromClass(ProbeDelegate.class));
} }
