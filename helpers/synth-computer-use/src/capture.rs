//! Screenshots of one app's windows, encoded as PNG.
//!
//! Scoped to the target app's windows rather than the whole display. A
//! full-screen grab would put the operator's other windows — their mail, their
//! messages, whatever is open behind — into a durable trajectory that was only
//! ever meant to record one app.

use crate::sys::*;
use anyhow::{bail, Result};
use core_foundation::array::CFArray;
use core_foundation::base::{CFType, TCFType};
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_foundation_sys::base::CFRelease;
use std::ffi::c_void;

/// Window ids belonging to `pid`, on screen, excluding desktop furniture.
#[cfg(target_os = "macos")]
pub fn window_ids(pid: i32) -> Vec<CGWindowID> {
    unsafe {
        let raw = CGWindowListCopyWindowInfo(
            kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
            0,
        );
        if raw.is_null() {
            return Vec::new();
        }
        let windows = CFArray::<CFType>::wrap_under_create_rule(raw);
        let owner_key = CFString::new("kCGWindowOwnerPID");
        let number_key = CFString::new("kCGWindowNumber");
        let layer_key = CFString::new("kCGWindowLayer");
        let mut ids = Vec::new();
        for entry in windows.iter() {
            let dictionary = CFDictionary::<CFString, CFType>::wrap_under_get_rule(
                entry.as_CFTypeRef() as core_foundation_sys::dictionary::CFDictionaryRef,
            );
            let owner = dictionary
                .find(&owner_key)
                .and_then(|value| value.downcast::<CFNumber>())
                .and_then(|number| number.to_i64());
            if owner != Some(pid as i64) {
                continue;
            }
            // Layer 0 is a normal window. Anything else is a panel, a menu, or
            // a shadow, and capturing those produces confusing artefacts.
            let layer = dictionary
                .find(&layer_key)
                .and_then(|value| value.downcast::<CFNumber>())
                .and_then(|number| number.to_i64())
                .unwrap_or(0);
            if layer != 0 {
                continue;
            }
            if let Some(id) = dictionary
                .find(&number_key)
                .and_then(|value| value.downcast::<CFNumber>())
                .and_then(|number| number.to_i64())
            {
                ids.push(id as CGWindowID);
            }
        }
        ids
    }
}

#[cfg(not(target_os = "macos"))]
pub fn window_ids(_pid: i32) -> Vec<CGWindowID> {
    Vec::new()
}

/// PNG bytes of the app's frontmost normal window.
///
/// Returns `Ok(None)` rather than an error when the app has no on-screen window:
/// a menu-bar app with nothing open is a normal state, not a failure, and an
/// error there would abort an otherwise fine action.
#[cfg(target_os = "macos")]
pub fn capture_app(pid: i32) -> Result<Option<Vec<u8>>> {
    if !unsafe { CGPreflightScreenCaptureAccess() } {
        bail!("screen recording is not granted to this helper");
    }
    let Some(window) = window_ids(pid).into_iter().next() else {
        return Ok(None);
    };
    unsafe {
        let image = CGWindowListCreateImage(
            CG_RECT_NULL,
            kCGWindowListOptionIncludingWindow,
            window,
            kCGWindowImageBoundsIgnoreFraming | kCGWindowImageNominalResolution,
        );
        if image.is_null() {
            return Ok(None);
        }
        let image = Owned(image);
        Ok(Some(encode_png(image.0)?))
    }
}

#[cfg(not(target_os = "macos"))]
pub fn capture_app(_pid: i32) -> Result<Option<Vec<u8>>> {
    bail!("Computer Use is macOS only")
}

#[cfg(target_os = "macos")]
unsafe fn encode_png(image: CGImageRef) -> Result<Vec<u8>> {
    use core_foundation_sys::data::{
        CFDataCreateMutable, CFDataGetBytePtr, CFDataGetLength, CFMutableDataRef,
    };

    let data: CFMutableDataRef = CFDataCreateMutable(std::ptr::null(), 0);
    if data.is_null() {
        bail!("could not allocate a PNG buffer");
    }
    let data = Owned(data as *mut c_void);
    let png_type = CFString::new("public.png");
    let destination = CGImageDestinationCreateWithData(
        data.0,
        png_type.as_concrete_TypeRef(),
        1,
        std::ptr::null(),
    );
    if destination.is_null() {
        bail!("could not create a PNG encoder");
    }
    let destination = Owned(destination);
    CGImageDestinationAddImage(destination.0, image, std::ptr::null());
    if !CGImageDestinationFinalize(destination.0) {
        bail!("could not encode the screenshot as PNG");
    }
    let length = CFDataGetLength(data.0 as CFMutableDataRef) as usize;
    let bytes = CFDataGetBytePtr(data.0 as CFMutableDataRef);
    if bytes.is_null() || length == 0 {
        bail!("the PNG encoder produced no bytes");
    }
    Ok(std::slice::from_raw_parts(bytes, length).to_vec())
}

/// A CF object released on drop, so an early return does not leak an image
/// that may be several megabytes.
struct Owned(*mut c_void);

impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0 as *const c_void) }
        }
    }
}

