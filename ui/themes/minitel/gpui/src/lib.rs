//! The Minitel theme for the GPUI kit: the look recorded in `ui/themes/minitel/DESIGN.md`, the
//! same colours and sizes as `react/styles.css` and the SwiftUI version. `#282C34`, the
//! terminal's own background, is the main one, so a session reads as one surface with the
//! terminal beside it.
//!
//! ```ignore
//! archipel_gpui::init(cx);
//! archipel_gpui::set_theme(cx, archipel_gpui_minitel::minitel(true));
//! ```

use archipel_gpui::theme::{BadgeStyle, Colors, Theme, Type, default_mono_family, hex};
use gpui::FontWeight;

const ANSI_DARK: [u32; 16] = [
    0x3A3F4B, 0xF2777A, 0x59C780, 0xF2A640, 0x8799DE, 0xBA8AF5, 0x56C2D6, 0xC8CDD8, 0x868EA2,
    0xFF9A9C, 0x84DDA4, 0xF7C47A, 0xAAB7EC, 0xD2B1F9, 0x86DCEA, 0xFFFFFF,
];
const ANSI_LIGHT: [u32; 16] = [
    0x16191D, 0xB33833, 0x1D7A45, 0x8A5300, 0x3F55B0, 0x7A3FC9, 0x12707F, 0x545B6B, 0x6B7385,
    0xC9463F, 0x23914F, 0x9E6200, 0x4F66C7, 0x8E52DA, 0x16879A, 0x2B2F37,
];

/// Minitel: dark as in the app, or the light version of the same colours.
#[must_use]
pub fn minitel(dark: bool) -> Theme {
    let pick = |on_dark: u32, on_light: u32| hex(if dark { on_dark } else { on_light });
    let text = pick(0xE6E9EF, 0x16191D);
    let raised = pick(0x3A3F4B, 0xE4E7EE);
    Theme {
        colors: Colors {
            text,
            secondary: pick(0xA9B0BF, 0x545B6B),
            background: pick(0x282C34, 0xF3F4F7),
            card: pick(0x30343E, 0xFFFFFF),
            raised,
            message: raised,
            on_message: text,
            sunken: pick(0x16191D, 0xEAECF1),
            line: pick(0x444A58, 0xD3D7E0),
            edge: hex(0x868EA2),
            accent: pick(0x8799DE, 0x3F55B0),
            on_accent: pick(0x16191D, 0xFFFFFF),
            ok: pick(0x59C780, 0x1D7A45),
            bad: pick(0xF2777A, 0xB33833),
            // Purple: running, and the selected session. Amber: only what waits on the user.
            running: pick(0xBA8AF5, 0x7A3FC9),
            attention: pick(0xF2A640, 0x8A5300),
            ansi: (if dark { ANSI_DARK } else { ANSI_LIGHT }).map(hex),
        },
        // Nothing small: body 16, code and secondary 15, labels, badges and details 14.
        text: Type {
            body: 16.0,
            secondary: 15.0,
            title: 18.0,
            request_title: 17.0,
            heading_large: 19.0,
            heading: 17.0,
            mono: 15.0,
            code: 15.0,
            meta: 14.0,
            label: 14.0,
            label_weight: FontWeight::SEMIBOLD,
            line_height: 1.6,
            family: None,
            mono_family: default_mono_family(),
        },
        badge: BadgeStyle::Tinted,
        card_radius: 10.0,
        control_radius: 8.0,
        card_padding: (16.0, 12.0),
        row_spacing: 14.0,
        waiting_border: 2.0,
        waiting_fill: 0.08,
        control_height: 36.0,
        folder_labels: true,
        dark,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the theme promises: #282C34 behind everything when dark, nothing under 14 pixels,
    /// and amber kept apart from running.
    #[test]
    fn the_theme_keeps_its_promises() {
        let dark = minitel(true);
        assert_eq!(dark.colors.background, hex(0x282C34));
        assert!(dark.dark && !minitel(false).dark);
        let sizes = [
            dark.text.body,
            dark.text.secondary,
            dark.text.title,
            dark.text.request_title,
            dark.text.heading_large,
            dark.text.heading,
            dark.text.mono,
            dark.text.code,
            dark.text.meta,
            dark.text.label,
        ];
        assert!(sizes.iter().all(|size| *size >= 14.0));
        assert_ne!(dark.colors.attention, dark.colors.running);
        assert_eq!(dark.badge, BadgeStyle::Tinted);
    }
}
