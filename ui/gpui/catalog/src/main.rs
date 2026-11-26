//! Every component of the GPUI kit in each of its states, in a window: the same made-up data and
//! the same sections as the React catalogue (`ui/react/demo/catalog.tsx`) and the SwiftUI one,
//! so the three can be compared.
//!
//!   cargo run -p archipel-gpui-catalog                     the standard look
//!   cargo run -p archipel-gpui-catalog -- --theme minitel  the Minitel theme
//!   --light                                                the light version
//!   --section <id>                                         opens on that section
//!
//! (from `ui/gpui`). The theme and the scheme can also be changed in the window.

mod specimens;

use archipel_gpui::parts::{button, label};
use archipel_gpui::{Theme, set_theme, theme};
use gpui::{
    App, Application, Bounds, Context, FontWeight, IntoElement, Render, ScrollHandle, SharedString,
    Window, WindowBounds, WindowOptions, div, point, prelude::*, px, relative, size,
};
use specimens::{Section, sections};

fn named_theme(name: &str, dark: bool) -> Theme {
    match name {
        "minitel" => archipel_gpui_minitel::minitel(dark),
        _ => Theme::standard(dark),
    }
}

struct Catalog {
    sections: Vec<Section>,
    section: usize,
    theme_name: String,
    dark: bool,
    scroll: ScrollHandle,
}

impl Catalog {
    fn restyle(&mut self, cx: &mut Context<Self>) {
        set_theme(cx, named_theme(&self.theme_name, self.dark));
        cx.notify();
    }
}

impl Render for Catalog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme(cx);
        let mut sidebar = div()
            .flex()
            .flex_col()
            .flex_none()
            .gap(px(2.0))
            .w(px(240.0))
            .h_full()
            .p(px(12.0))
            .border_r_1()
            .border_color(theme.colors.line)
            .child(div().px(px(8.0)).pb(px(8.0)).child(label("Components", cx)));
        for (index, section) in self.sections.iter().enumerate() {
            let chosen = index == self.section;
            sidebar = sidebar.child(
                div()
                    .id(("section", index))
                    .px(px(8.0))
                    .py(px(5.0))
                    .rounded(px(theme.control_radius))
                    .cursor_pointer()
                    .when(chosen, |row| row.bg(theme.colors.running.opacity(0.16)))
                    .when(!chosen, |row| row.hover(|row| row.bg(theme.colors.card)))
                    .child(SharedString::from(section.title))
                    .on_click(cx.listener(move |catalog, _, _, cx| {
                        catalog.section = index;
                        catalog.scroll.set_offset(point(px(0.0), px(0.0)));
                        cx.notify();
                    })),
            );
        }
        let other = if self.theme_name == "minitel" {
            "standard"
        } else {
            "minitel"
        };
        sidebar = sidebar.child(div().flex_1()).child(
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(
                    button("theme", &format!("Theme: {}", self.theme_name), false, cx).on_click(
                        cx.listener(move |catalog, _, _, cx| {
                            catalog.theme_name = other.to_owned();
                            catalog.restyle(cx);
                        }),
                    ),
                )
                .child(
                    button(
                        "scheme",
                        if self.dark { "Dark" } else { "Light" },
                        false,
                        cx,
                    )
                    .on_click(cx.listener(|catalog, _, _, cx| {
                        catalog.dark = !catalog.dark;
                        catalog.restyle(cx);
                    })),
                ),
        );

        let section = &self.sections[self.section];
        let mut page = div().flex().flex_col().gap(px(28.0)).p(px(24.0)).child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(24.0))
                        .font_weight(FontWeight::BOLD)
                        .child(SharedString::from(section.title)),
                )
                .child(
                    div()
                        .text_color(theme.colors.secondary)
                        .child(SharedString::from(section.intro)),
                ),
        );
        for specimen in &section.specimens {
            page = page.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(SharedString::from(specimen.name)),
                    )
                    .child(
                        div()
                            .font_family(theme.text.mono_family.clone())
                            .text_size(px(theme.text.code))
                            .text_color(theme.colors.secondary)
                            .child(SharedString::from(specimen.code)),
                    )
                    .when_some(specimen.note, |column, note| {
                        column.child(
                            div()
                                .text_size(px(theme.text.secondary))
                                .text_color(theme.colors.secondary)
                                .child(SharedString::from(note)),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .w(px(specimen.width))
                            .when_some(specimen.height, |frame, height| frame.h(px(height)))
                            .mt(px(4.0))
                            .child((specimen.draw)(window, cx)),
                    ),
            );
        }
        div()
            .flex()
            .size_full()
            .bg(theme.colors.background)
            .text_color(theme.colors.text)
            .text_size(px(theme.text.body))
            .line_height(relative(theme.text.line_height))
            .child(sidebar)
            .child(
                div()
                    .id("page")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(page),
            )
    }
}

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    let value = |flag: &str| {
        arguments
            .iter()
            .position(|argument| argument == flag)
            .and_then(|at| arguments.get(at + 1))
            .cloned()
    };
    let theme_name = value("--theme").unwrap_or_else(|| "standard".to_owned());
    let dark = !arguments.iter().any(|argument| argument == "--light");
    let wanted = value("--section");
    let offset: f32 = value("--offset")
        .and_then(|offset| offset.parse().ok())
        .unwrap_or(0.0);

    Application::new().run(move |cx: &mut App| {
        archipel_gpui::init(cx);
        set_theme(cx, named_theme(&theme_name, dark));
        let bounds = Bounds::centered(None, size(px(1240.0), px(900.0)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        };
        cx.open_window(options, |window, cx| {
            let sections = sections(window, cx);
            let section = wanted
                .and_then(|id| sections.iter().position(|section| section.id == id))
                .unwrap_or(0);
            let scroll = ScrollHandle::new();
            scroll.set_offset(point(px(0.0), px(-offset)));
            cx.new(|_| Catalog {
                sections,
                section,
                theme_name,
                dark,
                scroll,
            })
        })
        .expect("the catalogue's window opens");
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
}
