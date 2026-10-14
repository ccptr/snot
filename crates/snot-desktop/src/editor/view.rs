//! Drawing the note: toolbar, title, and each block.

use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;

use gpui::{
    AnyElement, AppContext, Bounds, Context, CursorStyle, ElementInputHandler, Entity,
    ExternalPaths, Font, FontStyle, FontWeight, Hsla, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, ParentElement, PathPromptOptions, Pixels, Render, ScrollHandle,
    StatefulInteractiveElement, StrikethroughStyle, Styled, StyledText, Subscription, TextLayout,
    TextRun, UnderlineStyle, WeakEntity, Window, canvas, div, img, point, prelude::FluentBuilder,
    px, size,
};
use snot_core::doc::{Block, Content, TextBlock};
use snot_core::page::{self, PAGE_WIDTH};
use snot_core::{Mark, Paper, ParaStyle, color};

use super::ink::{self, InkTool};
use super::{NoteEvent, NoteView, Popover, Selection};
use crate::assets::{MONO_FONT, UI_FONT};
use crate::theme::{Theme, argb};
use crate::ui::{self, icon, icon_button, menu_item, panel, separator, swatch};

/// Pixels per page unit at 100% zoom: body text at 20px.
const BASE_SCALE: f32 = 0.5;
/// Space kept either side of the page when the window is narrow.
const GUTTER: f32 = 16.;

/// A paragraph style's text, in pixels at the page's current zoom.
struct TextLook {
    size: f32,
    line_height: f32,
    weight: FontWeight,
    mono: bool,
    color: Hsla,
    top: f32,
    bottom: f32,
}

fn look(tb: &TextBlock, t: &Theme, z: f32) -> TextLook {
    let m = page::text_metrics(tb.style);
    TextLook {
        size: m.size * z,
        line_height: (m.line_height * z).round(),
        weight: if m.semibold {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::NORMAL
        },
        mono: m.mono,
        color: match tb.style {
            ParaStyle::Quote => t.text_muted,
            ParaStyle::Checklist if tb.checked => t.text_faint,
            _ => t.text,
        },
        top: m.top * z,
        bottom: m.bottom * z,
    }
}

/// GPUI text runs for a block's styled runs, with the IME composition
/// underlined.
fn text_runs(
    tb: &TextBlock,
    l: &TextLook,
    t: &Theme,
    marked: Option<Range<usize>>,
) -> Vec<TextRun> {
    let byte_of: Vec<usize> = tb
        .text
        .char_indices()
        .map(|(i, _)| i)
        .chain([tb.text.len()])
        .collect();
    let mut out = Vec::new();
    for r in tb.runs() {
        let (start, end) = (byte_of[r.start as usize], byte_of[r.end as usize]);
        let weight = if r.bold {
            FontWeight::SEMIBOLD
        } else {
            l.weight
        };
        let font = Font {
            family: if l.mono { MONO_FONT } else { UI_FONT }.into(),
            weight: if l.mono && r.bold {
                FontWeight::BOLD
            } else {
                weight
            },
            style: if r.italic {
                FontStyle::Italic
            } else {
                FontStyle::Normal
            },
            features: Default::default(),
            fallbacks: None,
        };
        let color = r
            .color
            .as_deref()
            .map(|c| argb(color::ink(c, t.dark)))
            .unwrap_or(l.color);
        let strike = r.strike || tb.style == ParaStyle::Checklist && tb.checked;
        let run = TextRun {
            len: end - start,
            font,
            color,
            background_color: r
                .highlight
                .as_deref()
                .map(|h| argb(color::highlight(h, t.dark))),
            underline: r.underline.then_some(UnderlineStyle {
                color: Some(color),
                thickness: px(1.),
                wavy: false,
            }),
            strikethrough: strike.then_some(StrikethroughStyle {
                color: Some(color),
                thickness: px(1.),
            }),
        };
        // Cut at the composition's edges.
        let mut pos = start;
        let cuts = marked
            .iter()
            .flat_map(|m| [m.start, m.end])
            .filter(|c| *c > start && *c < end)
            .chain([end]);
        for cut in cuts {
            let in_marked = marked
                .as_ref()
                .is_some_and(|m| pos >= m.start && cut <= m.end);
            let mut piece = TextRun {
                len: cut - pos,
                ..run.clone()
            };
            if in_marked {
                piece.underline = Some(UnderlineStyle {
                    color: Some(color),
                    thickness: px(1.5),
                    wavy: false,
                });
            }
            if piece.len > 0 {
                out.push(piece);
            }
            pos = cut;
        }
    }
    if tb.text.is_empty() {
        out.clear();
    }
    out
}

thread_local! {
    /// The zoom chosen with Ctrl+= and Ctrl+-, kept from note to note.
    static ZOOM: std::cell::Cell<f32> = const { std::cell::Cell::new(1.0) };
}

impl NoteView {
    /// Pixels per page unit: the zoom chosen, but never wider than the
    /// window can show.
    pub(crate) fn scale(&self) -> f32 {
        scale_in(&self.scroll)
    }

    pub(crate) fn zoom_by(&mut self, factor: f32, cx: &mut Context<Self>) {
        let z = if factor == 0. {
            1.0
        } else {
            (ZOOM.get() * factor).clamp(0.5, 3.0)
        };
        ZOOM.set(z);
        cx.notify();
    }

    pub(crate) fn to_page(&self, pos: gpui::Point<Pixels>) -> [f32; 2] {
        ink::to_page(self.geometry.borrow().page, self.scale(), pos)
    }

    fn render_text(
        &self,
        b: &Block,
        tb: &TextBlock,
        number: Option<u32>,
        focused: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = self.theme;
        let z = self.scale();
        let l = look(tb, &t, z);
        let id = b.id.clone();
        let sel = self.sel.as_ref().filter(|s| s.block == id).cloned();
        let marked = sel.as_ref().and(self.marked.clone());
        let styled = StyledText::new(tb.text.clone()).with_runs(text_runs(tb, &l, &t, marked));
        let layout = styled.layout().clone();
        self.geometry
            .borrow_mut()
            .text
            .insert(id.clone(), layout.clone());

        let prefix: Option<AnyElement> = match tb.style {
            ParaStyle::Bullet => Some(
                div()
                    .h(px(l.line_height))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .size(px(page::BULLET * z))
                            .rounded_full()
                            .when(tb.indent.is_multiple_of(2), |d| d.bg(t.text))
                            .when(!tb.indent.is_multiple_of(2), |d| {
                                d.border_1().border_color(t.text)
                            }),
                    )
                    .into_any_element(),
            ),
            ParaStyle::Numbered => Some(
                div()
                    .text_color(t.text_muted)
                    .child(format!("{}.", number.unwrap_or(1)))
                    .into_any_element(),
            ),
            ParaStyle::Checklist => {
                let bid = id.clone();
                let checked = tb.checked;
                Some(
                    div()
                        .id(gpui::SharedString::from(format!("check-{id}")))
                        .mt(px((l.line_height - page::CHECKBOX * z) / 2.))
                        .size(px(page::CHECKBOX * z))
                        .rounded(px(12. * z))
                        .border_2()
                        .border_color(if checked { t.accent } else { t.text_faint })
                        .when(checked, |d| d.bg(t.accent))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .when(checked, |d| d.child(icon("check", 36. * z, t.accent_text)))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(move |this, _, _, cx| this.toggle_checked(&bid, cx)))
                        .into_any_element(),
                )
            }
            _ => None,
        };

        let geometry = self.geometry.clone();
        let ready_id = id.clone();
        let caret = t.caret;
        let selection = t.selection;
        let overlay = canvas(
            move |_, _, _| {
                geometry.borrow_mut().ready.insert(ready_id);
            },
            move |_, _, window, _| {
                let Some(sel) = sel.filter(|_| focused) else {
                    return;
                };
                paint_selection(&layout, &sel, caret, selection, window);
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();

        let down_id = id.clone();
        let is_code = tb.style == ParaStyle::Code;
        let is_quote = tb.style == ParaStyle::Quote;
        div()
            .id(gpui::SharedString::from(id.clone()))
            .flex()
            .w_full()
            .pt(px(l.top))
            .pb(px(l.bottom))
            .pl(px(page::INDENT * z * tb.indent as f32))
            .cursor(CursorStyle::IBeam)
            .text_size(px(l.size))
            .line_height(px(l.line_height))
            .font_weight(l.weight)
            .when(l.mono, |d| d.font_family(MONO_FONT))
            .text_color(l.color)
            .children(prefix.map(|p| div().w(px(page::PREFIX * z)).flex_none().flex().child(p)))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .when(is_code, |d| {
                        d.bg(t.code_bg)
                            .rounded(px(15. * z))
                            .px(px(page::CODE_PAD_X * z))
                            .py(px(page::CODE_PAD_Y * z))
                    })
                    .when(is_quote, |d| {
                        d.border_l(px(page::QUOTE_BAR * z))
                            .border_color(t.quote_bar)
                            .pl(px(page::QUOTE_GAP * z))
                    })
                    .child(div().relative().w_full().child(styled).child(overlay)),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                    this.text_mouse_down(
                        &down_id,
                        e.position,
                        e.click_count,
                        e.modifiers.shift,
                        window,
                        cx,
                    )
                }),
            )
            .into_any_element()
    }

    fn render_image(
        &self,
        b: &Block,
        image: &snot_core::ImageBlock,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = self.theme;
        let id = b.id.clone();
        let picked = self.picked.as_deref() == Some(&id);
        let path = self.lib.attachment_path(&image.attachment);
        let group = gpui::SharedString::from(format!("img-{id}"));
        let (a, bb, c, down) = (id.clone(), id.clone(), id.clone(), id.clone());
        let mut bar = div()
            .absolute()
            .top(px(8.))
            .right(px(8.))
            .flex()
            .gap(px(2.))
            .p(px(3.))
            .rounded(px(9.))
            .bg(t.popover_bg)
            .border_1()
            .border_color(t.border)
            .shadow_md()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                icon_button(
                    gpui::SharedString::from(format!("img-up-{id}")),
                    "arrow-up",
                    "Move up",
                    false,
                    &t,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.move_block(&a, -1, cx))),
            )
            .child(
                icon_button(
                    gpui::SharedString::from(format!("img-down-{id}")),
                    "arrow-down",
                    "Move down",
                    false,
                    &t,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.move_block(&bb, 1, cx))),
            )
            .child(
                icon_button(
                    gpui::SharedString::from(format!("img-delete-{id}")),
                    "trash",
                    "Delete picture",
                    false,
                    &t,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.delete_block(&c, cx))),
            );
        if !picked {
            bar = bar.invisible().group_hover(group.clone(), |s| s.visible());
        }
        let z = self.scale();
        div()
            .id(gpui::SharedString::from(id.clone()))
            .group(group)
            .relative()
            .w_full()
            .my(px(page::IMAGE_GAP * z))
            .flex()
            .flex_col()
            .gap(px(15. * z))
            .cursor_pointer()
            .child(
                div()
                    .w_full()
                    .rounded(px(20. * z))
                    .overflow_hidden()
                    .border_2()
                    .border_color(if picked {
                        t.accent
                    } else {
                        gpui::transparent_black()
                    })
                    .child(img(path).w_full().aspect_ratio(1. / image.aspect.max(0.05))),
            )
            .when(!image.caption.is_empty(), |d| {
                d.child(
                    div()
                        .w_full()
                        .flex()
                        .justify_center()
                        .text_size(px(page::CAPTION_SIZE * z))
                        .line_height(px(page::CAPTION_SIZE * page::LINE_HEIGHT * z))
                        .italic()
                        .text_color(t.text_muted)
                        .child(image.caption.clone()),
                )
            })
            .child(bar)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| this.pick(&down, window, cx)),
            )
            .into_any_element()
    }

    fn render_divider(&self, b: &Block, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme;
        let id = b.id.clone();
        let picked = self.picked.as_deref() == Some(&id);
        div()
            .id(gpui::SharedString::from(id.clone()))
            .w_full()
            .h(px(page::DIVIDER_HEIGHT * self.scale()))
            .flex()
            .items_center()
            .cursor_pointer()
            .child(
                div()
                    .w_full()
                    .h(px(if picked { 2. } else { 1. }))
                    .bg(if picked { t.accent } else { t.border }),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| this.pick(&id, window, cx)),
            )
            .into_any_element()
    }

    // ---- ink input -------------------------------------------------------

    /// The pen, marker, highlighter or eraser touches the page.
    fn ink_mouse_down(
        &mut self,
        pos: gpui::Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.popover = None;
        window.focus(&self.focus, cx);
        self.picked = None;
        let at = self.to_page(pos);
        if self.tool == InkTool::Eraser {
            self.ink.erasing = true;
            self.ink.eraser_at = Some(pos);
            self.erase_at(at, cx);
        } else {
            self.ink.begin(self.tool, at);
        }
        cx.notify();
    }

    fn ink_mouse_move(&mut self, e: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let pressed = e.pressed_button == Some(MouseButton::Left);
        if self.ink.live.is_some() {
            if !pressed {
                return self.mouse_up(cx);
            }
            let at = self.to_page(e.position);
            self.ink.extend(at);
            cx.notify();
            return;
        }
        if self.tool == InkTool::Eraser {
            self.ink.eraser_at = Some(e.position);
            if self.ink.erasing && pressed {
                let at = self.to_page(e.position);
                self.erase_at(at, cx);
            }
            cx.notify();
        }
    }

    fn erase_at(&mut self, at: [f32; 2], cx: &mut Context<Self>) {
        let r = self.ink.width(InkTool::Eraser);
        if !self.session.erase(at[0], at[1], r).is_empty() {
            self.changed(cx);
        }
    }

    fn mouse_move(&mut self, e: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if e.pressed_button == Some(MouseButton::Left) {
            self.text_mouse_drag(e.position, cx);
        }
    }

    pub fn set_tool(&mut self, tool: InkTool, _: &mut Window, cx: &mut Context<Self>) {
        self.popover = None;
        self.tool = tool;
        if tool.draws() {
            // The keyboard goes nowhere while drawing.
            self.sel = None;
        }
        cx.notify();
    }

    pub fn pick_images(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Insert".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| {
                this.insert_image_files(&paths, window, cx)
            });
        })
        .detach();
    }

    fn insert_image_files(
        &mut self,
        paths: &[std::path::PathBuf],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for p in paths {
            match std::fs::read(p) {
                Ok(bytes) => {
                    let name = p
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    self.insert_image_bytes(&bytes, &name, window, cx);
                }
                Err(e) => eprintln!("snot: could not read {}: {e}", p.display()),
            }
        }
    }

    pub fn export_markdown(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.popover = None;
        self.save(cx);
        let start = dirs::document_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_default();
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Export here".into()),
        });
        let _ = start;
        let id = self.note_id().to_string();
        let lib = self.lib.clone();
        cx.spawn_in(window, async move |_, _| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(dir) = paths.first() else { return };
            match lib.export_markdown(&id, dir) {
                Ok(file) => eprintln!("snot: exported to {}", file.display()),
                Err(e) => eprintln!("snot: export failed: {e}"),
            }
        })
        .detach();
    }

    // ---- toolbar ---------------------------------------------------------

    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let tool = self.tool;
        let tools = [
            (InkTool::Type, "type", "Type (Esc)"),
            (InkTool::Pen, "pen", "Pen"),
            (InkTool::Marker, "marker", "Marker"),
            (InkTool::Highlighter, "highlighter", "Highlighter"),
            (InkTool::Eraser, "eraser", "Eraser"),
        ];
        let mut bar = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(2.))
            .px(px(10.))
            .py(px(6.))
            .min_h(px(48.))
            .border_b_1()
            .border_color(t.border)
            .bg(self.page_color());
        bar = bar
            .child(
                icon_button("undo", "undo", "Undo (Ctrl+Z)", false, &t)
                    .when(!self.session.can_undo(), |d| d.opacity(0.35))
                    .on_click(
                        cx.listener(|this, _, window, cx| this.undo(&super::Undo, window, cx)),
                    ),
            )
            .child(
                icon_button("redo", "redo", "Redo (Ctrl+Shift+Z)", false, &t)
                    .when(!self.session.can_redo(), |d| d.opacity(0.35))
                    .on_click(
                        cx.listener(|this, _, window, cx| this.redo(&super::Redo, window, cx)),
                    ),
            )
            .child(vsep(&t));
        for (tl, name, tip) in tools {
            bar = bar.child(
                icon_button(
                    gpui::SharedString::from(format!("tool-{name}")),
                    name,
                    tip,
                    tool == tl,
                    &t,
                )
                .on_click(cx.listener(move |this, _, window, cx| this.set_tool(tl, window, cx))),
            );
        }
        bar = bar.child(vsep(&t));
        if tool.draws() {
            if tool != InkTool::Eraser {
                let current = self.ink.color(tool).to_string();
                bar = bar.child(
                    swatch("pen-color", argb(color::ink(&current, t.dark)), false, &t)
                        .tooltip(ui::tooltip("Colour"))
                        .on_click(cx.listener(|this, e: &gpui::ClickEvent, _, cx| {
                            this.open_popover(Popover::PenColor, e.position(), cx)
                        })),
                );
                // A few colours a click away; the rest in the popover.
                for token in ["ink", "red", "blue", "green"]
                    .into_iter()
                    .filter(|c| *c != current)
                    .take(3)
                {
                    bar = bar.child(
                        swatch(
                            gpui::SharedString::from(format!("quick-{token}")),
                            argb(color::ink(token, t.dark)),
                            false,
                            &t,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.ink.set_color(this.tool, token);
                            cx.notify();
                        })),
                    );
                }
                bar = bar.child(vsep(&t));
            }
            let width = self.ink.width(tool);
            let widths = tool.widths();
            let max = widths.iter().cloned().fold(1., f32::max);
            for (i, w) in widths.iter().copied().enumerate() {
                let dot = 4. + 12. * (w / max);
                let active = (width - w).abs() < 0.01;
                bar = bar.child(
                    div()
                        .id(gpui::SharedString::from(format!("width-{i}")))
                        .size(px(32.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(7.))
                        .cursor_pointer()
                        .when(active, |d| d.bg(t.accent_soft))
                        .hover(|s| s.bg(t.hover))
                        .child(div().size(px(dot)).rounded_full().bg(if active {
                            t.accent
                        } else {
                            t.text_muted
                        }))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.ink.set_width(this.tool, w);
                            cx.notify();
                        })),
                );
            }
            bar = bar.child(vsep(&t));
            let paper = self.session.ink().paper;
            for (p, name, tip) in [
                (Paper::Plain, "plain", "Plain paper"),
                (Paper::Lined, "lined", "Lined paper"),
                (Paper::Grid, "grid", "Grid paper"),
                (Paper::Dotted, "dotted", "Dotted paper"),
            ] {
                bar = bar.child(
                    icon_button(
                        gpui::SharedString::from(format!("paper-{name}")),
                        name,
                        tip,
                        paper == p,
                        &t,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.set_paper(p, cx))),
                );
            }
            bar = bar.child(
                icon_button("ink-clear", "trash", "Erase all writing", false, &t)
                    .on_click(cx.listener(|this, _, _, cx| this.clear_ink(cx))),
            );
        } else {
            let marks = self.active_marks();
            let has = |m: &Mark| marks.contains(m);
            let style = self.current_style().unwrap_or(ParaStyle::Body);
            for (mark, name, tip) in [
                (Mark::Bold, "bold", "Bold (Ctrl+B)"),
                (Mark::Italic, "italic", "Italic (Ctrl+I)"),
                (Mark::Underline, "underline", "Underline (Ctrl+U)"),
                (Mark::Strike, "strike", "Strikethrough (Ctrl+Shift+X)"),
            ] {
                let m = mark.clone();
                bar = bar.child(
                    icon_button(
                        gpui::SharedString::from(format!("mark-{name}")),
                        name,
                        tip,
                        has(&mark),
                        &t,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_mark(m.clone(), cx))),
                );
            }
            let hl = marks.iter().any(|m| matches!(m, Mark::Highlight(_)));
            let tc = marks.iter().any(|m| matches!(m, Mark::Color(_)));
            bar = bar
                .child(
                    icon_button("mark-highlight", "highlighter", "Highlight", hl, &t).on_click(
                        cx.listener(|this, e: &gpui::ClickEvent, _, cx| {
                            this.open_popover(Popover::Highlight, e.position(), cx)
                        }),
                    ),
                )
                .child(
                    icon_button("mark-color", "palette", "Text colour", tc, &t).on_click(
                        cx.listener(|this, e: &gpui::ClickEvent, _, cx| {
                            this.open_popover(Popover::TextColor, e.position(), cx)
                        }),
                    ),
                )
                .child(
                    icon_button("mark-clear", "close", "Clear formatting", false, &t)
                        .on_click(cx.listener(|this, _, _, cx| this.clear_marks(cx))),
                )
                .child(vsep(&t));
            for (s, name, tip) in [
                (ParaStyle::Heading1, "heading", "Paragraph style"),
                (ParaStyle::Bullet, "bullets", "Bulleted list (Ctrl+Shift+8)"),
                (
                    ParaStyle::Numbered,
                    "numbers",
                    "Numbered list (Ctrl+Shift+7)",
                ),
                (
                    ParaStyle::Checklist,
                    "checklist",
                    "Checklist (Ctrl+Shift+9)",
                ),
            ] {
                let active = style == s
                    || s == ParaStyle::Heading1
                        && matches!(
                            style,
                            ParaStyle::Heading2
                                | ParaStyle::Heading3
                                | ParaStyle::Quote
                                | ParaStyle::Code
                        );
                let mut b = icon_button(
                    gpui::SharedString::from(format!("style-{name}")),
                    name,
                    tip,
                    active,
                    &t,
                );
                b = if s == ParaStyle::Heading1 {
                    b.on_click(cx.listener(|this, e: &gpui::ClickEvent, _, cx| {
                        this.open_popover(Popover::Style, e.position(), cx)
                    }))
                } else {
                    b.on_click(cx.listener(move |this, _, _, cx| this.set_style(s, cx)))
                };
                bar = bar.child(b);
            }
        }
        bar =
            bar.child(vsep(&t))
                .child(
                    icon_button("insert", "plus", "Insert", false, &t).on_click(cx.listener(
                        |this, e: &gpui::ClickEvent, _, cx| {
                            this.open_popover(Popover::Insert, e.position(), cx)
                        },
                    )),
                );
        bar
    }

    fn page_color(&self) -> Hsla {
        match self.color.as_deref() {
            Some(c) => argb(color::note_background(Some(c), self.theme.dark)),
            None => self.theme.page_bg,
        }
    }

    fn trash_banner(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        div()
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(16.))
            .min_h(px(48.))
            .border_b_1()
            .border_color(t.border)
            .bg(t.accent_soft)
            .text_size(px(14.))
            .child(icon("trash", 16., t.text_muted))
            .child(
                div()
                    .flex_1()
                    .text_color(t.text_muted)
                    .child("This note is in the trash."),
            )
            .child(
                ui::menu_item("restore-note", Some("restore"), "Restore", &t)
                    .min_w(px(0.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if this.lib.restore_note(this.note_id()).is_ok() {
                            this.trashed = false;
                            cx.emit(NoteEvent::Saved);
                            cx.notify();
                        }
                    })),
            )
    }

    fn note_actions(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        div()
            .flex()
            .items_center()
            .gap(px(2.))
            .child(
                icon_button(
                    "favorite",
                    if self.favorite { "star-filled" } else { "star" },
                    "Favourite",
                    self.favorite,
                    &t,
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_favorite(cx))),
            )
            .child(
                icon_button("pin", "pin", "Pin to top", self.pinned, &t)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_pinned(cx))),
            )
            .child(
                icon_button("note-color", "palette", "Note colour", false, &t).on_click(
                    cx.listener(|this, e: &gpui::ClickEvent, _, cx| {
                        this.open_popover(Popover::NoteColor, e.position(), cx)
                    }),
                ),
            )
            .child(
                icon_button("note-more", "more", "More", false, &t).on_click(cx.listener(
                    |this, e: &gpui::ClickEvent, _, cx| {
                        this.open_popover(Popover::More, e.position(), cx)
                    },
                )),
            )
    }

    fn render_popover(
        &self,
        which: Popover,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = self.theme;
        let mut p = panel(&t);
        match which {
            Popover::PenColor => {
                let current = self.ink.color(self.tool).to_string();
                let mut grid = div()
                    .flex()
                    .flex_wrap()
                    .w(px(5. * 30.))
                    .gap(px(4.))
                    .p(px(4.));
                let tokens: Vec<&str> = if self.tool == InkTool::Highlighter {
                    color::HIGHLIGHTS.to_vec()
                } else {
                    color::TOKENS.to_vec()
                };
                for token in tokens {
                    grid = grid.child(
                        swatch(
                            gpui::SharedString::from(format!("pen-{token}")),
                            argb(color::ink(token, t.dark)),
                            token == current,
                            &t,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.ink.set_color(this.tool, token);
                            this.popover = None;
                            cx.notify();
                        })),
                    );
                }
                p = p.child(grid);
            }
            Popover::Highlight | Popover::TextColor => {
                let highlight = which == Popover::Highlight;
                let tokens: Vec<&str> = if highlight {
                    color::HIGHLIGHTS.to_vec()
                } else {
                    color::TOKENS.to_vec()
                };
                let marks = self.active_marks();
                let mut grid = div()
                    .flex()
                    .flex_wrap()
                    .w(px(5. * 30.))
                    .gap(px(4.))
                    .p(px(4.));
                for token in tokens {
                    let mark = if highlight {
                        Mark::Highlight(token.into())
                    } else {
                        Mark::Color(token.into())
                    };
                    let c = if highlight {
                        color::highlight(token, t.dark)
                    } else {
                        color::ink(token, t.dark)
                    };
                    let on = marks.contains(&mark);
                    grid = grid.child(
                        swatch(
                            gpui::SharedString::from(format!("mark-{token}")),
                            argb(c),
                            on,
                            &t,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.toggle_mark(mark.clone(), cx);
                            this.popover = None;
                        })),
                    );
                }
                p = p.child(grid).child(separator(&t)).child(
                    menu_item(
                        "mark-none",
                        Some("close"),
                        if highlight {
                            "No highlight"
                        } else {
                            "Default colour"
                        },
                        &t,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let marks = this.active_marks();
                        if let Some(m) = marks.into_iter().find(|m| {
                            matches!(
                                (m, highlight),
                                (Mark::Highlight(_), true) | (Mark::Color(_), false)
                            )
                        }) {
                            this.toggle_mark(m, cx);
                        }
                        this.popover = None;
                        cx.notify();
                    })),
                );
            }
            Popover::Style => {
                for (s, label) in [
                    (ParaStyle::Body, "Body text"),
                    (ParaStyle::Heading1, "Heading 1"),
                    (ParaStyle::Heading2, "Heading 2"),
                    (ParaStyle::Heading3, "Heading 3"),
                    (ParaStyle::Quote, "Quote"),
                    (ParaStyle::Code, "Code"),
                ] {
                    let current = self.current_style() == Some(s);
                    p = p.child(
                        menu_item(
                            gpui::SharedString::from(format!("style-{label}")),
                            current.then_some("check"),
                            label,
                            &t,
                        )
                        .when(!current, |d| d.pl(px(36.)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if this.current_style() != Some(s) {
                                let target = if s == ParaStyle::Body {
                                    this.current_style().unwrap_or(s)
                                } else {
                                    s
                                };
                                this.set_style(target, cx);
                            }
                            this.popover = None;
                            cx.notify();
                        })),
                    );
                }
            }
            Popover::Insert => {
                let close = |this: &mut Self| this.popover = None;
                p = p
                    .child(
                        menu_item("insert-image", Some("image"), "Picture…", &t).on_click(
                            cx.listener(move |this, _, window, cx| {
                                close(this);
                                this.pick_images(window, cx)
                            }),
                        ),
                    )
                    .child(
                        menu_item("insert-divider", Some("divider"), "Divider", &t).on_click(
                            cx.listener(move |this, _, window, cx| {
                                close(this);
                                this.insert_divider(window, cx)
                            }),
                        ),
                    );
            }
            Popover::NoteColor => {
                let mut grid = div()
                    .flex()
                    .flex_wrap()
                    .w(px(4. * 30.))
                    .gap(px(4.))
                    .p(px(4.));
                grid = grid.child(
                    swatch(
                        "note-none",
                        argb(color::note_background(None, t.dark)),
                        self.color.is_none(),
                        &t,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.set_note_color(None, cx))),
                );
                for token in color::NOTE_COLORS {
                    grid = grid.child(
                        swatch(
                            gpui::SharedString::from(format!("note-{token}")),
                            argb(color::note_background(Some(token), t.dark)),
                            self.color.as_deref() == Some(token),
                            &t,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.set_note_color(Some(token.into()), cx)
                        })),
                    );
                }
                p = p.child(grid);
            }
            Popover::MoveTo => {
                p = p.child(
                    menu_item("move-none", Some("inbox"), "No folder", &t)
                        .when(self.folder.is_none(), |d| d.text_color(t.accent))
                        .on_click(cx.listener(|this, _, _, cx| this.move_to_folder(None, cx))),
                );
                for f in self.lib.folders().unwrap_or_default() {
                    let fid = f.id.clone();
                    p = p.child(
                        menu_item(
                            gpui::SharedString::from(format!("move-{}", f.id)),
                            Some("folder"),
                            f.name.clone(),
                            &t,
                        )
                        .when(self.folder.as_deref() == Some(&f.id), |d| {
                            d.text_color(t.accent)
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.move_to_folder(Some(fid.clone()), cx)
                        })),
                    );
                }
            }
            Popover::More => {
                p = p
                    .child(
                        menu_item("more-move", Some("move"), "Move to folder…", &t).on_click(
                            cx.listener(|this, e: &gpui::ClickEvent, _, cx| {
                                this.popover = Some((Popover::MoveTo, e.position()));
                                cx.notify();
                            }),
                        ),
                    )
                    .child(
                        menu_item("more-duplicate", Some("copy"), "Duplicate", &t).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.popover = None;
                                this.save(cx);
                                if let Ok(copy) = this.lib.duplicate_note(this.note_id()) {
                                    cx.emit(NoteEvent::Open(copy.id));
                                }
                            }),
                        ),
                    )
                    .child(
                        menu_item("more-export", Some("export"), "Export as Markdown…", &t)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.export_markdown(window, cx)),
                            ),
                    )
                    .child(separator(&t))
                    .child(
                        menu_item("more-trash", Some("trash"), "Move to trash", &t)
                            .text_color(t.danger)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.popover = None;
                                this.save(cx);
                                if this.lib.trash_note(this.note_id()).is_ok() {
                                    cx.emit(NoteEvent::Closed);
                                }
                            })),
                    );
            }
        }
        let (_, at) = self.popover.unwrap_or((which, point(px(0.), px(0.))));
        ui::overlay(
            point(at.x - px(16.), at.y + px(18.)),
            window.viewport_size(),
            p.into_any_element(),
            {
                let this = cx.entity().downgrade();
                move |_, cx| {
                    let _ = this.update(cx, |this, cx| {
                        this.popover = None;
                        cx.notify();
                    });
                }
            },
        )
        .into_any_element()
    }
}

fn vsep(t: &Theme) -> impl IntoElement {
    div().w(px(1.)).h(px(20.)).mx(px(6.)).bg(t.border)
}

fn paint_selection(
    layout: &TextLayout,
    sel: &Selection,
    caret: Hsla,
    selection: Hsla,
    window: &mut Window,
) {
    let lh = layout.line_height();
    let b = layout.bounds();
    if sel.is_empty() {
        let p = layout.position_for_index(sel.head).unwrap_or(b.origin);
        window.paint_quad(gpui::fill(Bounds::new(p, size(px(2.), lh)), caret));
        return;
    }
    let r = sel.range();
    let (Some(a), Some(z)) = (
        layout.position_for_index(r.start),
        layout.position_for_index(r.end),
    ) else {
        return;
    };
    let mut y = a.y;
    while y <= z.y {
        let x0 = if y == a.y { a.x } else { b.left() };
        let x1 = if y == z.y { z.x } else { b.right() };
        if x1 > x0 {
            window.paint_quad(gpui::fill(
                Bounds::from_corners(point(x0, y), point(x1, y + lh)),
                selection,
            ));
        }
        y += lh;
    }
}

/// What the blocks show, besides their text. While neither changes the
/// blocks are not laid out again.
#[derive(Clone, PartialEq)]
struct BlocksKey {
    revision: u64,
    sel: Option<Selection>,
    marked: Option<Range<usize>>,
    picked: Option<String>,
    focused: bool,
    dark: bool,
    scale: f32,
}

/// The blocks of the note as a view of their own. GPUI keeps a view's last
/// frame until the view is notified, and this one is only notified when
/// what it shows has changed, so moving the pen repaints the handwriting
/// alone: a stroke over a long note costs the same as one over an empty
/// page.
pub(crate) struct Blocks {
    note: WeakEntity<NoteView>,
    key: Option<BlocksKey>,
    _sub: Subscription,
}

impl Blocks {
    fn new(note: &Entity<NoteView>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Blocks {
            note: note.downgrade(),
            key: None,
            _sub: cx.observe_in(note, window, |this, note, window, cx| {
                if this.key.as_ref() != Some(&note.read(cx).blocks_key(window)) {
                    cx.notify();
                }
            }),
        }
    }
}

impl Render for Blocks {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Ok((key, el)) = self.note.update(cx, |note, cx| {
            (note.blocks_key(window), note.render_blocks(window, cx))
        }) else {
            return div().into_any_element();
        };
        self.key = Some(key);
        el
    }
}

impl NoteView {
    fn blocks_key(&self, window: &Window) -> BlocksKey {
        BlocksKey {
            revision: self.session.blocks_revision(),
            sel: self.sel.clone(),
            marked: self.marked.clone(),
            picked: self.picked.clone(),
            focused: self.focus.is_focused(window),
            dark: self.theme.dark,
            scale: self.scale(),
        }
    }

    fn render_blocks(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        {
            let mut g = self.geometry.borrow_mut();
            g.ready.clear();
            g.text.clear();
            g.rects.clear();
        }
        let focused = self.focus.is_focused(window);
        let numbers = self.session.doc().list_numbers();
        let blocks = self.session.doc().blocks.clone();
        let z = self.scale();
        let page_w = px(PAGE_WIDTH * z);

        let mut items: Vec<AnyElement> = Vec::with_capacity(blocks.len() + 2);
        let (geometry, scroll) = (self.geometry.clone(), self.scroll.clone());
        let top = Rc::new(Cell::new(px(0.)));
        let start = top.clone();
        items.push(
            canvas(move |b, _, _| start.set(b.top()), |_, _, _, _| {})
                .h(px(0.))
                .into_any_element(),
        );
        for (i, b) in blocks.iter().enumerate() {
            let el = match &b.content {
                Content::Text(tb) => {
                    self.render_text(b, tb, numbers.get(i).copied().flatten(), focused, cx)
                }
                Content::Image(im) => self.render_image(b, im, cx),
                Content::Divider => self.render_divider(b, cx),
            };
            let (geometry, scroll, id) = (geometry.clone(), scroll.clone(), b.id.clone());
            let record = canvas(
                move |b, _, _| {
                    let content = scroll.bounds().top() + scroll.offset().y;
                    geometry
                        .borrow_mut()
                        .rects
                        .insert(id, (b.top() - content, b.bottom() - content));
                },
                |_, _, _, _| {},
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full();
            items.push(
                column(page_w, z)
                    .relative()
                    .child(el)
                    .child(record)
                    .into_any_element(),
            );
        }
        // Once all are laid out: their height, for the room left for them,
        // and whether the block to reveal is in view.
        let note = cx.weak_entity();
        items.push(
            canvas(
                move |b, window, _| {
                    let mut g = geometry.borrow_mut();
                    let height = b.top() - top.get();
                    let mut again = g.blocks_height != height;
                    g.blocks_height = height;
                    if let Some(id) = g.recheck.take()
                        && let Some(&rect) = g.rects.get(&id)
                    {
                        again |= bring_into_view(&scroll, rect);
                    }
                    if again {
                        window.on_next_frame(move |_, cx| {
                            note.update(cx, |_, cx| cx.notify()).ok();
                        });
                    }
                },
                |_, _, _, _| {},
            )
            .h(px(0.))
            .into_any_element(),
        );
        div()
            .w(page_w)
            .flex()
            .flex_col()
            .children(items)
            .into_any_element()
    }

    /// Scrolls the block asked for into view, from where it was laid out
    /// last; the blocks look again once laid out, in case it has moved or
    /// is new.
    fn reveal(&self) {
        let mut g = self.geometry.borrow_mut();
        g.recheck = g.reveal.take();
        if let Some(id) = &g.recheck
            && let Some(&rect) = g.rects.get(id)
        {
            bring_into_view(&self.scroll, rect);
        }
    }
}

/// Pixels per page unit for a page shown in `scroll`, as it was last laid
/// out.
fn scale_in(scroll: &ScrollHandle) -> f32 {
    let width = f32::from(scroll.bounds().size.width);
    let width = if width > 0. { width } else { 900. };
    let fit = (width - 2. * GUTTER) / PAGE_WIDTH;
    (BASE_SCALE * ZOOM.get()).min(fit).max(0.1)
}

/// Scrolls the least distance that shows all of a block, or its top if it
/// is taller than the view. True when it scrolled.
fn bring_into_view(scroll: &ScrollHandle, (top, bottom): (Pixels, Pixels)) -> bool {
    let offset = scroll.offset();
    let height = scroll.bounds().size.height;
    let view_top = -offset.y;
    let to = if top < view_top {
        top
    } else if bottom > view_top + height {
        top.min(bottom - height)
    } else {
        return false;
    };
    scroll.set_offset(point(offset.x, -to));
    true
}

impl Render for NoteView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let page = self.page_color();
        self.reveal();

        let z = self.scale();
        let page_w = px(PAGE_WIDTH * z);
        let column = || column(page_w, z);

        let mut items: Vec<AnyElement> = Vec::with_capacity(5);
        items.push(
            column()
                .pt(px(36.))
                .pb(px(10.))
                .child(
                    div()
                        .text_size(px(30.))
                        .line_height(px(42.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(self.title.clone()),
                )
                .child(
                    div()
                        .mt(px(2.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(12.5))
                                .text_color(t.text_faint)
                                .child(ui::date_label(self.updated_at)),
                        )
                        .child(self.note_actions(cx)),
                )
                .into_any_element(),
        );
        // The top of the page: where it starts, and the paper under the
        // text from there down.
        let geometry = self.geometry.clone();
        let paper = self.session.ink().paper;
        let (scroll, note) = (self.scroll.clone(), cx.weak_entity());
        items.push(
            canvas(
                move |bounds, window, _| {
                    geometry.borrow_mut().page = bounds.origin;
                    // The zoom was worked out from the width last laid out;
                    // if the window has since been resized, lay out again.
                    if scale_in(&scroll) != z {
                        window.on_next_frame(move |_, cx| {
                            note.update(cx, |_, cx| cx.notify()).ok();
                        });
                    }
                },
                move |bounds, _, window, _| {
                    ink::paint_paper(bounds.origin, bounds.size.width, z, paper, &t, window)
                },
            )
            .w(page_w)
            .h(px(0.))
            .flex_none()
            .into_any_element(),
        );
        let blocks = self
            .blocks
            .get_or_insert_with(|| {
                let note = cx.entity();
                cx.new(|cx| Blocks::new(&note, window, cx))
            })
            .clone();
        // Laid out at no height, so its place stays put as it grows and it
        // need not be redone for that; the room it takes is measured the
        // frame it is laid out, and left for it here.
        let height = self.geometry.borrow().blocks_height;
        items.push(
            div()
                .w(page_w)
                .h(height)
                .flex_none()
                .child(blocks.cached(gpui::StyleRefinement::default().w(page_w).h(px(0.))))
                .into_any_element(),
        );
        // The page runs on past the last line and the lowest writing, so
        // there is always room; clicking there types at the end.
        let (page_top, blocks_bottom) = {
            let g = self.geometry.borrow();
            (g.page.y, g.blocks_bottom)
        };
        let ink_bottom = self.session.ink().content_bottom() + page::ROOM;
        let room = (ink_bottom * z - f32::from(blocks_bottom - page_top)).max(page::ROOM * z);
        let geometry = self.geometry.clone();
        items.push(
            column()
                .id("note-tail")
                .h(px(room))
                .cursor(CursorStyle::IBeam)
                .child(
                    canvas(
                        move |bounds, _, _| geometry.borrow_mut().blocks_bottom = bounds.top(),
                        |_, _, _, _| {},
                    )
                    .size_full(),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        let last = this
                            .session
                            .doc()
                            .blocks
                            .last()
                            .filter(|b| b.as_text().is_some())
                            .map(|b| b.id.clone());
                        if let Some(id) = last {
                            let len = this.session.text(&id).map(|t| t.text.len()).unwrap_or(0);
                            this.sel = Some(Selection::caret(id, len));
                            this.picked = None;
                            window.focus(&this.focus, cx);
                            cx.notify();
                        }
                    }),
                )
                .into_any_element(),
        );
        // The handwriting, over everything on the page.
        self.ink.tiles.borrow_mut().sync(
            self.session.ink(),
            self.session.ink_revision(),
            z * window.scale_factor(),
            t.dark,
        );
        let tiles = self.ink.tiles.clone();
        let live = self.ink.live_painted(z, t.dark);
        let eraser = (self.tool == InkTool::Eraser)
            .then_some(self.ink.eraser_at)
            .flatten()
            .map(|at| (at, px(self.ink.width(InkTool::Eraser) * z)));
        let geometry = self.geometry.clone();
        items.push(
            canvas(
                |_, _, _| {},
                move |_, _, window, _| {
                    let origin = geometry.borrow().page;
                    ink::paint_ink(origin, &mut tiles.borrow_mut(), live, window);
                    if let Some((at, r)) = eraser {
                        window.paint_quad(
                            gpui::outline(
                                Bounds::new(point(at.x - r, at.y - r), size(r * 2., r * 2.)),
                                t.text_muted,
                                gpui::BorderStyle::Solid,
                            )
                            .corner_radii(r),
                        );
                    }
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .into_any_element(),
        );

        let entity = cx.entity();
        let focus = self.focus.clone();
        let input = canvas(
            |_, _, _| {},
            move |bounds, _, window, cx| {
                window.handle_input(&focus, ElementInputHandler::new(bounds, entity), cx);
            },
        )
        .absolute()
        .size_full();

        let trashed = self.trashed;
        let top: AnyElement = if trashed {
            self.trash_banner(cx).into_any_element()
        } else {
            self.toolbar(cx).into_any_element()
        };
        div()
            .id("note-editor")
            .when(!trashed, |d| {
                d.key_context(super::CONTEXT).track_focus(&self.focus)
            })
            .size_full()
            .flex()
            .flex_col()
            .bg(page)
            .text_color(t.text)
            .font_family(UI_FONT)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::enter))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::word_left))
            .on_action(cx.listener(Self::word_right))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::select_home))
            .on_action(cx.listener(Self::select_end))
            .on_action(cx.listener(Self::doc_start))
            .on_action(cx.listener(Self::doc_end))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::bold))
            .on_action(cx.listener(Self::italic))
            .on_action(cx.listener(Self::underline))
            .on_action(cx.listener(Self::strike))
            .on_action(cx.listener(Self::indent))
            .on_action(cx.listener(Self::outdent))
            .on_action(cx.listener(Self::heading1))
            .on_action(cx.listener(Self::heading2))
            .on_action(cx.listener(Self::heading3))
            .on_action(cx.listener(Self::body_text))
            .on_action(cx.listener(Self::bullet_list))
            .on_action(cx.listener(Self::numbered_list))
            .on_action(cx.listener(Self::checklist))
            .on_action(cx.listener(Self::toggle_check))
            .on_action(cx.listener(Self::escape))
            .on_action(cx.listener(Self::zoom_in))
            .on_action(cx.listener(Self::zoom_out))
            .on_action(cx.listener(Self::zoom_reset))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.mouse_up(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.mouse_up(cx)),
            )
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                this.insert_image_files(paths.paths(), window, cx)
            }))
            .child(top)
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .when(!trashed, |d| d.child(input))
                    // A note in the trash can be read and scrolled, not changed.
                    .when(trashed, |d| {
                        d.child(
                            div()
                                .id("trash-shield")
                                .absolute()
                                .size_full()
                                .occlude()
                                .block_mouse_except_scroll(),
                        )
                    })
                    .child(
                        div()
                            .id("note-scroll")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll)
                            .flex()
                            .flex_col()
                            .items_center()
                            .children(items),
                    )
                    // A drawing tool takes the mouse everywhere on the page.
                    .when(!trashed && self.tool.draws(), |d| {
                        d.child(
                            div()
                                .id("ink-input")
                                .absolute()
                                .top_0()
                                .left_0()
                                .size_full()
                                .cursor(CursorStyle::Crosshair)
                                .block_mouse_except_scroll()
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, e: &MouseDownEvent, window, cx| {
                                        this.ink_mouse_down(e.position, window, cx)
                                    }),
                                )
                                .on_mouse_move(cx.listener(Self::ink_mouse_move))
                                .on_mouse_up(
                                    MouseButton::Left,
                                    cx.listener(|this, _, _, cx| this.mouse_up(cx)),
                                )
                                .on_mouse_up_out(
                                    MouseButton::Left,
                                    cx.listener(|this, _, _, cx| this.mouse_up(cx)),
                                ),
                        )
                    }),
            )
            .children(
                self.popover
                    .map(|(p, _)| self.render_popover(p, window, cx)),
            )
    }
}

/// One row of the page: its width, less the margins.
fn column(width: Pixels, z: f32) -> gpui::Div {
    div().w(width).px(px(page::MARGIN * z)).flex_none()
}
