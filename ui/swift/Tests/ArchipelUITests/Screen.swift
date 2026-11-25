import AppKit
import SwiftUI
import XCTest

/// A SwiftUI view in a window that is never shown, used the way a person would: its buttons
/// are found by their label and pressed, and text is typed into its fields.
///
/// The controls are found through accessibility, inside this process, so nothing needs the
/// user's permission and no window appears. Menus, alerts, file dialogs and drags cannot be
/// driven this way: they run their own event loop.
@MainActor
final class Screen {
    private let window: NSWindow
    private let host: NSView

    init<Content: View>(width: CGFloat = 640, height: CGFloat = 480, @ViewBuilder _ content: () -> Content) {
        _ = NSApplication.shared
        // SwiftUI describes its views to accessibility only once something has asked for it.
        NSApp.accessibilitySetValue(true, forAttribute: NSAccessibility.Attribute(rawValue: "AXEnhancedUserInterface"))
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: width, height: height), styleMask: [.titled], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        host = NSHostingView(rootView: content().frame(width: width, height: height, alignment: .top))
        window.contentView = host
        settle()
    }

    deinit { MainActor.assumeIsolated { window.close() } }

    /// Lets the view take in what just happened.
    func settle(_ seconds: TimeInterval = 0.05) {
        RunLoop.current.run(until: Date().addingTimeInterval(seconds))
        host.layoutSubtreeIfNeeded()
        // A list builds its rows when it is drawn. This draws into the window's memory only.
        window.displayIfNeeded()
    }

    /// Waits until `condition` holds, for work the view does in a task.
    func wait(_ what: String, file: StaticString = #filePath, line: UInt = #line, until condition: () -> Bool) {
        let deadline = Date().addingTimeInterval(3)
        while !condition() {
            if Date() > deadline { return XCTFail("timed out waiting until \(what); on screen: \(texts)", file: file, line: line) }
            settle(0.02)
        }
        settle()
    }

    private struct Element {
        let object: AnyObject
        let role: String
        let label: String
        let value: String
    }

    private var elements: [Element] {
        var found: [Element] = []
        var seen: Set<ObjectIdentifier> = []
        func walk(_ object: AnyObject) {
            guard seen.insert(ObjectIdentifier(object)).inserted else { return }
            let role = object.accessibilityRole?()?.rawValue ?? ""
            let label = [object.accessibilityLabel?(), object.accessibilityTitle?()].compactMap { $0 }.first { !$0.isEmpty } ?? ""
            // A value is a string for text and a number for a switch.
            let asksValue = NSSelectorFromString("accessibilityValue")
            let raw = (object as? NSObject).flatMap { $0.responds(to: asksValue) ? $0.perform(asksValue)?.takeUnretainedValue() : nil }
            let value = (raw as? String) ?? (raw as? NSNumber)?.stringValue ?? ""
            found.append(Element(object: object, role: role, label: label, value: value))
            for child in object.accessibilityChildren?() ?? [] { walk(child as AnyObject) }
            // A list's rows do not say what is in them; the views inside them do.
            for child in (object as? NSView)?.subviews ?? [] { walk(child) }
        }
        walk(host)
        return found
    }

    /// Every piece of text on the screen, in order: static text, and the labels of controls.
    var texts: [String] {
        elements.compactMap { element in
            switch element.role {
            case "AXStaticText": return element.value.isEmpty ? element.label : element.value
            case "AXButton", "AXLink", "AXHeading", "AXCheckBox", "AXDisclosureTriangle", "AXPopUpButton": return element.label.isEmpty ? nil : element.label
            default: return nil
            }
        }
    }

    func shows(_ text: String) -> Bool { texts.contains { $0.contains(text) } }

    /// The labels of the buttons on screen.
    var buttons: [String] { elements.filter { $0.role == "AXButton" && !$0.label.isEmpty }.map(\.label) }

    /// Presses the button, switch or fold with this label.
    func press(_ label: String, file: StaticString = #filePath, line: UInt = #line) {
        let pressable: Set = ["AXButton", "AXLink", "AXCheckBox", "AXDisclosureTriangle"]
        guard let element = elements.first(where: { pressable.contains($0.role) && $0.label == label }) else {
            return XCTFail("no control labelled “\(label)”; on screen: \(texts)", file: file, line: line)
        }
        if element.object.isAccessibilityEnabled?() == false {
            return XCTFail("“\(label)” is disabled", file: file, line: line)
        }
        XCTAssertEqual(element.object.accessibilityPerformPress?(), true, "“\(label)” could not be pressed", file: file, line: line)
        settle()
    }

    func isEnabled(_ label: String) -> Bool {
        elements.first { $0.label == label }?.object.isAccessibilityEnabled?() ?? false
    }

    /// The value of the switch with this label.
    func isOn(_ label: String) -> Bool { elements.first { $0.role == "AXCheckBox" && $0.label == label }?.value == "1" }

    private var fields: [NSTextField] {
        var found: [NSTextField] = []
        func walk(_ view: NSView) {
            if let field = view as? NSTextField, field.isEditable { found.append(field) }
            view.subviews.forEach(walk)
        }
        walk(host)
        return found
    }

    private func field(_ placeholder: String, file: StaticString, line: UInt) -> NSTextField? {
        let field = fields.first { $0.placeholderString == placeholder || $0.placeholderAttributedString?.string == placeholder }
        if field == nil { XCTFail("no field “\(placeholder)”; fields: \(fields.map { $0.placeholderString ?? "" })", file: file, line: line) }
        return field
    }

    /// What the field with this placeholder holds.
    func text(in placeholder: String, file: StaticString = #filePath, line: UInt = #line) -> String {
        field(placeholder, file: file, line: line)?.stringValue ?? ""
    }

    /// Types at the end of the field with this placeholder, as the keyboard would.
    func type(_ text: String, into placeholder: String, file: StaticString = #filePath, line: UInt = #line) {
        guard let field = field(placeholder, file: file, line: line) else { return }
        if field.currentEditor() == nil { window.makeFirstResponder(field) }
        guard let editor = field.currentEditor() as? NSTextView else { return XCTFail("“\(placeholder)” cannot be typed into", file: file, line: line) }
        editor.setSelectedRange(NSRange(location: editor.string.utf16.count, length: 0))
        editor.insertText(text, replacementRange: editor.selectedRange())
        settle()
    }

    /// Presses Return in the field with this placeholder.
    func pressReturn(in placeholder: String, file: StaticString = #filePath, line: UInt = #line) {
        guard let field = field(placeholder, file: file, line: line) else { return }
        if field.currentEditor() == nil { window.makeFirstResponder(field) }
        (field.currentEditor() as? NSTextView)?.insertNewline(nil)
        settle()
    }
}
