//! Small pieces of UI shared by the panes.

use gpui::{
    AnyElement, AnyView, App, AppContext, Div, ElementId, Hsla, InteractiveElement, IntoElement,
    ParentElement, Pixels, Point, Render, SharedString, Stateful, StatefulInteractiveElement,
    Styled, Svg, Window, anchored, deferred, div, point, px, svg,
};

use crate::theme::Theme;

pub fn icon(name: &str, size: f32, color: Hsla) -> Svg {
    svg()
        .path(SharedString::from(format!("icons/{name}.svg")))
        .size(px(size))
        .flex_none()
        .text_color(color)
}

/// A square icon button with a hover state and a tooltip.
pub fn icon_button(
    id: impl Into<ElementId>,
    name: &str,
    tip: &'static str,
    active: bool,
    t: &Theme,
) -> Stateful<Div> {
    let (bg, fg) = if active {
        (t.accent_soft, t.accent)
    } else {
        (gpui::transparent_black(), t.text_muted)
    };
    let hover = if active { t.accent_soft } else { t.hover };
    div()
        .id(id.into())
        .size(px(32.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(7.))
        .bg(bg)
        .hover(move |s| s.bg(hover))
        .active(|s| s.opacity(0.8))
        .cursor_pointer()
        .child(icon(name, 18., fg))
        .tooltip(tooltip(tip))
}

pub fn tooltip(text: &'static str) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    move |window, cx| {
        let t = Theme::for_appearance(window.appearance());
        cx.new(|_| Tooltip { text, theme: t }).into()
    }
}

struct Tooltip {
    text: &'static str,
    theme: Theme,
}

impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        let t = &self.theme;
        div()
            .mt(px(6.))
            .px(px(8.))
            .py(px(4.))
            .rounded(px(6.))
            .bg(t.popover_bg)
            .border_1()
            .border_color(t.border)
            .shadow_md()
            .text_size(px(12.5))
            .text_color(t.text)
            .child(self.text)
    }
}

/// A round colour chip, ringed when chosen.
pub fn swatch(id: impl Into<ElementId>, color: Hsla, selected: bool, t: &Theme) -> Stateful<Div> {
    div()
        .id(id.into())
        .size(px(26.))
        .flex_none()
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .border_2()
        .border_color(if selected {
            t.accent
        } else {
            gpui::transparent_black()
        })
        .cursor_pointer()
        .child(
            div()
                .size(px(18.))
                .rounded_full()
                .bg(color)
                .border_1()
                .border_color(t.border),
        )
}

pub fn panel(t: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .p(px(6.))
        .gap(px(2.))
        .rounded(px(10.))
        .bg(t.popover_bg)
        .border_1()
        .border_color(t.border)
        .shadow_lg()
        .text_size(px(14.))
        .text_color(t.text)
}

pub fn menu_item(
    id: impl Into<ElementId>,
    icon_name: Option<&str>,
    label: impl Into<SharedString>,
    t: &Theme,
) -> Stateful<Div> {
    let hover = t.hover;
    div()
        .id(id.into())
        .flex()
        .items_center()
        .gap(px(10.))
        .px(px(10.))
        .h(px(32.))
        .min_w(px(180.))
        .rounded(px(6.))
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .children(icon_name.map(|n| icon(n, 16., t.text_muted)))
        .child(label.into())
}

pub fn separator(t: &Theme) -> Div {
    div().h(px(1.)).mx(px(4.)).my(px(4.)).bg(t.border)
}

/// Lays `content` over the window at `at`, with a backdrop that swallows
/// the click that dismisses it.
pub fn overlay(
    at: Point<Pixels>,
    viewport: gpui::Size<Pixels>,
    content: AnyElement,
    dismiss: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    deferred(
        anchored().position(point(px(0.), px(0.))).child(
            div()
                .id("overlay-backdrop")
                .w(viewport.width)
                .h(viewport.height)
                .occlude()
                .on_mouse_down(gpui::MouseButton::Left, move |_, w, cx| dismiss(w, cx))
                .child(
                    anchored()
                        .position(at)
                        .snap_to_window_with_margin(px(8.))
                        .child(
                            div()
                                .occlude()
                                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                    cx.stop_propagation()
                                })
                                .child(content),
                        ),
                ),
        ),
    )
    .with_priority(2)
}

/// When a note was last changed, as a list shows it.
pub fn date_label(ms: i64) -> String {
    use chrono::{Datelike, Local, TimeZone};
    let Some(then) = Local.timestamp_millis_opt(ms).single() else {
        return String::new();
    };
    let now = Local::now();
    let ago = (now - then).num_seconds();
    let days = now
        .date_naive()
        .signed_duration_since(then.date_naive())
        .num_days();
    match ago {
        ..60 => "Just now".into(),
        60..3600 => format!("{} min ago", ago / 60),
        _ if days == 0 => then.format("%H:%M").to_string(),
        _ if days == 1 => "Yesterday".into(),
        _ if then.year() == now.year() => then.format("%b %-d").to_string(),
        _ => then.format("%b %-d, %Y").to_string(),
    }
}
