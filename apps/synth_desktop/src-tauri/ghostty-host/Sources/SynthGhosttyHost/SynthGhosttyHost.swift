import AppKit
import Foundation
import GhosttyTerminal

public typealias WriteCallback = @convention(c) (
    UnsafePointer<UInt8>?,
    Int,
    UnsafeMutableRawPointer?
) -> Void

public typealias ResizeCallback = @convention(c) (
    UInt16,
    UInt16,
    UnsafeMutableRawPointer?
) -> Void

@MainActor
private final class HostedTerminal {
    let view: TerminalView
    let session: InMemoryTerminalSession
    let controller: TerminalController

    init(
        parent: NSView,
        frame: NSRect,
        fontFamily: String,
        fontSize: Float,
        write: WriteCallback?,
        resize: ResizeCallback?,
        userdata: UnsafeMutableRawPointer?
    ) {
        let userdataAddress = UInt(bitPattern: userdata)
        session = InMemoryTerminalSession(
            write: { data in
                data.withUnsafeBytes { bytes in
                    write?(
                        bytes.bindMemory(to: UInt8.self).baseAddress,
                        bytes.count,
                        UnsafeMutableRawPointer(bitPattern: userdataAddress)
                    )
                }
            },
            resize: { viewport in
                resize?(
                    viewport.columns,
                    viewport.rows,
                    UnsafeMutableRawPointer(bitPattern: userdataAddress)
                )
            },
            suppressesPixelOnlyResizes: true
        )
        controller = TerminalController { builder in
            builder.withFontFamily(fontFamily)
            builder.withBackground("ffffff")
            builder.withForeground("2d2a27")
            builder.withCursorColor("e45b2b")
            builder.withCursorText("ffffff")
            builder.withSelectionBackground("f2d7c8")
            builder.withBackgroundOpacity(1)
            builder.withWindowPaddingX(12)
            builder.withWindowPaddingY(10)
        }
        view = TerminalView(frame: frame)
        view.setAccessibilityElement(true)
        view.setAccessibilityIdentifier("synth.terminal.surface")
        view.setAccessibilityLabel("Terminal")
        view.controller = controller
        view.configuration = TerminalSurfaceOptions(
            backend: .inMemory(session),
            fontSize: fontSize,
            context: .split,
            resizeThrottleMilliseconds: 16
        )
        parent.addSubview(view)
        view.fitToSize()
    }
}

@MainActor
private func browserRect(
    in parent: NSView,
    x: Double,
    top: Double,
    width: Double,
    height: Double
) -> NSRect {
    let y = parent.isFlipped
        ? top
        : parent.bounds.height - top - height
    return NSRect(x: x, y: y, width: width, height: height)
}

private func onMain<T: Sendable>(_ work: @MainActor () -> T) -> T {
    if Thread.isMainThread {
        return MainActor.assumeIsolated(work)
    }
    return DispatchQueue.main.sync {
        MainActor.assumeIsolated(work)
    }
}

@_cdecl("synth_ghostty_host_create")
public func synthGhosttyHostCreate(
    _ parentPointer: UnsafeMutableRawPointer?,
    _ x: Double,
    _ top: Double,
    _ width: Double,
    _ height: Double,
    _ fontFamilyPointer: UnsafePointer<CChar>?,
    _ fontSize: Float,
    _ write: WriteCallback?,
    _ resize: ResizeCallback?,
    _ userdata: UnsafeMutableRawPointer?
) -> UnsafeMutableRawPointer? {
    guard let parentPointer else { return nil }
    let parentAddress = UInt(bitPattern: parentPointer)
    let userdataAddress = UInt(bitPattern: userdata)
    let fontFamily = fontFamilyPointer.map(String.init(cString:)) ?? "Menlo"
    let address: UInt = onMain {
        let parent = Unmanaged<NSView>
            .fromOpaque(UnsafeMutableRawPointer(bitPattern: parentAddress)!)
            .takeUnretainedValue()
        let host = HostedTerminal(
            parent: parent,
            frame: browserRect(
                in: parent,
                x: x,
                top: top,
                width: width,
                height: height
            ),
            fontFamily: fontFamily,
            fontSize: fontSize,
            write: write,
            resize: resize,
            userdata: UnsafeMutableRawPointer(bitPattern: userdataAddress)
        )
        return UInt(bitPattern: Unmanaged.passRetained(host).toOpaque())
    }
    return UnsafeMutableRawPointer(bitPattern: address)
}

@_cdecl("synth_ghostty_host_receive")
public func synthGhosttyHostReceive(
    _ handle: UnsafeMutableRawPointer?,
    _ bytes: UnsafePointer<UInt8>?,
    _ count: Int
) {
    guard let handle, let bytes, count > 0 else { return }
    let host = Unmanaged<HostedTerminal>.fromOpaque(handle).takeUnretainedValue()
    host.session.receive(Data(bytes: bytes, count: count))
}

@_cdecl("synth_ghostty_host_finish")
public func synthGhosttyHostFinish(
    _ handle: UnsafeMutableRawPointer?,
    _ exitCode: UInt32,
    _ runtimeMilliseconds: UInt64
) {
    guard let handle else { return }
    let host = Unmanaged<HostedTerminal>.fromOpaque(handle).takeUnretainedValue()
    host.session.finish(exitCode: exitCode, runtimeMilliseconds: runtimeMilliseconds)
}

@_cdecl("synth_ghostty_host_set_frame")
public func synthGhosttyHostSetFrame(
    _ handle: UnsafeMutableRawPointer?,
    _ x: Double,
    _ top: Double,
    _ width: Double,
    _ height: Double
) {
    guard let handle else { return }
    let address = UInt(bitPattern: handle)
    onMain {
        let host = Unmanaged<HostedTerminal>
            .fromOpaque(UnsafeMutableRawPointer(bitPattern: address)!)
            .takeUnretainedValue()
        guard let parent = host.view.superview else { return }
        host.view.frame = browserRect(
            in: parent,
            x: x,
            top: top,
            width: width,
            height: height
        )
        host.view.fitToSize()
    }
}

@_cdecl("synth_ghostty_host_set_visible")
public func synthGhosttyHostSetVisible(
    _ handle: UnsafeMutableRawPointer?,
    _ visible: Bool
) {
    guard let handle else { return }
    let address = UInt(bitPattern: handle)
    onMain {
        let host = Unmanaged<HostedTerminal>
            .fromOpaque(UnsafeMutableRawPointer(bitPattern: address)!)
            .takeUnretainedValue()
        host.view.isHidden = !visible
        host.view.setSurfaceVisible(visible)
    }
}

@_cdecl("synth_ghostty_host_focus")
public func synthGhosttyHostFocus(_ handle: UnsafeMutableRawPointer?) {
    guard let handle else { return }
    let address = UInt(bitPattern: handle)
    onMain {
        let host = Unmanaged<HostedTerminal>
            .fromOpaque(UnsafeMutableRawPointer(bitPattern: address)!)
            .takeUnretainedValue()
        host.view.window?.makeFirstResponder(host.view)
    }
}

@_cdecl("synth_ghostty_host_destroy")
public func synthGhosttyHostDestroy(_ handle: UnsafeMutableRawPointer?) {
    guard let handle else { return }
    let address = UInt(bitPattern: handle)
    onMain {
        let host = Unmanaged<HostedTerminal>
            .fromOpaque(UnsafeMutableRawPointer(bitPattern: address)!)
            .takeRetainedValue()
        host.view.removeFromSuperview()
    }
}
