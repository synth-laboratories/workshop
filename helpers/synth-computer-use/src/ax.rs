//! Reading an app's accessibility tree and assigning element indexes.
//!
//! Indexes are assigned in depth-first order within a single read. They are
//! meaningful only until the UI changes, which is why Desktop refuses to reuse
//! one across an action boundary rather than trusting the agent to remember.
//!
//! Secure fields are redacted here, at the source, where the role is known.
//! Redacting later would mean a password had already crossed a process boundary
//! in the clear.

use crate::sys::*;
use anyhow::{bail, Result};
use core_foundation::array::CFArray;
use core_foundation::base::{CFType, TCFType};
use core_foundation::string::CFString;
use core_foundation_sys::base::{CFGetTypeID, CFRelease, CFRetain, CFTypeRef};
use core_foundation_sys::string::{CFStringGetTypeID, CFStringRef};
use serde::Serialize;
use std::ffi::c_void;

/// Depth beyond which a tree is almost certainly a scrolling list of identical
/// rows rather than more structure worth reading.
pub const MAX_DEPTH: usize = 24;
/// Ceiling on elements per read. A mail client with a large mailbox can expose
/// tens of thousands; a truncated tree the agent knows is truncated beats a
/// read that takes a minute.
pub const MAX_ELEMENTS: usize = 2_000;

pub const REDACTED: &str = "[redacted]";

const SECURE_ROLES: &[&str] = &["AXSecureTextField", "AXSecureTextArea"];

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Element {
    pub index: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_index: Option<u64>,
    pub depth: usize,
    pub role: String,
    /// Best available human label: title, then description, then value.
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Actions the element actually exposes. The agent must pick from these
    /// rather than guessing a name.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame: Option<[f64; 4]>,
    #[serde(skip)]
    handle: ElementHandle,
}

impl Element {
    pub fn handle(&self) -> AXUIElementRef {
        self.handle.0
    }
}

/// A retained `AXUIElementRef`. Without this the tree would hold dangling
/// references the moment the read returns.
#[derive(Debug)]
pub struct ElementHandle(pub AXUIElementRef);

impl Clone for ElementHandle {
    fn clone(&self) -> Self {
        if !self.0.is_null() {
            unsafe { CFRetain(self.0) };
        }
        Self(self.0)
    }
}

impl Drop for ElementHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) };
        }
    }
}

impl PartialEq for ElementHandle {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppTree {
    pub pid: i32,
    pub elements: Vec<Element>,
    /// True when the read hit `MAX_ELEMENTS`. Reported so an agent that cannot
    /// find its target knows the tree is incomplete rather than concluding the
    /// control does not exist.
    pub truncated: bool,
}

impl AppTree {
    #[cfg(test)]
    pub fn test_tree(rows: &[(u64, &str, &str)]) -> Self {
        Self {
            pid: 1,
            elements: rows
                .iter()
                .map(|(index, role, label)| Element {
                    index: *index,
                    parent_index: None,
                    depth: 0,
                    role: (*role).to_owned(),
                    label: (*label).to_owned(),
                    value: None,
                    actions: Vec::new(),
                    enabled: true,
                    frame: None,
                    handle: ElementHandle(std::ptr::null_mut()),
                })
                .collect(),
            truncated: false,
        }
    }

    pub fn get(&self, index: u64) -> Option<&Element> {
        self.elements.iter().find(|element| element.index == index)
    }

    /// One line per element. This is what the agent reads, so it is terse and
    /// stable: index, role, label, then only what is unusual.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for element in &self.elements {
            out.push_str(&format!("[{}] {}", element.index, element.role));
            if !element.label.is_empty() {
                out.push_str(&format!(" \"{}\"", element.label));
            }
            if let Some(value) = &element.value {
                if value != &element.label {
                    out.push_str(&format!(" value=\"{value}\""));
                }
            }
            if !element.enabled {
                out.push_str(" disabled");
            }
            if !element.actions.is_empty() {
                out.push_str(&format!(" actions=[{}]", element.actions.join(",")));
            }
            out.push('\n');
        }
        if self.truncated {
            out.push_str(&format!(
                "… tree truncated at {MAX_ELEMENTS} elements; narrow the target app or scroll\n"
            ));
        }
        out
    }

    /// Lines present now and not before. Diffing is the default because a full
    /// tree on every read is mostly unchanged text, and the unchanged part is
    /// what pushes an agent's context over its limit.
    pub fn diff_from(&self, previous: &str) -> String {
        let seen: std::collections::HashSet<&str> = previous.lines().collect();
        let mut out = String::new();
        for line in self.render().lines() {
            if !seen.contains(line) {
                out.push_str(line);
                out.push('\n');
            }
        }
        if out.is_empty() {
            out.push_str("(no change)\n");
        }
        out
    }
}

/// Read one app's tree. `AXUIElementSetMessagingTimeout` bounds the wait: a
/// hung app must not hang the helper, because a hung helper looks to Desktop
/// like a helper that needs killing.
#[cfg(target_os = "macos")]
pub fn read_tree(pid: i32) -> Result<AppTree> {
    unsafe {
        let app = AXUIElementCreateApplication(pid);
        if app.is_null() {
            bail!("could not attach to process {pid}");
        }
        let app = ElementHandle(app);
        AXUIElementSetMessagingTimeout(app.0, 2.0);

        let mut elements = Vec::new();
        let mut next_index = 0u64;
        let mut truncated = false;
        walk(
            app.0,
            0,
            None,
            &mut next_index,
            &mut elements,
            &mut truncated,
        );
        Ok(AppTree {
            pid,
            elements,
            truncated,
        })
    }
}

#[cfg(not(target_os = "macos"))]
pub fn read_tree(_pid: i32) -> Result<AppTree> {
    bail!("Computer Use is macOS only")
}

#[cfg(target_os = "macos")]
unsafe fn walk(
    element: AXUIElementRef,
    depth: usize,
    parent_index: Option<u64>,
    next_index: &mut u64,
    out: &mut Vec<Element>,
    truncated: &mut bool,
) {
    if depth > MAX_DEPTH || out.len() >= MAX_ELEMENTS {
        *truncated = *truncated || out.len() >= MAX_ELEMENTS;
        return;
    }

    let role = string_attribute(element, "AXRole").unwrap_or_default();
    let is_secure = SECURE_ROLES.iter().any(|secure| role == *secure);
    let title = string_attribute(element, "AXTitle");
    let description = string_attribute(element, "AXDescription");
    let raw_value = if is_secure {
        Some(REDACTED.to_owned())
    } else {
        string_attribute(element, "AXValue")
    };
    let label = title
        .clone()
        .or_else(|| description.clone())
        .or_else(|| {
            // A secure field's redacted value is not a label.
            if is_secure {
                None
            } else {
                raw_value.clone()
            }
        })
        .unwrap_or_default();

    // Skip pure structural containers with nothing to say, so the rendered tree
    // stays readable. They are still walked for their children.
    let interesting = !role.is_empty() && (!label.is_empty() || !actions(element).is_empty());
    let mut descendant_parent = parent_index;
    if interesting {
        let index = *next_index;
        *next_index += 1;
        descendant_parent = Some(index);
        out.push(Element {
            index,
            parent_index,
            depth,
            role: role.clone(),
            label: truncate(&label, 200),
            value: raw_value.map(|value| truncate(&value, 500)),
            actions: actions(element),
            enabled: bool_attribute(element, "AXEnabled").unwrap_or(true),
            frame: frame(element),
            handle: ElementHandle({
                CFRetain(element);
                element
            }),
        });
    }

    if let Some(children) = array_attribute(element, "AXChildren") {
        for child in children.iter() {
            if out.len() >= MAX_ELEMENTS {
                *truncated = true;
                return;
            }
            walk(
                child.as_CFTypeRef(),
                depth + 1,
                descendant_parent,
                next_index,
                out,
                truncated,
            );
        }
    }
}

#[cfg(target_os = "macos")]
pub unsafe fn copy_attribute(element: AXUIElementRef, name: &str) -> Option<CFType> {
    let key = CFString::new(name);
    let mut value: CFTypeRef = std::ptr::null();
    let status = AXUIElementCopyAttributeValue(element, key.as_concrete_TypeRef(), &mut value);
    if status != kAXErrorSuccess || value.is_null() {
        return None;
    }
    Some(CFType::wrap_under_create_rule(value))
}

#[cfg(target_os = "macos")]
unsafe fn string_attribute(element: AXUIElementRef, name: &str) -> Option<String> {
    let value = copy_attribute(element, name)?;
    if CFGetTypeID(value.as_CFTypeRef()) != CFStringGetTypeID() {
        return None;
    }
    let text = CFString::wrap_under_get_rule(value.as_CFTypeRef() as CFStringRef).to_string();
    (!text.is_empty()).then_some(text)
}

#[cfg(target_os = "macos")]
unsafe fn bool_attribute(element: AXUIElementRef, name: &str) -> Option<bool> {
    let value = copy_attribute(element, name)?;
    if CFGetTypeID(value.as_CFTypeRef()) != core_foundation_sys::number::CFBooleanGetTypeID() {
        return None;
    }
    Some(core_foundation_sys::number::CFBooleanGetValue(
        value.as_CFTypeRef() as core_foundation_sys::number::CFBooleanRef,
    ))
}

#[cfg(target_os = "macos")]
unsafe fn array_attribute(element: AXUIElementRef, name: &str) -> Option<CFArray<CFType>> {
    let value = copy_attribute(element, name)?;
    if CFGetTypeID(value.as_CFTypeRef()) != core_foundation_sys::array::CFArrayGetTypeID() {
        return None;
    }
    Some(CFArray::<CFType>::wrap_under_get_rule(
        value.as_CFTypeRef() as core_foundation_sys::array::CFArrayRef
    ))
}

#[cfg(target_os = "macos")]
pub unsafe fn actions(element: AXUIElementRef) -> Vec<String> {
    let mut names: core_foundation_sys::array::CFArrayRef = std::ptr::null();
    if AXUIElementCopyActionNames(element, &mut names) != kAXErrorSuccess || names.is_null() {
        return Vec::new();
    }
    let array = CFArray::<CFType>::wrap_under_create_rule(names);
    array
        .iter()
        .filter_map(|item| {
            if CFGetTypeID(item.as_CFTypeRef()) == CFStringGetTypeID() {
                Some(CFString::wrap_under_get_rule(item.as_CFTypeRef() as CFStringRef).to_string())
            } else {
                None
            }
        })
        .collect()
}

#[cfg(target_os = "macos")]
unsafe fn frame(element: AXUIElementRef) -> Option<[f64; 4]> {
    let position = copy_attribute(element, "AXPosition")?;
    let size = copy_attribute(element, "AXSize")?;
    let mut point = CGPoint::default();
    let mut extent = CGSize::default();
    let got_point = AXValueGetValue(
        position.as_CFTypeRef(),
        kAXValueTypeCGPoint,
        &mut point as *mut _ as *mut c_void,
    ) != 0;
    let got_size = AXValueGetValue(
        size.as_CFTypeRef(),
        kAXValueTypeCGSize,
        &mut extent as *mut _ as *mut c_void,
    ) != 0;
    (got_point && got_size).then_some([point.x, point.y, extent.width, extent.height])
}

/// Set a value on an element.
#[cfg(target_os = "macos")]
pub fn set_value(element: AXUIElementRef, value: &str) -> Result<()> {
    unsafe {
        let key = CFString::new("AXValue");
        let text = CFString::new(value);
        let status = AXUIElementSetAttributeValue(
            element,
            key.as_concrete_TypeRef(),
            text.as_concrete_TypeRef() as CFTypeRef,
        );
        if status != kAXErrorSuccess {
            bail!("{}", ax_error_message(status));
        }
    }
    Ok(())
}

/// Perform a named action. The name must be one the element reported, which is
/// checked by the caller — guessing action names produces silent no-ops.
#[cfg(target_os = "macos")]
pub fn perform(element: AXUIElementRef, action: &str) -> Result<()> {
    unsafe {
        let name = CFString::new(action);
        let status = AXUIElementPerformAction(element, name.as_concrete_TypeRef());
        if status != kAXErrorSuccess {
            bail!("{}", ax_error_message(status));
        }
    }
    Ok(())
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_owned();
    }
    let kept: String = value.chars().take(max).collect();
    format!("{kept}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn element(index: u64, role: &str, label: &str) -> Element {
        Element {
            index,
            parent_index: None,
            depth: 0,
            role: role.into(),
            label: label.into(),
            value: None,
            actions: vec!["AXPress".into()],
            enabled: true,
            frame: None,
            handle: ElementHandle(std::ptr::null()),
        }
    }

    fn tree(elements: Vec<Element>, truncated: bool) -> AppTree {
        AppTree {
            pid: 1,
            elements,
            truncated,
        }
    }

    #[test]
    fn the_rendered_tree_leads_with_the_index_the_agent_must_use() {
        let rendered = tree(vec![element(0, "AXButton", "Send")], false).render();
        assert_eq!(rendered, "[0] AXButton \"Send\" actions=[AXPress]\n");
    }

    /// An agent that cannot find its target must be able to tell "the control
    /// is not there" from "I did not read far enough".
    #[test]
    fn truncation_is_stated_in_the_tree_itself() {
        let rendered = tree(vec![element(0, "AXButton", "Send")], true).render();
        assert!(rendered.contains("truncated"), "{rendered}");
    }

    #[test]
    fn a_diff_reports_only_new_lines_and_says_so_when_nothing_changed() {
        let before = tree(vec![element(0, "AXButton", "Send")], false);
        let after = tree(
            vec![
                element(0, "AXButton", "Send"),
                element(1, "AXButton", "Cancel"),
            ],
            false,
        );
        let diff = after.diff_from(&before.render());
        assert!(diff.contains("Cancel"));
        assert!(!diff.contains("Send"));
        assert_eq!(before.diff_from(&before.render()), "(no change)\n");
    }

    #[test]
    fn a_disabled_control_is_marked_so_the_agent_does_not_keep_clicking_it() {
        let mut disabled = element(0, "AXButton", "Send");
        disabled.enabled = false;
        assert!(tree(vec![disabled], false).render().contains("disabled"));
    }

    #[test]
    fn a_value_identical_to_the_label_is_not_repeated() {
        let mut labelled = element(0, "AXStaticText", "Inbox");
        labelled.value = Some("Inbox".into());
        assert!(!tree(vec![labelled], false).render().contains("value="));
    }

    #[test]
    fn long_labels_are_truncated_rather_than_flooding_the_tree() {
        let long = "x".repeat(500);
        assert_eq!(truncate(&long, 200).chars().count(), 201);
        assert_eq!(truncate("short", 200), "short");
    }

    #[test]
    fn elements_are_addressable_by_the_index_the_render_shows() {
        let subject = tree(
            vec![
                element(0, "AXButton", "Send"),
                element(1, "AXButton", "Cancel"),
            ],
            false,
        );
        assert_eq!(subject.get(1).unwrap().label, "Cancel");
        assert!(subject.get(9).is_none());
    }
}
