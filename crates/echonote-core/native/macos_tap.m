// System audio capture on macOS 14.2+ through a Core Audio process tap.
//
// The tap mixes every process except this one into mono float32. A private aggregate device
// hosts the tap and an IOProc forwards each buffer to the Rust callback with its age (how long
// ago its first frame was captured). Opening and starting are separate so the caller can size
// its ring buffer from the tap format before any callback runs.

#import <CoreAudio/AudioHardware.h>
#import <CoreAudio/AudioHardwareTapping.h>
#import <CoreAudio/CATapDescription.h>
#import <CoreAudio/HostTime.h>
#import <Foundation/Foundation.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

typedef void (*echonote_tap_callback)(void *context, const float *samples, uint32_t count,
                                      uint64_t age_ns);

typedef struct echonote_tap_format {
    double sample_rate;
    uint32_t channels;
    uint32_t format_id;
    uint32_t format_flags;
    uint32_t bits_per_channel;
} echonote_tap_format;

typedef struct echonote_tap {
    AudioObjectID tap_id;
    AudioObjectID aggregate_id;
    AudioDeviceIOProcID io_proc;
    echonote_tap_callback callback;
    void *context;
    echonote_tap_format format;
    // Set by property listeners when the default output device or the tap format changes;
    // the caller then replaces the tap.
    atomic_int changed;
    int listening;
    // Diagnostics, printed to stderr on stop when ECHONOTE_TAP_DEBUG is set.
    atomic_uint_fast64_t io_calls;
    atomic_uint_fast64_t empty_calls;
    atomic_uint_fast32_t last_buffers;
} echonote_tap;

static OSStatus io_proc(AudioObjectID device, const AudioTimeStamp *now, const AudioBufferList *input,
                        const AudioTimeStamp *input_time, AudioBufferList *output,
                        const AudioTimeStamp *output_time, void *client) {
    (void)device; (void)output; (void)output_time;
    echonote_tap *tap = (echonote_tap *)client;
    atomic_fetch_add_explicit(&tap->io_calls, 1, memory_order_relaxed);
    if (input == NULL || input->mNumberBuffers == 0) {
        atomic_fetch_add_explicit(&tap->empty_calls, 1, memory_order_relaxed);
        return noErr;
    }
    atomic_store_explicit(&tap->last_buffers, input->mNumberBuffers, memory_order_relaxed);
    // Tap streams follow any input streams of the clock sub-device; disabled streams have no
    // data, so take the last buffer that has some.
    const AudioBuffer *buffer = NULL;
    for (UInt32 i = input->mNumberBuffers; i > 0; i--) {
        if (input->mBuffers[i - 1].mData != NULL) {
            buffer = &input->mBuffers[i - 1];
            break;
        }
    }
    if (buffer == NULL) {
        atomic_fetch_add_explicit(&tap->empty_calls, 1, memory_order_relaxed);
        return noErr;
    }
    uint32_t channels = buffer->mNumberChannels ? buffer->mNumberChannels : 1;
    uint32_t frames = buffer->mDataByteSize / (uint32_t)(sizeof(float) * channels);
    uint64_t age_ns = 0;
    if (input_time && now && (input_time->mFlags & kAudioTimeStampHostTimeValid) &&
        (now->mFlags & kAudioTimeStampHostTimeValid) && now->mHostTime > input_time->mHostTime) {
        age_ns = AudioConvertHostTimeToNanos(now->mHostTime - input_time->mHostTime);
    }
    tap->callback(tap->context, (const float *)buffer->mData, frames * channels, age_ns);
    return noErr;
}

static OSStatus read_property(AudioObjectID object, AudioObjectPropertySelector selector, void *data,
                              UInt32 size, const void *qualifier, UInt32 qualifier_size) {
    AudioObjectPropertyAddress address = {selector, kAudioObjectPropertyScopeGlobal,
                                          kAudioObjectPropertyElementMain};
    return AudioObjectGetPropertyData(object, &address, qualifier_size, qualifier, &size, data);
}

static OSStatus on_change(AudioObjectID object, UInt32 count, const AudioObjectPropertyAddress *addresses,
                          void *client) {
    (void)object; (void)count; (void)addresses;
    atomic_store(&((echonote_tap *)client)->changed, 1);
    return noErr;
}

static const AudioObjectPropertyAddress DEFAULT_OUTPUT = {
    kAudioHardwarePropertyDefaultOutputDevice, kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMain};
static const AudioObjectPropertyAddress TAP_FORMAT = {kAudioTapPropertyFormat, kAudioObjectPropertyScopeGlobal,
                                                      kAudioObjectPropertyElementMain};

void echonote_tap_stop(echonote_tap *tap);

static void debug_aggregate(const echonote_tap *tap, CATapDescription *description, NSString *output_uid) {
    if (getenv("ECHONOTE_TAP_DEBUG") == NULL) {
        return;
    }
    CFStringRef tap_uid = NULL;
    read_property(tap->tap_id, kAudioTapPropertyUID, &tap_uid, sizeof(tap_uid), NULL, 0);
    AudioObjectPropertyAddress streams = {kAudioDevicePropertyStreams, kAudioObjectPropertyScopeInput,
                                          kAudioObjectPropertyElementMain};
    UInt32 streams_size = 0;
    AudioObjectGetPropertyDataSize(tap->aggregate_id, &streams, 0, NULL, &streams_size);
    fprintf(stderr, "tap: description uuid=%s tap uid=%s output=%s input_streams=%u\n",
            description.UUID.UUIDString.UTF8String,
            tap_uid ? ((__bridge NSString *)tap_uid).UTF8String : "?", output_uid.UTF8String,
            (unsigned)(streams_size / sizeof(AudioStreamID)));
    if (tap_uid) {
        CFRelease(tap_uid);
    }
}

// Create the tap and its aggregate device without starting capture. Returns NULL and writes an
// OSStatus to *status on failure.
echonote_tap *echonote_tap_open(OSStatus *status) {
    @autoreleasepool {
        echonote_tap *tap = calloc(1, sizeof(echonote_tap));

        // Exclude our own output so playback inside EchoNote is never captured.
        NSMutableArray<NSNumber *> *excluded = [NSMutableArray array];
        pid_t pid = getpid();
        AudioObjectID self_object = kAudioObjectUnknown;
        if (read_property(kAudioObjectSystemObject, kAudioHardwarePropertyTranslatePIDToProcessObject,
                          &self_object, sizeof(self_object), &pid, sizeof(pid)) == noErr &&
            self_object != kAudioObjectUnknown) {
            [excluded addObject:@(self_object)];
        }

        CATapDescription *description = [[CATapDescription alloc] initMonoGlobalTapButExcludeProcesses:excluded];
        description.name = @"EchoNote system audio";
        description.privateTap = YES;
        description.muteBehavior = CATapUnmuted;

        *status = AudioHardwareCreateProcessTap(description, &tap->tap_id);
        if (*status != noErr) {
            free(tap);
            return NULL;
        }

        AudioStreamBasicDescription asbd = {0};
        *status = read_property(tap->tap_id, kAudioTapPropertyFormat, &asbd, sizeof(asbd), NULL, 0);
        if (*status != noErr) {
            echonote_tap_stop(tap);
            return NULL;
        }
        tap->format = (echonote_tap_format){asbd.mSampleRate, asbd.mChannelsPerFrame, asbd.mFormatID,
                                            asbd.mFormatFlags, asbd.mBitsPerChannel};

        // A tap-only aggregate has no clock and never runs its IOProc, so the current output
        // device joins as the main sub-device and drives the timing.
        AudioObjectID output = kAudioObjectUnknown;
        CFStringRef output_uid = NULL;
        *status = read_property(kAudioObjectSystemObject, kAudioHardwarePropertyDefaultOutputDevice, &output,
                                sizeof(output), NULL, 0);
        if (*status == noErr) {
            *status = read_property(output, kAudioDevicePropertyDeviceUID, &output_uid, sizeof(output_uid), NULL, 0);
        }
        if (*status != noErr || output_uid == NULL) {
            echonote_tap_stop(tap);
            return NULL;
        }
        NSString *output_uid_string = (__bridge_transfer NSString *)output_uid;

        NSDictionary *aggregate = @{
            @kAudioAggregateDeviceNameKey : @"EchoNote Tap",
            @kAudioAggregateDeviceUIDKey : [[NSUUID UUID] UUIDString],
            @kAudioAggregateDeviceMainSubDeviceKey : output_uid_string,
            @kAudioAggregateDeviceIsPrivateKey : @YES,
            @kAudioAggregateDeviceIsStackedKey : @NO,
            @kAudioAggregateDeviceTapAutoStartKey : @YES,
            @kAudioAggregateDeviceSubDeviceListKey : @[ @{@kAudioSubDeviceUIDKey : output_uid_string} ],
            @kAudioAggregateDeviceTapListKey : @[ @{
                @kAudioSubTapUIDKey : description.UUID.UUIDString,
                @kAudioSubTapDriftCompensationKey : @YES,
            } ],
        };
        *status = AudioHardwareCreateAggregateDevice((__bridge CFDictionaryRef)aggregate, &tap->aggregate_id);
        if (*status != noErr) {
            echonote_tap_stop(tap);
            return NULL;
        }
        if (AudioObjectAddPropertyListener(kAudioObjectSystemObject, &DEFAULT_OUTPUT, on_change, tap) == noErr &&
            AudioObjectAddPropertyListener(tap->tap_id, &TAP_FORMAT, on_change, tap) == noErr) {
            tap->listening = 1;
        }
        debug_aggregate(tap, description, output_uid_string);
        return tap;
    }
}

// Enable only the tap's input stream: the sub-device's own inputs (a headset mic) stay closed,
// so Bluetooth headsets are not switched to their low-quality call profile.
static void use_tap_stream_only(echonote_tap *tap) {
    AudioObjectPropertyAddress usage_address = {kAudioDevicePropertyIOProcStreamUsage,
                                                kAudioObjectPropertyScopeInput, kAudioObjectPropertyElementMain};
    UInt32 size = 0;
    if (AudioObjectGetPropertyDataSize(tap->aggregate_id, &usage_address, 0, NULL, &size) != noErr ||
        size < offsetof(AudioHardwareIOProcStreamUsage, mStreamIsOn)) {
        return;
    }
    AudioHardwareIOProcStreamUsage *usage = calloc(1, size);
    usage->mIOProc = (void *)tap->io_proc;
    if (AudioObjectGetPropertyData(tap->aggregate_id, &usage_address, 0, NULL, &size, usage) == noErr &&
        usage->mNumberStreams > 1) {
        // The tap stream comes after the sub-device's input streams.
        for (UInt32 i = 0; i < usage->mNumberStreams; i++) {
            usage->mStreamIsOn[i] = (i == usage->mNumberStreams - 1);
        }
        AudioObjectSetPropertyData(tap->aggregate_id, &usage_address, 0, NULL, size, usage);
    }
    free(usage);
}

// Start delivering buffers to `callback`. Returns an OSStatus.
OSStatus echonote_tap_start(echonote_tap *tap, echonote_tap_callback callback, void *context) {
    tap->callback = callback;
    tap->context = context;
    OSStatus status = AudioDeviceCreateIOProcID(tap->aggregate_id, io_proc, tap, &tap->io_proc);
    if (status != noErr) {
        tap->io_proc = NULL;
        return status;
    }
    use_tap_stream_only(tap);
    return AudioDeviceStart(tap->aggregate_id, tap->io_proc);
}

echonote_tap_format echonote_tap_get_format(const echonote_tap *tap) {
    return tap->format;
}

int echonote_tap_changed(echonote_tap *tap) {
    return atomic_load(&tap->changed);
}

void echonote_tap_stop(echonote_tap *tap) {
    if (tap == NULL) {
        return;
    }
    if (getenv("ECHONOTE_TAP_DEBUG") != NULL) {
        fprintf(stderr, "tap: io_calls=%llu empty_calls=%llu last_buffers=%u\n",
                (unsigned long long)atomic_load(&tap->io_calls), (unsigned long long)atomic_load(&tap->empty_calls),
                (unsigned)atomic_load(&tap->last_buffers));
    }
    if (tap->listening) {
        AudioObjectRemovePropertyListener(kAudioObjectSystemObject, &DEFAULT_OUTPUT, on_change, tap);
        AudioObjectRemovePropertyListener(tap->tap_id, &TAP_FORMAT, on_change, tap);
    }
    if (tap->aggregate_id != kAudioObjectUnknown && tap->io_proc != NULL) {
        AudioDeviceStop(tap->aggregate_id, tap->io_proc);
        AudioDeviceDestroyIOProcID(tap->aggregate_id, tap->io_proc);
    }
    if (tap->aggregate_id != kAudioObjectUnknown) {
        AudioHardwareDestroyAggregateDevice(tap->aggregate_id);
    }
    if (tap->tap_id != kAudioObjectUnknown) {
        AudioHardwareDestroyProcessTap(tap->tap_id);
    }
    free(tap);
}
