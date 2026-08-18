//! Synthesizing input and delivering it to one process.
//!
//! Everything here posts with `CGEventPostToPid`, never `CGEventPost`. The
//! difference is the entire product: `CGEventPost` puts the event on the global
//! HID stream, which moves the real cursor, steals focus, and lands wherever the
//! operator happens to be typing. `CGEventPostToPid` delivers to one process and
//! leaves the session alone. That is G3.
//!
//! Events come from a private `CGEventSource` so synthetic modifier flags never
//! mix with the operator's real keyboard state — a stuck synthetic Command key
//! on someone's actual machine is a very bad afternoon.

use crate::sys::*;
use anyhow::{bail, Result};
use core_foundation_sys::base::CFRelease;
use std::collections::HashMap;
use std::ffi::c_void;

/// Virtual keycode table for xdotool-style keysyms, ANSI layout.
///
/// Keysyms rather than raw keycodes because an agent should say `Return`, not
/// `36`, and because the name is stable across the places this vocabulary is
/// written down.
fn keysym_table() -> HashMap<&'static str, u16> {
    HashMap::from([
        ("a", 0), ("s", 1), ("d", 2), ("f", 3), ("h", 4), ("g", 5), ("z", 6), ("x", 7),
        ("c", 8), ("v", 9), ("b", 11), ("q", 12), ("w", 13), ("e", 14), ("r", 15), ("y", 16),
        ("t", 17), ("1", 18), ("2", 19), ("3", 20), ("4", 21), ("6", 22), ("5", 23),
        ("equal", 24), ("9", 25), ("7", 26), ("minus", 27), ("8", 28), ("0", 29),
        ("bracketright", 30), ("o", 31), ("u", 32), ("bracketleft", 33), ("i", 34), ("p", 35),
        ("Return", 36), ("l", 37), ("j", 38), ("apostrophe", 39), ("k", 40), ("semicolon", 41),
        ("backslash", 42), ("comma", 43), ("slash", 44), ("n", 45), ("m", 46), ("period", 47),
        ("Tab", 48), ("space", 49), ("grave", 50), ("BackSpace", 51), ("Escape", 53),
        ("F5", 96), ("F6", 97), ("F7", 98), ("F3", 99), ("F8", 100), ("F9", 101), ("F11", 103),
        ("F10", 109), ("F12", 111), ("Home", 115), ("Page_Up", 116), ("Delete", 117),
        ("F4", 118), ("End", 119), ("F2", 120), ("Page_Down", 121), ("F1", 122),
        ("Left", 123), ("Right", 124), ("Down", 125), ("Up", 126),
        // Aliases people actually type.
        ("Enter", 36), ("KP_Enter", 36), ("Esc", 53), ("Backspace", 51), ("Del", 117),
        ("PageUp", 116), ("PageDown", 121),
    ])
}

fn modifier_mask(name: &str) -> Option<u64> {
    match name.to_ascii_lowercase().as_str() {
        "shift" => Some(kCGEventFlagMaskShift),
        "ctrl" | "control" => Some(kCGEventFlagMaskControl),
        "alt" | "option" | "opt" => Some(kCGEventFlagMaskAlternate),
        "cmd" | "command" | "super" | "meta" => Some(kCGEventFlagMaskCommand),
        _ => None,
    }
}

/// Parse `cmd+shift+a` into flags and a keycode.
pub fn parse_keysym(spec: &str) -> Result<(u64, u16)> {
    let table = keysym_table();
    let mut flags = 0u64;
    let parts: Vec<&str> = spec.split('+').filter(|part| !part.is_empty()).collect();
    let Some((key, modifiers)) = parts.split_last() else {
        bail!("empty key specification");
    };
    for modifier in modifiers {
        match modifier_mask(modifier) {
            Some(mask) => flags |= mask,
            None => bail!("`{modifier}` is not a modifier; use shift, ctrl, alt, or cmd"),
        }
    }
    // Case matters for named keys (`Return`) but not for letters (`A` vs `a`),
    // and an agent that sends `RETURN` should not get a cryptic failure.
    let code = table
        .get(key)
        .or_else(|| table.get(key.to_ascii_lowercase().as_str()))
        .or_else(|| {
            table.iter().find_map(|(name, code)| {
                name.eq_ignore_ascii_case(key).then_some(code)
            })
        })
        .copied();
    match code {
        Some(code) => Ok((flags, code)),
        None => bail!("`{key}` is not a known key name"),
    }
}

/// A `CGEventRef` released on drop.
struct Event(CGEventRef);

impl Drop for Event {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0 as *const c_void) }
        }
    }
}

struct Source(CGEventSourceRef);

impl Drop for Source {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0 as *const c_void) }
        }
    }
}

#[cfg(target_os = "macos")]
fn source() -> Result<Source> {
    let raw = unsafe { CGEventSourceCreate(kCGEventSourceStatePrivate) };
    if raw.is_null() {
        bail!("could not create a private event source");
    }
    Ok(Source(raw))
}

/// Click at a point, delivered only to `pid`.
#[cfg(target_os = "macos")]
pub fn click(pid: i32, x: f64, y: f64, button: &str, count: u32) -> Result<()> {
    let (down, up, code) = match button {
        "right" => (kCGEventRightMouseDown, kCGEventRightMouseUp, kCGMouseButtonRight),
        "middle" => (kCGEventOtherMouseDown, kCGEventOtherMouseUp, kCGMouseButtonCenter),
        _ => (kCGEventLeftMouseDown, kCGEventLeftMouseUp, kCGMouseButtonLeft),
    };
    let source = source()?;
    let point = CGPoint { x, y };
    for click_index in 1..=count {
        for phase in [down, up] {
            let event = Event(unsafe {
                CGEventCreateMouseEvent(source.0, phase, point, code)
            });
            if event.0.is_null() {
                bail!("could not synthesize a mouse event");
            }
            unsafe {
                // Without an explicit click state a double-click is two
                // unrelated single clicks and never opens anything.
                CGEventSetIntegerValueField(
                    event.0,
                    kCGMouseEventClickState,
                    click_index as i64,
                );
                CGEventPostToPid(pid, event.0);
            }
        }
    }
    Ok(())
}

/// Type literal text. Uses the Unicode string field rather than per-character
/// keycodes, so it is layout-independent and handles anything the agent sends.
#[cfg(target_os = "macos")]
pub fn type_text(pid: i32, text: &str) -> Result<()> {
    let source = source()?;
    // Chunked: the Unicode string field is bounded, and a long paste sent as
    // one event is silently truncated rather than refused.
    for chunk in chunks(text, 16) {
        let utf16: Vec<u16> = chunk.encode_utf16().collect();
        for down in [true, false] {
            let event = Event(unsafe { CGEventCreateKeyboardEvent(source.0, 0, down) });
            if event.0.is_null() {
                bail!("could not synthesize a keyboard event");
            }
            unsafe {
                CGEventKeyboardSetUnicodeString(
                    event.0,
                    utf16.len() as libc::c_ulong,
                    utf16.as_ptr(),
                );
                CGEventPostToPid(pid, event.0);
            }
        }
    }
    Ok(())
}

/// Press one key, with modifiers, scoped to `pid`.
///
/// Because delivery is per-process, this cannot invoke a global shortcut. That
/// is a feature, not a limitation: it is what keeps the operator's session
/// intact while an agent works.
#[cfg(target_os = "macos")]
pub fn press_key(pid: i32, spec: &str) -> Result<()> {
    let (flags, code) = parse_keysym(spec)?;
    let source = source()?;
    for down in [true, false] {
        let event = Event(unsafe { CGEventCreateKeyboardEvent(source.0, code, down) });
        if event.0.is_null() {
            bail!("could not synthesize a keyboard event");
        }
        unsafe {
            if flags != 0 {
                CGEventSetFlags(event.0, flags);
            }
            CGEventPostToPid(pid, event.0);
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn scroll(pid: i32, direction: &str, pages: f64) -> Result<()> {
    // One "page" in line units. Chosen to match what a trackpad page-scroll
    // does in a typical list rather than any exact pixel count.
    const LINES_PER_PAGE: f64 = 24.0;
    let magnitude = (pages * LINES_PER_PAGE).round() as i32;
    let amount = match direction {
        "up" => magnitude,
        "down" => -magnitude,
        "left" => magnitude,
        "right" => -magnitude,
        other => bail!("`{other}` is not a scroll direction"),
    };
    let horizontal = matches!(direction, "left" | "right");
    let source = source()?;
    let event = Event(unsafe {
        if horizontal {
            CGEventCreateScrollWheelEvent(source.0, kCGScrollEventUnitLine, 2, 0, amount)
        } else {
            CGEventCreateScrollWheelEvent(source.0, kCGScrollEventUnitLine, 1, amount)
        }
    });
    if event.0.is_null() {
        bail!("could not synthesize a scroll event");
    }
    unsafe { CGEventPostToPid(pid, event.0) };
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn drag(pid: i32, from_x: f64, from_y: f64, to_x: f64, to_y: f64) -> Result<()> {
    let source = source()?;
    let steps = 12;
    let post = |kind: u32, x: f64, y: f64| -> Result<()> {
        let event = Event(unsafe {
            CGEventCreateMouseEvent(source.0, kind, CGPoint { x, y }, kCGMouseButtonLeft)
        });
        if event.0.is_null() {
            bail!("could not synthesize a drag event");
        }
        unsafe { CGEventPostToPid(pid, event.0) };
        Ok(())
    };
    post(kCGEventLeftMouseDown, from_x, from_y)?;
    // Interpolated rather than a single jump: apps that track movement to
    // decide what is being dragged see nothing at all from one teleport.
    for step in 1..steps {
        let ratio = step as f64 / steps as f64;
        post(
            kCGEventLeftMouseDragged,
            from_x + (to_x - from_x) * ratio,
            from_y + (to_y - from_y) * ratio,
        )?;
    }
    post(kCGEventLeftMouseDragged, to_x, to_y)?;
    post(kCGEventLeftMouseUp, to_x, to_y)?;
    Ok(())
}

fn chunks(text: &str, size: usize) -> Vec<String> {
    let characters: Vec<char> = text.chars().collect();
    characters
        .chunks(size)
        .map(|chunk| chunk.iter().collect())
        .collect()
}

#[cfg(not(target_os = "macos"))]
mod unsupported {
    use anyhow::{bail, Result};
    pub fn click(_: i32, _: f64, _: f64, _: &str, _: u32) -> Result<()> {
        bail!("Computer Use is macOS only")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_keys_and_modifier_combinations_parse() {
        assert_eq!(parse_keysym("Return").unwrap(), (0, 36));
        assert_eq!(parse_keysym("space").unwrap(), (0, 49));
        let (flags, code) = parse_keysym("cmd+shift+a").unwrap();
        assert_eq!(code, 0);
        assert_eq!(flags, kCGEventFlagMaskCommand | kCGEventFlagMaskShift);
    }

    /// An agent that sends `RETURN` or `enter` should not get a cryptic
    /// failure it cannot act on.
    #[test]
    fn key_names_are_forgiving_about_case_and_common_aliases() {
        assert_eq!(parse_keysym("RETURN").unwrap().1, 36);
        assert_eq!(parse_keysym("Enter").unwrap().1, 36);
        assert_eq!(parse_keysym("Esc").unwrap().1, 53);
        assert_eq!(parse_keysym("PageDown").unwrap().1, 121);
    }

    #[test]
    fn an_unknown_key_or_modifier_is_named_in_the_error() {
        let error = parse_keysym("Frobnicate").unwrap_err().to_string();
        assert!(error.contains("Frobnicate"), "{error}");
        let error = parse_keysym("hyper+a").unwrap_err().to_string();
        assert!(error.contains("hyper"), "{error}");
        assert!(parse_keysym("").is_err());
    }

    /// A long paste sent as one event is silently truncated by the OS, which
    /// looks like the agent typed half a sentence for no reason.
    #[test]
    fn long_text_is_chunked_rather_than_truncated() {
        let text = "x".repeat(100);
        let pieces = chunks(&text, 16);
        assert_eq!(pieces.len(), 7);
        assert_eq!(pieces.concat(), text);
    }

    #[test]
    fn chunking_splits_on_characters_not_bytes() {
        // Splitting a multi-byte character in half produces mojibake, or a
        // panic on a str boundary.
        let text = "日本語のテキストです";
        let pieces = chunks(text, 4);
        assert_eq!(pieces.concat(), text);
        assert_eq!(pieces[0].chars().count(), 4);
    }
}
