// Read and invoke one known QuartzCore color conversion routine in this process.
// Pass the unslid address obtained from this build's dyld_info disassembly;
// no addresses or system-version offsets are baked into this diagnostic.
#import <Foundation/Foundation.h>
#import <QuartzCore/QuartzCore.h>
#import <mach-o/dyld.h>
#import <dlfcn.h>
int main(int argc,char **argv) { @autoreleasepool {
    if(argc!=9 && argc!=3) {fprintf(stderr,"usage: native-color-matrix unslid-address white black saturation r g b alpha\n");return 2;}
    dlopen("/System/Library/Frameworks/QuartzCore.framework/Versions/A/QuartzCore",RTLD_NOW);
    intptr_t slide=0;BOOL found=NO;
    for(uint32_t i=0;i<_dyld_image_count();i++) {
        if(strstr(_dyld_get_image_name(i),"/QuartzCore.framework/")) {slide=_dyld_get_image_vmaddr_slide(i);found=YES;break;}
    }
    if(!found) return 1;
    uintptr_t address=strtoull(argv[1],NULL,0)+slide;
    if(argc==3 && strcmp(argv[2],"read20")==0) {
        const float *values=(const float*)address;
        for(unsigned i=0;i<20;i++)printf("%.9g%s",values[i],i==19?"\n":" ");
        return 0;
    }
    float matrix[20]={0},fill[4];
    for(int i=0;i<4;i++)fill[i]=strtof(argv[i+5],NULL);
    // This native call receives premultiplied fill RGB, as get_float_color_key does.
    for(int i=0;i<3;i++)fill[i]*=fill[3];
    typedef void (*SetMatrix)(float *,float,float,float,const float *);
    ((SetMatrix)address)(matrix,strtof(argv[2],NULL),strtof(argv[3],NULL),strtof(argv[4],NULL),fill);
    for(unsigned i=0;i<20;i++)printf("%.9g%s",matrix[i],i==19?"\n":" ");
} return 0; }
