#import <AppKit/AppKit.h>
#import <ApplicationServices/ApplicationServices.h>
#import <CoreGraphics/CoreGraphics.h>

static NSArray<NSDictionary<NSString *, NSData *> *> *g_clipboard_snapshot = nil;

static char *copy_utf8(NSString *value) {
    if (value == nil) {
        return NULL;
    }
    const char *text = [value UTF8String];
    return text == NULL ? NULL : strdup(text);
}

static void snapshot_clipboard(void) {
    NSPasteboard *pasteboard = [NSPasteboard generalPasteboard];
    NSMutableArray<NSDictionary<NSString *, NSData *> *> *items = [NSMutableArray array];
    for (NSPasteboardItem *item in pasteboard.pasteboardItems) {
        NSMutableDictionary<NSString *, NSData *> *values = [NSMutableDictionary dictionary];
        for (NSPasteboardType type in item.types) {
            NSData *data = [item dataForType:type];
            if (data != nil) {
                values[type] = data;
            }
        }
        if (values.count > 0) {
            [items addObject:values];
        }
    }
    g_clipboard_snapshot = [items copy];
}

static void restore_clipboard(void) {
    if (g_clipboard_snapshot == nil) {
        return;
    }
    NSPasteboard *pasteboard = [NSPasteboard generalPasteboard];
    [pasteboard clearContents];
    NSMutableArray<NSPasteboardItem *> *items = [NSMutableArray array];
    for (NSDictionary<NSString *, NSData *> *values in g_clipboard_snapshot) {
        NSPasteboardItem *item = [[NSPasteboardItem alloc] init];
        for (NSPasteboardType type in values) {
            [item setData:values[type] forType:type];
        }
        [items addObject:item];
    }
    if (items.count > 0) {
        [pasteboard writeObjects:items];
    }
    g_clipboard_snapshot = nil;
}

bool stools_ax_is_trusted(void) {
    return AXIsProcessTrusted();
}

void stools_request_accessibility(void) {
    NSDictionary *options = @{(__bridge NSString *)kAXTrustedCheckOptionPrompt: @YES};
    AXIsProcessTrustedWithOptions((__bridge CFDictionaryRef)options);
}

bool stools_screen_recording_allowed(void) {
    return CGPreflightScreenCaptureAccess();
}

void stools_request_screen_recording(void) {
    CGRequestScreenCaptureAccess();
}

char *stools_selected_text_ax(void) {
    AXUIElementRef system_wide = AXUIElementCreateSystemWide();
    if (system_wide == NULL) {
        return NULL;
    }
    CFTypeRef focused = NULL;
    AXError focused_error = AXUIElementCopyAttributeValue(
        system_wide,
        kAXFocusedUIElementAttribute,
        &focused
    );
    CFRelease(system_wide);
    if (focused_error != kAXErrorSuccess || focused == NULL) {
        return NULL;
    }

    CFTypeRef selected = NULL;
    AXError selected_error = AXUIElementCopyAttributeValue(
        (AXUIElementRef)focused,
        kAXSelectedTextAttribute,
        &selected
    );
    CFRelease(focused);
    if (selected_error != kAXErrorSuccess || selected == NULL) {
        return NULL;
    }

    NSString *text = nil;
    if (CFGetTypeID(selected) == CFStringGetTypeID()) {
        text = [(__bridge NSString *)selected copy];
    }
    CFRelease(selected);
    return copy_utf8(text);
}

char *stools_copy_selected_text(void) {
    NSPasteboard *pasteboard = [NSPasteboard generalPasteboard];
    NSInteger original_change_count = pasteboard.changeCount;
    snapshot_clipboard();
    [pasteboard clearContents];

    CGEventSourceRef source = CGEventSourceCreate(kCGEventSourceStateCombinedSessionState);
    CGEventRef key_down = CGEventCreateKeyboardEvent(source, (CGKeyCode)0x08, true);
    CGEventRef key_up = CGEventCreateKeyboardEvent(source, (CGKeyCode)0x08, false);
    CGEventSetFlags(key_down, kCGEventFlagMaskCommand);
    CGEventSetFlags(key_up, kCGEventFlagMaskCommand);
    CGEventPost(kCGHIDEventTap, key_down);
    CGEventPost(kCGHIDEventTap, key_up);
    if (key_down != NULL) CFRelease(key_down);
    if (key_up != NULL) CFRelease(key_up);
    if (source != NULL) CFRelease(source);

    NSString *text = nil;
    for (NSInteger attempt = 0; attempt < 40; attempt++) {
        usleep(20000);
        if (pasteboard.changeCount != original_change_count) {
            text = [pasteboard stringForType:NSPasteboardTypeString];
            if (text.length > 0) {
                break;
            }
        }
    }
    restore_clipboard();
    return copy_utf8(text);
}

void stools_free_string(char *value) {
    if (value != NULL) {
        free(value);
    }
}

uint32_t stools_frontmost_app_pid(void) {
    NSRunningApplication *application = [[NSWorkspace sharedWorkspace] frontmostApplication];
    return application == nil ? 0 : application.processIdentifier;
}

void stools_activate_app_pid(uint32_t pid) {
    if (pid == 0) {
        return;
    }
    NSRunningApplication *application =
        [NSRunningApplication runningApplicationWithProcessIdentifier:(pid_t)pid];
    [application activateWithOptions:0];
}

char *stools_window_rects_json(
    double display_x,
    double display_y,
    double display_width,
    double display_height
) {
    CFArrayRef raw_windows = CGWindowListCopyWindowInfo(
        kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
        kCGNullWindowID
    );
    if (raw_windows == NULL) {
        return strdup("[]");
    }

    pid_t own_pid = [[NSProcessInfo processInfo] processIdentifier];
    NSMutableArray<NSDictionary *> *rectangles = [NSMutableArray array];
    CFIndex count = CFArrayGetCount(raw_windows);
    for (CFIndex index = 0; index < count; index++) {
        CFDictionaryRef info = CFArrayGetValueAtIndex(raw_windows, index);
        if (info == NULL) {
            continue;
        }

        CFNumberRef owner_pid_value = CFDictionaryGetValue(info, kCGWindowOwnerPID);
        pid_t owner_pid = 0;
        if (owner_pid_value == NULL ||
            !CFNumberGetValue(owner_pid_value, kCFNumberIntType, &owner_pid) ||
            owner_pid == own_pid) {
            continue;
        }

        CFNumberRef layer_value = CFDictionaryGetValue(info, kCGWindowLayer);
        int layer = 0;
        if (layer_value != NULL &&
            CFNumberGetValue(layer_value, kCFNumberIntType, &layer) &&
            layer < 0) {
            continue;
        }

        CFNumberRef alpha_value = CFDictionaryGetValue(info, kCGWindowAlpha);
        double alpha = 1.0;
        if (alpha_value != NULL &&
            (!CFNumberGetValue(alpha_value, kCFNumberDoubleType, &alpha) || alpha <= 0.0)) {
            continue;
        }

        NSString *owner_name =
            (__bridge NSString *)CFDictionaryGetValue(info, kCGWindowOwnerName);
        if ([owner_name isEqualToString:@"Dock"] ||
            [owner_name isEqualToString:@"Window Server"] ||
            [owner_name isEqualToString:@"SystemUIServer"] ||
            [owner_name isEqualToString:@"Control Center"] ||
            [owner_name isEqualToString:@"Notification Center"]) {
            continue;
        }

        CFDictionaryRef bounds_value = CFDictionaryGetValue(info, kCGWindowBounds);
        CGRect bounds = CGRectZero;
        if (bounds_value == NULL ||
            !CGRectMakeWithDictionaryRepresentation(bounds_value, &bounds)) {
            continue;
        }

        double left = MAX(bounds.origin.x, display_x);
        double top = MAX(bounds.origin.y, display_y);
        double right = MIN(bounds.origin.x + bounds.size.width, display_x + display_width);
        double bottom = MIN(bounds.origin.y + bounds.size.height, display_y + display_height);
        double width = right - left;
        double height = bottom - top;
        if (width < 8.0 || height < 8.0) {
            continue;
        }

        [rectangles addObject:@{
            @"x": @(left - display_x),
            @"y": @(top - display_y),
            @"width": @(width),
            @"height": @(height)
        }];
    }
    CFRelease(raw_windows);

    NSData *json = [NSJSONSerialization dataWithJSONObject:rectangles options:0 error:nil];
    if (json == nil) {
        return strdup("[]");
    }
    NSString *text = [[NSString alloc] initWithData:json encoding:NSUTF8StringEncoding];
    return copy_utf8(text);
}
