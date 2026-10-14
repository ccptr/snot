//! Drawing the note: toolbar, title, and each block.

use std::ops::Range;

use gpui::{
    AnyElement, Bounds, Context, CursorStyle, ElementInputHandler, ExternalPaths, Font, FontStyle,
    FontWeight, Hsla, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    ParentElement, PathPromptOptions, Pixels, Render, StatefulInteractiveElement,
    StrikethroughStyle, Styled, StyledText, TextLayout, TextRun, UnderlineStyle, Window, canvas,
    div, img, point, prelude::FluentBuilder, px, size,
};
use snot_core::doc::{Block, Content, TextBlock};
use snot_core::ink::INK_WIDTH;
use snot_core::{Mark, Paper, ParaStyle, color};

use super::ink::{self, InkTool};
use super::{NoteEvent, NoteView, Popover, Selection};
use crate::assets::{MONO_FONT, UI_FONT};
use crate::theme::{Theme, argb};
use crate::ui::{self, icon, icon_button, menu_item, panel, separator, swatch};

/// Widest the page grows to; wider windows get margins.
const PAGE_WIDTH: f32 = 780.;
const PAGE_PAD: f32 = 44.;
const PREFIX: f32 = 28.;

struct TextLook {
    size: f32,
    weight: FontWeight,
    mono: bool,
    color: Hsla,
    top: f32,
    bottom: f32,
}

fn look(tb: &TextBlock, t: &Theme) -> TextLook {
    let base = TextLook {
        size: 16.,
        weight: FontWeight::NORMAL,
        mono: false,
        color: t.text,
        top: 3.,
        bottom: 3.,
    };
    match tb.style {
        ParaStyle::Heading1 => TextLook {
            size: 27.,
            weight: FontWeight::SEMIBOLD,
            top: 18.,
            bottom: 6.,
            ..base
        },
        ParaStyle::Heading2 => TextLook {
            size: 21.5,
            weight: FontWeight::SEMIBOLD,
            top: 14.,
            bottom: 4.,
            ..base
        },
        ParaStyle::Heading3 => TextLook {
            size: 18.,
            weight: FontWeight::SEMIBOLD,
            top: 10.,
            bottom: 3.,
            ..base
        },
        ParaStyle::Code => TextLook {
            size: 14.5,
            mono: true,
            ..base
        },
        ParaStyle::Quote => TextLook {
            color: t.text_muted,
            ..base
        },
        ParaStyle::Checklist if tb.checked => TextLook {
            color: t.text_faint,
            ..base
        },
        _ => base,
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

impl NoteView {
    fn render_text(
        &self,
        b: &Block,
        tb: &TextBlock,
        number: Option<u32>,
        focused: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = self.theme;
        let l = look(tb, &t);
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
                    .text_color(t.text_muted)
                    .child(if tb.indent.is_multiple_of(2) {
                        "•"
                    } else {
                        "◦"
                    })
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
                        .mt(px(l.size * 0.75 - 9.))
                        .size(px(18.))
                        .rounded(px(5.))
                        .border_2()
                        .border_color(if checked { t.accent } else { t.text_faint })
                        .when(checked, |d| d.bg(t.accent))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .when(checked, |d| d.child(icon("check", 13., t.accent_text)))
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
            .pl(px(PREFIX * tb.indent as f32))
            .cursor(CursorStyle::IBeam)
            .text_size(px(l.size))
            .line_height(px((l.size * 1.5).round()))
            .font_weight(l.weight)
            .when(l.mono, |d| d.font_family(MONO_FONT))
            .text_color(l.color)
            .children(prefix.map(|p| {
                div()
                    .w(px(PREFIX))
                    .flex_none()
                    .flex()
                    .justify_center()
                    .child(p)
            }))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .when(is_code, |d| {
                        d.bg(t.code_bg).rounded(px(6.)).px(px(10.)).py(px(4.))
                    })
                    .when(is_quote, |d| {
                        d.border_l_4().border_color(t.quote_bar).pl(px(14.))
                    })
                    .child(div().relative().w_full().child(styled).child(overlay)),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                    if this.tool.draws() {
                        this.tool = InkTool::Type;
                    }
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

    fn render_ink(
        &self,
        b: &Block,
        block: &snot_core::InkBlock,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = self.theme;
        let id = b.id.clone();
        let painted = self
            .ink
            .painted(&id, self.session.revision(&id), block, t.dark);
        let live = self.ink.live_painted(&id, t.dark);
        let eraser = (self.tool == InkTool::Eraser)
            .then_some(self.ink.eraser_at)
            .flatten();
        let eraser_r = self.ink.width(InkTool::Eraser);
        let block_c = block.clone();
        let geometry = self.geometry.clone();
        let gid = id.clone();
        let picked = self.picked.as_deref() == Some(&id);
        let drawing = self.tool.draws();
        let surface = canvas(
            move |bounds, _, _| {
                geometry.borrow_mut().ink.insert(gid, bounds);
                bounds
            },
            move |_, bounds, window, _| {
                ink::paint(bounds, &block_c, &painted, live.as_ref(), &t, window);
                if let Some(at) = eraser
                    && bounds.contains(&at)
                {
                    let r = px(eraser_r * ink::scale(bounds));
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
        .w_full()
        .aspect_ratio(INK_WIDTH / block.height.max(1.));

        let group = gpui::SharedString::from(format!("ink-{id}"));
        let down_id = id.clone();
        div()
            .id(gpui::SharedString::from(id.clone()))
            .group(group.clone())
            .relative()
            .w_full()
            .my(px(8.))
            .rounded(px(10.))
            .border_1()
            .border_color(if picked { t.accent } else { t.border })
            .when(picked, |d| d.border_2())
            .overflow_hidden()
            .cursor(if drawing {
                CursorStyle::Crosshair
            } else {
                CursorStyle::PointingHand
            })
            .child(surface)
            .child(self.ink_bar(&id, block.paper, picked, group, cx))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                    this.ink_mouse_down(&down_id, e.position, window, cx)
                }),
            )
            .into_any_element()
    }

    fn ink_bar(
        &self,
        id: &str,
        paper: Paper,
        picked: bool,
        group: gpui::SharedString,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = self.theme;
        let papers = [
            (Paper::Plain, "plain", "Plain paper"),
            (Paper::Lined, "lined", "Lined paper"),
            (Paper::Grid, "grid", "Grid paper"),
            (Paper::Dotted, "dotted", "Dotted paper"),
        ];
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
            .cursor(CursorStyle::Arrow)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        if !picked {
            bar = bar.invisible().group_hover(group, |s| s.visible());
        }
        for (p, name, tip) in papers {
            let bid = id.to_string();
            bar = bar.child(
                icon_button(
                    gpui::SharedString::from(format!("paper-{name}-{id}")),
                    name,
                    tip,
                    paper == p,
                    &t,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.set_paper(&bid, p, cx))),
            );
        }
        let (a, b, c, d, e) = (
            id.to_string(),
            id.to_string(),
            id.to_string(),
            id.to_string(),
            id.to_string(),
        );
        bar.child(div().w(px(1.)).my(px(6.)).bg(t.border))
            .child(
                icon_button(
                    gpui::SharedString::from(format!("ink-extend-{id}")),
                    "expand",
                    "More room",
                    false,
                    &t,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.extend_ink(&a, cx))),
            )
            .child(
                icon_button(
                    gpui::SharedString::from(format!("ink-up-{id}")),
                    "arrow-up",
                    "Move up",
                    false,
                    &t,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.move_block(&b, -1, cx))),
            )
            .child(
                icon_button(
                    gpui::SharedString::from(format!("ink-down-{id}")),
                    "arrow-down",
                    "Move down",
                    false,
                    &t,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.move_block(&c, 1, cx))),
            )
            .child(
                icon_button(
                    gpui::SharedString::from(format!("ink-clear-{id}")),
                    "eraser",
                    "Erase all",
                    false,
                    &t,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.clear_ink(&d, cx))),
            )
            .child(
                icon_button(
                    gpui::SharedString::from(format!("ink-delete-{id}")),
                    "trash",
                    "Delete drawing",
                    false,
                    &t,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.delete_block(&e, cx))),
            )
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
        div()
            .id(gpui::SharedString::from(id.clone()))
            .group(group)
            .relative()
            .w_full()
            .my(px(8.))
            .flex()
            .flex_col()
            .gap(px(6.))
            .cursor_pointer()
            .child(
                div()
                    .w_full()
                    .rounded(px(10.))
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
                        .text_size(px(13.5))
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
            .h(px(28.))
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

    fn ink_mouse_down(
        &mut self,
        id: &str,
        pos: gpui::Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.popover = None;
        if !self.tool.draws() {
            return self.pick(id, window, cx);
        }
        window.focus(&self.focus, cx);
        let Some(bounds) = self.geometry.borrow().ink.get(id).copied() else {
            return;
        };
        self.picked = None;
        let at = ink::to_ink(bounds, pos);
        if self.tool == InkTool::Eraser {
            self.ink.erasing = Some(id.to_string());
            self.ink.eraser_at = Some(pos);
            if self
                .session
                .erase(id, at[0], at[1], self.ink.width(InkTool::Eraser))
                > 0
            {
                self.changed(cx);
            }
        } else {
            self.ink.begin(id, self.tool, at);
        }
        cx.notify();
    }

    fn mouse_move(&mut self, e: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let pressed = e.pressed_button == Some(MouseButton::Left);
        if let Some(block) = self.ink.live.as_ref().map(|l| l.block.clone()) {
            if !pressed {
                return self.mouse_up(cx);
            }
            if let Some(bounds) = self.geometry.borrow().ink.get(&block).copied() {
                self.ink.extend(ink::to_ink(bounds, e.position));
            }
            cx.notify();
            return;
        }
        if self.tool == InkTool::Eraser {
            self.ink.eraser_at = Some(e.position);
            let target = self.ink.erasing.clone().filter(|_| pressed);
            let bounds = target
                .as_ref()
                .and_then(|b| self.geometry.borrow().ink.get(b).copied());
            if let (Some(block), Some(bounds)) = (target, bounds) {
                let at = ink::to_ink(bounds, e.position);
                if self
                    .session
                    .erase(&block, at[0], at[1], self.ink.width(InkTool::Eraser))
                    > 0
                {
                    self.changed(cx);
                }
            }
            cx.notify();
            return;
        }
        if pressed {
            self.text_mouse_drag(e.position, cx);
        }
    }

    /// Choosing a pen in a note with nowhere to draw adds a drawing.
    pub fn set_tool(&mut self, tool: InkTool, window: &mut Window, cx: &mut Context<Self>) {
        self.popover = None;
        if tool.draws() && tool != InkTool::Eraser && !self.session.doc().has_ink() {
            self.insert_drawing(window, cx);
        }
        self.tool = tool;
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
                        menu_item("insert-drawing", Some("drawing"), "Drawing", &t).on_click(
                            cx.listener(move |this, _, window, cx| {
                                close(this);
                                this.insert_drawing(window, cx)
                            }),
                        ),
                    )
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

impl Render for NoteView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        {
            let mut g = self.geometry.borrow_mut();
            g.ready.clear();
            g.text.clear();
        }
        let focused = self.focus.is_focused(window);
        let numbers = self.session.doc().list_numbers();
        let blocks = self.session.doc().blocks.clone();
        let page = self.page_color();

        let mut items: Vec<AnyElement> = Vec::with_capacity(blocks.len() + 2);
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
        for (i, b) in blocks.iter().enumerate() {
            let el = match &b.content {
                Content::Text(tb) => {
                    self.render_text(b, tb, numbers.get(i).copied().flatten(), focused, cx)
                }
                Content::Ink(ink) => self.render_ink(b, ink, cx),
                Content::Image(im) => self.render_image(b, im, cx),
                Content::Divider => self.render_divider(b, cx),
            };
            items.push(column().child(el).into_any_element());
        }
        // Clicking below the last line puts the caret at its end.
        items.push(
            column()
                .id("note-tail")
                .h(px(240.))
                .cursor(CursorStyle::IBeam)
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
                            this.tool = InkTool::Type;
                            this.sel = Some(Selection::caret(id, len));
                            this.picked = None;
                            window.focus(&this.focus, cx);
                            cx.notify();
                        }
                    }),
                )
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
                    ),
            )
            .children(
                self.popover
                    .map(|(p, _)| self.render_popover(p, window, cx)),
            )
    }
}

/// One row of the page, centred at reading width.
fn column() -> gpui::Div {
    div()
        .w_full()
        .max_w(px(PAGE_WIDTH))
        .px(px(PAGE_PAD))
        .flex_none()
}
