import ArchipelClient
import SwiftUI

/// A setting of the agent (model, effort, permission mode, a switch) as the kit draws it.
/// Nothing here knows a model's or a mode's name: it is all read from what the agent reports.
public struct Setting: Identifiable, Equatable {
    /// One value a setting can take.
    public struct Choice: Identifiable, Equatable {
        public var value: String
        public var name: String
        public var description: String?
        public var id: String { value }
    }

    /// Values under a heading, when the agent groups them. `name` is nil for values without one.
    public struct Group: Equatable {
        public var name: String?
        public var choices: [Choice]
    }

    public enum Value: Equatable {
        case choice(String)
        case on(Bool)
    }

    public var id: String
    public var name: String
    public var description: String?
    /// What the setting is about, such as `model`, `thought_level` or `mode`, when the agent says.
    public var category: String?
    public var current: Value
    /// For a choice: its values.
    public var groups: [Group] = []

    /// Reads the agent's `configOptions`: a session's (`state.values["configOptions"]`) or those
    /// of `client.sessionOptions` before a session exists. A setting of a kind this kit does
    /// not know is left out.
    public static func all(from configOptions: JSON?) -> [Setting] {
        (configOptions?.array ?? []).compactMap { option in
            guard let id = option["configId"]?.string else { return nil }
            var setting = Setting(
                id: id, name: option["name"]?.string ?? id, description: option["description"]?.string,
                category: option["category"]?.string, current: .on(false))
            switch (option["type"]?.string, option["currentValue"]) {
            case ("boolean", .bool(let on)?):
                setting.current = .on(on)
            case ("select", .string(let value)?):
                setting.current = .choice(value)
                var flat: [Choice] = []
                for entry in option["options"]?.array ?? [] {
                    if let inner = entry["options"]?.array {
                        setting.groups.append(Group(name: entry["name"]?.string, choices: inner.compactMap(choice)))
                    } else if let found = choice(entry) {
                        flat.append(found)
                    }
                }
                if !flat.isEmpty { setting.groups.insert(Group(name: nil, choices: flat), at: 0) }
            default:
                return nil
            }
            return setting
        }
    }

    private static func choice(_ option: JSON) -> Choice? {
        guard let value = option["value"]?.string else { return nil }
        return Choice(value: value, name: option["name"]?.string ?? value, description: option["description"]?.string)
    }
}

/// One control per setting: a menu for a choice, a toggle for a switch.
///
/// `change` is called when the user changes one. While it runs the control shows the new
/// value; if it throws, the bar says why and shows the old value again. Without `change` the
/// settings are shown and cannot be changed.
public struct SettingsBar: View {
    let settings: [Setting]
    var change: ((String, Setting.Value) async throws -> Void)?
    /// What the user chose and the agent has not confirmed yet.
    @State private var asked: (id: String, value: Setting.Value)?
    @State private var problem: String?
    @Environment(\.archipelTheme) private var theme

    public init(settings: [Setting], change: ((String, Setting.Value) async throws -> Void)? = nil) {
        self.settings = settings
        self.change = change
    }

    public var body: some View {
        if !settings.isEmpty {
            VStack(alignment: .leading, spacing: 4) {
                // Wraps onto more lines when the controls do not fit beside each other.
                Flow(spacing: 14, lineSpacing: 8) {
                    ForEach(settings) { setting in
                        control(setting).help(setting.description ?? "")
                    }
                }
                .disabled(change == nil || asked != nil)
                if let problem { Text("Not changed: \(problem)").font(theme.fonts.secondary).foregroundStyle(theme.colors.bad) }
            }
            .controlSize(theme.settingsControlSize)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    @ViewBuilder
    private func control(_ setting: Setting) -> some View {
        let shown = asked?.id == setting.id ? asked?.value ?? setting.current : setting.current
        switch shown {
        case .on(let on):
            Toggle(setting.name, isOn: Binding(get: { on }, set: { set(setting, .on($0)) }))
                .fixedSize()
        case .choice(let value):
            Picker(setting.name, selection: Binding(get: { value }, set: { set(setting, .choice($0)) })) {
                ForEach(Array(setting.groups.enumerated()), id: \.offset) { _, group in
                    if let name = group.name {
                        Section(name) { ForEach(group.choices) { Text($0.name).tag($0.value) } }
                    } else {
                        ForEach(group.choices) { Text($0.name).tag($0.value) }
                    }
                }
            }
            .pickerStyle(.menu)
            .fixedSize()
        }
    }

    private func set(_ setting: Setting, _ value: Setting.Value) {
        guard let change, value != setting.current else { return }
        asked = (setting.id, value)
        problem = nil
        Task {
            do { try await change(setting.id, value) } catch { problem = "\(setting.name): \(describe(error))" }
            asked = nil
        }
    }
}

/// Lays its views out in a row, and starts a new row when the next one does not fit.
struct Flow: Layout {
    var spacing: CGFloat
    var lineSpacing: CGFloat

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let rows = arrange(subviews, width: proposal.width ?? .infinity)
        return CGSize(width: rows.map(\.width).max() ?? 0, height: rows.map(\.height).reduce(0, +) + lineSpacing * CGFloat(max(rows.count - 1, 0)))
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var y = bounds.minY
        for row in arrange(subviews, width: bounds.width) {
            var x = bounds.minX
            for index in row.items {
                let size = subviews[index].sizeThatFits(.unspecified)
                subviews[index].place(at: CGPoint(x: x, y: y + (row.height - size.height) / 2), proposal: ProposedViewSize(size))
                x += size.width + spacing
            }
            y += row.height + lineSpacing
        }
    }

    private func arrange(_ subviews: Subviews, width: CGFloat) -> [(items: [Int], width: CGFloat, height: CGFloat)] {
        var rows: [(items: [Int], width: CGFloat, height: CGFloat)] = []
        for (index, subview) in subviews.enumerated() {
            let size = subview.sizeThatFits(.unspecified)
            if let last = rows.last, last.width + spacing + size.width <= width {
                rows[rows.count - 1] = (last.items + [index], last.width + spacing + size.width, max(last.height, size.height))
            } else {
                rows.append(([index], size.width, size.height))
            }
        }
        return rows
    }
}

/// An error in words for the user: the daemon's message without its code.
func describe(_ error: Error) -> String {
    (error as? RPCError)?.message ?? String(describing: error)
}
