// Microphone permission and keeping the Mac awake while recording.

#import <AVFoundation/AVFoundation.h>
#import <Foundation/Foundation.h>
#include <dlfcn.h>

// AVAuthorizationStatus: 0 not determined, 1 restricted, 2 denied, 3 authorized.
int echonote_mic_permission(void) {
    return (int)[AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeAudio];
}

// Show the system prompt if not asked yet and wait for the answer. Returns 1 when granted.
// Must not be called on the main thread.
int echonote_mic_request(void) {
    dispatch_semaphore_t done = dispatch_semaphore_create(0);
    __block BOOL granted = NO;
    [AVCaptureDevice requestAccessForMediaType:AVMediaTypeAudio
                             completionHandler:^(BOOL allowed) {
                               granted = allowed;
                               dispatch_semaphore_signal(done);
                             }];
    dispatch_semaphore_wait(done, DISPATCH_TIME_FOREVER);
    return granted ? 1 : 0;
}

// System audio recording permission (kTCCServiceAudioCapture). There is no public API to
// read it; this uses TCCAccessPreflight from the private TCC framework, as AudioCap does.
// Returns 0 granted, 1 denied, 2 not determined or unknown (including when the symbol is
// missing). Creating a process tap shows the prompt; a denied tap delivers silence.
int echonote_audio_capture_permission(void) {
    static long (*preflight)(CFStringRef, CFDictionaryRef) = NULL;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
      void *tcc = dlopen("/System/Library/PrivateFrameworks/TCC.framework/Versions/A/TCC", RTLD_NOW);
      if (tcc != NULL) {
          preflight = (long (*)(CFStringRef, CFDictionaryRef))dlsym(tcc, "TCCAccessPreflight");
      }
    });
    if (preflight == NULL) {
        return 2;
    }
    long result = preflight(CFSTR("kTCCServiceAudioCapture"), NULL);
    return result == 0 ? 0 : result == 1 ? 1 : 2;
}

// Prevent idle sleep and App Nap (which would slow the capture thread) until the token is
// passed to echonote_keep_awake_end.
void *echonote_keep_awake_begin(void) {
    @autoreleasepool {
        id token = [[NSProcessInfo processInfo]
            beginActivityWithOptions:NSActivityUserInitiated | NSActivityLatencyCritical
                              reason:@"Recording a meeting"];
        return (__bridge_retained void *)token;
    }
}

void echonote_keep_awake_end(void *token) {
    if (token == NULL) {
        return;
    }
    @autoreleasepool {
        id activity = (__bridge_transfer id)token;
        [[NSProcessInfo processInfo] endActivity:activity];
    }
}
