// Microphone permission and keeping the Mac awake while recording.

#import <AVFoundation/AVFoundation.h>
#import <Foundation/Foundation.h>

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
