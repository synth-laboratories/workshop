//! Every macOS C entry point the helper uses, declared in one place.
//!
//! Hand-declared rather than pulled from a binding crate. These are stable C
//! APIs, the set is small, and having the signatures in one readable file is
//! worth more here than a dependency — this is the code that decides whether an
//! agent can press a key on someone's machine, and it should be possible to
//! audit it by reading it.
//!
//! `CGEventPostToPid` is the one that matters most: it delivers an event to a
//! single process without raising its window or moving the real cursor. That is
//! the whole mechanism behind G3.

#![allow(non_upper_case_globals, non_snake_case, dead_code)]

use core_foundation_sys::array::CFArrayRef;
use core_foundation_sys::base::{Boolean, CFTypeRef, OSStatus};
use core_foundation_sys::dictionary::CFDictionaryRef;
use core_foundation_sys::string::CFStringRef;
use std::os::raw::{c_int, c_void};

pub type AXUIElementRef = CFTypeRef;
pub type AXError = i32;
pub type CGEventRef = *mut c_void;
pub type CGEventSourceRef = *mut c_void;
pub type CGImageRef = *mut c_void;
pub type SecCodeRef = *mut c_void;
pub type SecRequirementRef = *mut c_void;
pub type CGWindowID = u32;
pub type CGKeyCode = u16;
pub type UniChar = u16;

pub const kAXErrorSuccess: AXError = 0;
pub const kAXErrorAttributeUnsupported: AXError = -25205;
pub const kAXErrorNoValue: AXError = -25212;
pub const kAXErrorAPIDisabled: AXError = -25211;
pub const kAXErrorCannotComplete: AXError = -25204;
pub const kAXErrorActionUnsupported: AXError = -25206;

/// `CGEventType`.
pub const kCGEventLeftMouseDown: u32 = 1;
pub const kCGEventLeftMouseUp: u32 = 2;
pub const kCGEventRightMouseDown: u32 = 3;
pub const kCGEventRightMouseUp: u32 = 4;
pub const kCGEventMouseMoved: u32 = 5;
pub const kCGEventLeftMouseDragged: u32 = 6;
pub const kCGEventKeyDown: u32 = 10;
pub const kCGEventKeyUp: u32 = 11;
pub const kCGEventOtherMouseDown: u32 = 25;
pub const kCGEventOtherMouseUp: u32 = 26;
pub const kCGEventScrollWheel: u32 = 22;

/// `CGMouseButton`.
pub const kCGMouseButtonLeft: u32 = 0;
pub const kCGMouseButtonRight: u32 = 1;
pub const kCGMouseButtonCenter: u32 = 2;

/// `CGEventField`. Click state must be set explicitly or a double-click is two
/// unrelated single clicks.
pub const kCGMouseEventClickState: u32 = 1;

/// `CGEventSourceStateID`. A private state keeps our synthetic modifier flags
/// out of the operator's real keyboard state.
pub const kCGEventSourceStatePrivate: i32 = -1;
pub const kCGEventSourceStateHIDSystemState: i32 = 1;

/// `CGEventFlags`.
pub const kCGEventFlagMaskShift: u64 = 0x0002_0000;
pub const kCGEventFlagMaskControl: u64 = 0x0004_0000;
pub const kCGEventFlagMaskAlternate: u64 = 0x0008_0000;
pub const kCGEventFlagMaskCommand: u64 = 0x0010_0000;

/// `CGWindowListOption` / `CGWindowImageOption`.
pub const kCGWindowListOptionIncludingWindow: u32 = 1 << 3;
pub const kCGWindowListOptionOnScreenOnly: u32 = 1 << 0;
pub const kCGWindowListExcludeDesktopElements: u32 = 1 << 4;
pub const kCGWindowImageBoundsIgnoreFraming: u32 = 1 << 0;
pub const kCGWindowImageNominalResolution: u32 = 1 << 4;

/// `SecCSFlags`.
pub const kSecCSDefaultFlags: u32 = 0;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CGPoint {
    pub x: f64,
    pub y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CGSize {
    pub width: f64,
    pub height: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CGRect {
    pub origin: CGPoint,
    pub size: CGSize,
}

/// `CGRectNull`, which asks `CGWindowListCreateImage` to fit the window.
pub const CG_RECT_NULL: CGRect = CGRect {
    origin: CGPoint {
        x: f64::INFINITY,
        y: f64::INFINITY,
    },
    size: CGSize {
        width: 0.0,
        height: 0.0,
    },
};

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    pub fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> Boolean;
    pub static kAXTrustedCheckOptionPrompt: CFStringRef;

    pub fn AXUIElementCreateApplication(pid: libc::pid_t) -> AXUIElementRef;
    pub fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    pub fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> AXError;
    pub fn AXUIElementSetAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: CFTypeRef,
    ) -> AXError;
    pub fn AXUIElementCopyAttributeNames(
        element: AXUIElementRef,
        names: *mut CFArrayRef,
    ) -> AXError;
    pub fn AXUIElementCopyActionNames(element: AXUIElementRef, names: *mut CFArrayRef) -> AXError;
    pub fn AXUIElementPerformAction(element: AXUIElementRef, action: CFStringRef) -> AXError;
    pub fn AXUIElementSetMessagingTimeout(element: AXUIElementRef, timeout: f32) -> AXError;
    pub fn AXValueGetValue(value: CFTypeRef, the_type: u32, out: *mut c_void) -> Boolean;
    pub fn AXValueCreate(the_type: u32, value: *const c_void) -> CFTypeRef;
}

/// `AXValueType`.
pub const kAXValueTypeCGPoint: u32 = 1;
pub const kAXValueTypeCGSize: u32 = 2;
pub const kAXValueTypeCGRect: u32 = 3;
pub const kAXValueTypeCFRange: u32 = 4;

/// `CFRange`, for selecting a span of text in an element.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CFRangeRaw {
    pub location: isize,
    pub length: isize,
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    pub fn CGEventSourceCreate(state: i32) -> CGEventSourceRef;
    pub fn CGEventCreateMouseEvent(
        source: CGEventSourceRef,
        mouse_type: u32,
        position: CGPoint,
        button: u32,
    ) -> CGEventRef;
    pub fn CGEventCreateKeyboardEvent(
        source: CGEventSourceRef,
        key: CGKeyCode,
        key_down: bool,
    ) -> CGEventRef;
    pub fn CGEventCreateScrollWheelEvent(
        source: CGEventSourceRef,
        units: u32,
        wheel_count: u32,
        wheel1: i32,
        ...
    ) -> CGEventRef;
    pub fn CGEventKeyboardSetUnicodeString(
        event: CGEventRef,
        length: libc::c_ulong,
        string: *const UniChar,
    );
    pub fn CGEventSetIntegerValueField(event: CGEventRef, field: u32, value: i64);
    pub fn CGEventSetFlags(event: CGEventRef, flags: u64);

    /// Deliver to one process. Does not raise the window, does not move the
    /// real cursor, does not change the frontmost app. G3 rests on this.
    pub fn CGEventPostToPid(pid: libc::pid_t, event: CGEventRef);

    pub fn CGPreflightScreenCaptureAccess() -> bool;
    pub fn CGRequestScreenCaptureAccess() -> bool;
    pub fn CGWindowListCreateImage(
        bounds: CGRect,
        option: u32,
        window: CGWindowID,
        image_option: u32,
    ) -> CGImageRef;
    pub fn CGWindowListCopyWindowInfo(option: u32, relative_to: CGWindowID) -> CFArrayRef;
}

/// `CGScrollEventUnit`.
pub const kCGScrollEventUnitPixel: u32 = 0;
pub const kCGScrollEventUnitLine: u32 = 1;

#[link(name = "ImageIO", kind = "framework")]
extern "C" {
    pub fn CGImageDestinationCreateWithData(
        data: *mut c_void,
        the_type: CFStringRef,
        count: usize,
        options: CFDictionaryRef,
    ) -> *mut c_void;
    pub fn CGImageDestinationAddImage(
        destination: *mut c_void,
        image: CGImageRef,
        properties: CFDictionaryRef,
    );
    pub fn CGImageDestinationFinalize(destination: *mut c_void) -> bool;
}

#[link(name = "Security", kind = "framework")]
extern "C" {
    pub fn SecCodeCopyGuestWithAttributes(
        host: SecCodeRef,
        attributes: CFDictionaryRef,
        flags: u32,
        guest: *mut SecCodeRef,
    ) -> OSStatus;
    pub fn SecCodeCheckValidity(
        code: SecCodeRef,
        flags: u32,
        requirement: SecRequirementRef,
    ) -> OSStatus;
    pub fn SecRequirementCreateWithString(
        text: CFStringRef,
        flags: u32,
        requirement: *mut SecRequirementRef,
    ) -> OSStatus;
    pub fn SecCodeCopySigningInformation(
        code: SecCodeRef,
        flags: u32,
        information: *mut CFDictionaryRef,
    ) -> OSStatus;
    pub static kSecGuestAttributePid: CFStringRef;
}

#[link(name = "CoreServices", kind = "framework")]
extern "C" {
    pub fn UCKeyTranslate(
        key_layout_ptr: *const c_void,
        virtual_key_code: u16,
        key_action: u16,
        modifier_key_state: u32,
        keyboard_type: u32,
        key_translate_options: u32,
        dead_key_state: *mut u32,
        max_string_length: usize,
        actual_string_length: *mut usize,
        unicode_string: *mut UniChar,
    ) -> c_int;
}

/// A convenience for the AX error codes the helper reports specially.
pub fn ax_error_message(code: AXError) -> &'static str {
    match code {
        kAXErrorSuccess => "success",
        kAXErrorAPIDisabled => {
            "accessibility is not granted to this helper (Privacy & Security → Accessibility)"
        }
        kAXErrorAttributeUnsupported => "the element does not expose that attribute",
        kAXErrorActionUnsupported => "the element does not expose that action",
        kAXErrorNoValue => "the attribute has no value",
        kAXErrorCannotComplete => "the app did not respond in time",
        _ => "the accessibility API refused the request",
    }
}
