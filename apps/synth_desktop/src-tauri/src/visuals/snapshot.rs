//! In-process WKWebView snapshot for review capture.
//!
//! `WKWebView takeSnapshot` renders the webview's own surface, so it needs no
//! Screen Recording TCC grant, works while the window is occluded or
//! backgrounded, and can never photograph the wrong window. This replaced the
//! `/usr/sbin/screencapture` shell-out, whose TCC principal was whichever
//! ad-hoc-signed helper happened to spawn it.
//! See: docs/contracts/desktop_review_capture.md.

use anyhow::{anyhow, Context, Result};
use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage};
use objc2_foundation::{NSDictionary, NSError};
use objc2_web_kit::{WKSnapshotConfiguration, WKWebView};
use std::sync::{Arc, Mutex};
use std::time::Duration;

type SnapshotSlot = Arc<Mutex<Option<tokio::sync::oneshot::Sender<Result<Vec<u8>, String>>>>>;

/// Snapshot the window's webview and return PNG bytes.
///
/// The closure handed to `with_webview` and the WebKit completion handler both
/// run on the main run loop; the caller awaits off it. The timeout is load
/// bearing: without it a wedged WebKit process would hang the IPC route that
/// called this, and the helper behind that route holds a resized window.
pub async fn capture_webview_png(
    window: &tauri::WebviewWindow,
    timeout: Duration,
) -> Result<Vec<u8>> {
    let (sender, receiver) = tokio::sync::oneshot::channel::<Result<Vec<u8>, String>>();
    let slot: SnapshotSlot = Arc::new(Mutex::new(Some(sender)));
    let deliver = move |result: Result<Vec<u8>, String>| {
        if let Ok(mut guard) = slot.lock() {
            if let Some(sender) = guard.take() {
                let _ = sender.send(result);
            }
        }
    };
    window
        .with_webview(move |platform| {
            let Some(mtm) = MainThreadMarker::new() else {
                deliver(Err("webview snapshot ran off the main thread".into()));
                return;
            };
            let completion = deliver.clone();
            unsafe {
                let view: &WKWebView = &*platform.inner().cast();
                let configuration = WKSnapshotConfiguration::new(mtm);
                // The review resize just changed layout; snapshot what the
                // renderer will actually show, not the stale backing store.
                configuration.setAfterScreenUpdates(true);
                let block = RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
                    completion(encode_snapshot_png(image, error));
                });
                view.takeSnapshotWithConfiguration_completionHandler(Some(&configuration), &block);
            }
        })
        .context("dispatch webview snapshot to the main thread")?;
    match tokio::time::timeout(timeout, receiver).await {
        Err(_) => Err(anyhow!(
            "webview snapshot produced no image within {}s",
            timeout.as_secs()
        )),
        Ok(Err(_)) => Err(anyhow!("webview snapshot completion was dropped")),
        Ok(Ok(Err(message))) => Err(anyhow!("{message}")),
        Ok(Ok(Ok(bytes))) => Ok(bytes),
    }
}

/// Convert the completion handler's `NSImage` into PNG bytes.
///
/// # Safety
/// Both pointers come straight from the WebKit completion handler and are only
/// dereferenced for the duration of that callback.
unsafe fn encode_snapshot_png(image: *mut NSImage, error: *mut NSError) -> Result<Vec<u8>, String> {
    if !error.is_null() {
        return Err(format!(
            "WebViewSnapshotFailed: {}",
            (*error).localizedDescription()
        ));
    }
    if image.is_null() {
        return Err("WebViewSnapshotFailed: WebKit returned no image".into());
    }
    let tiff = (*image)
        .TIFFRepresentation()
        .ok_or("WebViewSnapshotFailed: snapshot image has no bitmap representation")?;
    let rep = NSBitmapImageRep::imageRepWithData(&tiff)
        .ok_or("WebViewSnapshotFailed: snapshot bitmap could not be decoded")?;
    let png = rep
        .representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
        .ok_or("WebViewSnapshotFailed: snapshot could not be encoded as PNG")?;
    Ok(png.to_vec())
}
