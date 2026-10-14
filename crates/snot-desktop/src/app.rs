//! The window: folders on the left, the note list, and the open note.

use std::sync::Arc;

use gpui::{
    AnyElement, AppContext, ClickEvent, Context, Entity, FocusHandle, Focusable, FontWeight,
    HighlightStyle, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    Pixels, Point, Render, ScrollHandle, SharedString, StatefulInteractiveElement, Styled,
    StyledText, Subscription, Window, actions, div, point, prelude::FluentBuilder, px,
};
use snot_core::note::{HIT_END, HIT_START};
use snot_core::{Counts, Folder, Library, NoteSummary, Scope, SortBy, color};

use crate::editor::{NoteEvent, NoteView};
use crate::text_field::{FieldEvent, TextField};
use crate::theme::{Theme, argb};
use crate::ui::{self, icon, icon_button, menu_item, panel, separator, swatch};

actions!(
    snot,
    [
        NewNote,
        NewHandwriting,
        FocusSearch,
        SaveNow,
        Quit,
        NewFolder
    ]
);

pub const CONTEXT: &str = "SnotApp";

#[derive(Clone, PartialEq)]
enum Menu {
    Note(String),
    Folder(String),
    Sort,
}

enum Dialog {
    NewFolder(Entity<TextField>),
    RenameFolder(String, Entity<TextField>),
    EmptyTrash,
    DeleteNote(String),
    DeleteFolder(String),
}

pub struct SnotApp {
    lib: Arc<Library>,
    theme: Theme,
    scope: Scope,
    sort: SortBy,
    folders: Vec<Folder>,
    counts: Counts,
    notes: Vec<NoteSummary>,
    search: Entity<TextField>,
    query: String,
    editor: Option<(String, Entity<NoteView>)>,
    menu: Option<(Menu, Point<Pixels>)>,
    dialog: Option<Dialog>,
    list_scroll: ScrollHandle,
    focus: FocusHandle,
    _subs: Vec<Subscription>,
    editor_sub: Option<Subscription>,
}

impl Focusable for SnotApp {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl SnotApp {
    pub fn new(lib: Arc<Library>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| TextField::new("Search notes", cx));
        let subs = vec![
            cx.subscribe_in(&search, window, Self::on_search),
            cx.observe_window_appearance(window, |this, window, cx| {
                this.theme = Theme::for_appearance(window.appearance());
                this.style_field(&this.search.clone(), cx);
                if let Some((_, e)) = &this.editor {
                    let t = this.theme;
                    e.update(cx, |e, cx| e.set_theme(t, cx));
                }
                cx.notify();
            }),
        ];
        let mut app = SnotApp {
            lib,
            theme: Theme::for_appearance(window.appearance()),
            scope: Scope::All,
            sort: SortBy::Modified,
            folders: Vec::new(),
            counts: Counts::default(),
            notes: Vec::new(),
            search,
            query: String::new(),
            editor: None,
            menu: None,
            dialog: None,
            list_scroll: ScrollHandle::new(),
            focus: cx.focus_handle(),
            _subs: subs,
            editor_sub: None,
        };
        app.style_field(&app.search.clone(), cx);
        app.refresh(cx);
        window.focus(&app.focus, cx);
        if let Some(first) = app.notes.first().map(|n| n.id.clone()) {
            app.open(&first, false, window, cx);
        }
        app
    }

    fn style_field(&self, field: &Entity<TextField>, cx: &mut Context<Self>) {
        let t = self.theme;
        field.update(cx, |f, cx| {
            f.color = Some(t.text);
            f.caret_color = Some(t.caret);
            f.selection_color = Some(t.selection);
            f.placeholder_color = Some(t.text_faint);
            cx.notify();
        });
    }

    /// Re-reads folders, counts and the visible list.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.folders = self.lib.folders().unwrap_or_default();
        self.counts = self.lib.counts().unwrap_or_default();
        if let Scope::Folder(id) = &self.scope
            && !self.folders.iter().any(|f| &f.id == id)
        {
            self.scope = Scope::All;
        }
        let q = self.query.trim();
        self.notes = if q.is_empty() {
            self.lib.notes(&self.scope, self.sort).unwrap_or_default()
        } else {
            self.lib.search(q).unwrap_or_default()
        };
        cx.notify();
    }

    fn open_id(&self) -> Option<&str> {
        self.editor.as_ref().map(|(id, _)| id.as_str())
    }

    fn open(&mut self, id: &str, focus: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        if self.open_id() == Some(id) {
            return;
        }
        self.close_editor(cx);
        let Ok(note) = self.lib.note(id) else { return };
        let lib = self.lib.clone();
        let theme = self.theme;
        let view = cx.new(|cx| NoteView::new(lib, note, theme, window, cx));
        self.editor_sub = Some(cx.subscribe_in(&view, window, Self::on_note_event));
        if focus {
            view.update(cx, |v, cx| v.focus_initial(window, cx));
        } else {
            // Keep keyboard shortcuts working before the note is clicked.
            window.focus(&self.focus, cx);
        }
        self.editor = Some((id.to_string(), view));
        cx.notify();
    }

    /// Saves and drops the open note.
    fn close_editor(&mut self, cx: &mut Context<Self>) {
        if let Some((_, view)) = self.editor.take() {
            view.update(cx, |v, cx| v.save(cx));
        }
        self.editor_sub = None;
    }

    pub fn save_all(&mut self, cx: &mut Context<Self>) {
        if let Some((_, view)) = &self.editor {
            view.update(cx, |v, cx| v.save(cx));
        }
    }

    fn on_note_event(
        &mut self,
        _: &Entity<NoteView>,
        e: &NoteEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match e {
            NoteEvent::Saved => self.refresh(cx),
            NoteEvent::Closed => {
                self.editor = None;
                self.editor_sub = None;
                self.refresh(cx);
                self.open_first(window, cx);
            }
            NoteEvent::Open(id) => {
                let id = id.clone();
                self.refresh(cx);
                self.open(&id, true, window, cx);
            }
        }
    }

    fn open_first(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.notes.first().map(|n| n.id.clone()) {
            self.open(&id, false, window, cx);
        }
    }

    fn on_search(
        &mut self,
        _: &Entity<TextField>,
        e: &FieldEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match e {
            FieldEvent::Changed(q) => {
                self.query = q.clone();
                self.refresh(cx);
                self.list_scroll.set_offset(point(px(0.), px(0.)));
            }
            FieldEvent::Submit | FieldEvent::Down => self.open_first(window, cx),
            FieldEvent::Cancel => {
                self.search.update(cx, |f, cx| f.set_text("", cx));
                self.query.clear();
                self.refresh(cx);
            }
            FieldEvent::Up | FieldEvent::Blur => {}
        }
    }

    fn set_scope(&mut self, scope: Scope, window: &mut Window, cx: &mut Context<Self>) {
        self.scope = scope;
        self.menu = None;
        if !self.query.is_empty() {
            self.query.clear();
            self.search.update(cx, |f, cx| f.set_text("", cx));
        }
        self.list_scroll.set_offset(point(px(0.), px(0.)));
        self.refresh(cx);
        let open_visible = self
            .open_id()
            .is_some_and(|id| self.notes.iter().any(|n| n.id == id));
        if !open_visible {
            self.close_editor(cx);
            self.open_first(window, cx);
        }
    }

    fn folder_for_new(&self) -> Option<String> {
        match &self.scope {
            Scope::Folder(id) => Some(id.clone()),
            _ => None,
        }
    }

    fn new_note(&mut self, handwriting: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.save_all(cx);
        let folder = self.folder_for_new();
        let made = if handwriting {
            self.lib.create_ink_note(folder.as_deref())
        } else {
            self.lib.create_note(folder.as_deref())
        };
        let Ok(note) = made else { return };
        if matches!(self.scope, Scope::Trash | Scope::Favorites) || !self.query.is_empty() {
            self.scope = Scope::All;
            self.query.clear();
            self.search.update(cx, |f, cx| f.set_text("", cx));
        }
        self.refresh(cx);
        self.open(&note.id, true, window, cx);
    }

    // ---- actions -----------------------------------------------------------

    fn act_new_note(&mut self, _: &NewNote, window: &mut Window, cx: &mut Context<Self>) {
        self.new_note(false, window, cx)
    }

    fn act_new_handwriting(
        &mut self,
        _: &NewHandwriting,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.new_note(true, window, cx)
    }

    fn act_focus_search(&mut self, _: &FocusSearch, window: &mut Window, cx: &mut Context<Self>) {
        let f = self.search.focus_handle(cx);
        window.focus(&f, cx);
        self.search.update(cx, |f, cx| f.select_all_text(cx));
    }

    fn act_save(&mut self, _: &SaveNow, _: &mut Window, cx: &mut Context<Self>) {
        self.save_all(cx)
    }

    fn act_quit(&mut self, _: &Quit, _: &mut Window, cx: &mut Context<Self>) {
        self.save_all(cx);
        cx.quit();
    }

    fn act_new_folder(&mut self, _: &NewFolder, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt_new_folder(window, cx)
    }

    fn prompt_new_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let field = self.name_field("Folder name", "", window, cx);
        self.dialog = Some(Dialog::NewFolder(field));
        cx.notify();
    }

    fn name_field(
        &mut self,
        placeholder: &'static str,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<TextField> {
        let field = cx.new(|cx| {
            let mut f = TextField::new(placeholder, cx);
            f.set_text(text, cx);
            f.select_all_text(cx);
            f
        });
        self.style_field(&field, cx);
        self._subs.push(
            cx.subscribe_in(
                &field,
                window,
                |this, _, e: &FieldEvent, window, cx| match e {
                    FieldEvent::Submit => this.confirm_dialog(window, cx),
                    FieldEvent::Cancel => this.cancel_dialog(window, cx),
                    _ => {}
                },
            ),
        );
        window.focus(&field.focus_handle(cx), cx);
        field
    }

    fn cancel_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dialog = None;
        if let Some((_, v)) = &self.editor {
            window.focus(&v.focus_handle(cx), cx);
        }
        cx.notify();
    }

    fn confirm_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self.dialog.take() else {
            return;
        };
        match dialog {
            Dialog::NewFolder(field) => {
                let name = field.read(cx).text().trim().to_string();
                if !name.is_empty()
                    && let Ok(f) = self.lib.create_folder(&name, None)
                {
                    self.refresh(cx);
                    self.set_scope(Scope::Folder(f.id), window, cx);
                }
            }
            Dialog::RenameFolder(id, field) => {
                let name = field.read(cx).text().trim().to_string();
                if !name.is_empty() {
                    let _ = self.lib.rename_folder(&id, &name);
                }
            }
            Dialog::EmptyTrash => {
                if self.scope == Scope::Trash {
                    self.close_editor(cx);
                }
                let _ = self.lib.empty_trash();
            }
            Dialog::DeleteNote(id) => {
                if self.open_id() == Some(id.as_str()) {
                    self.editor = None;
                    self.editor_sub = None;
                }
                let _ = self.lib.delete_note(&id);
            }
            Dialog::DeleteFolder(id) => {
                let _ = self.lib.delete_folder(&id);
            }
        }
        self.refresh(cx);
        if self.editor.is_none() {
            self.open_first(window, cx);
        }
        cx.notify();
    }

    fn note_action(&mut self, id: &str, what: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        let lib = self.lib.clone();
        let n = self.notes.iter().find(|n| n.id == id).cloned();
        let open = self.open_id() == Some(id);
        if open {
            self.save_all(cx);
        }
        match what {
            "pin" => drop(lib.set_pinned(id, !n.is_some_and(|n| n.pinned))),
            "favorite" => drop(lib.set_favorite(id, !n.is_some_and(|n| n.favorite))),
            "duplicate" => {
                if let Ok(copy) = lib.duplicate_note(id) {
                    self.refresh(cx);
                    self.open(&copy.id, true, window, cx);
                    return;
                }
            }
            "trash" => {
                if open {
                    self.editor = None;
                    self.editor_sub = None;
                }
                let _ = lib.trash_note(id);
                self.refresh(cx);
                if open {
                    self.open_first(window, cx);
                }
                return;
            }
            "restore" => {
                let _ = lib.restore_note(id);
                if open {
                    self.editor = None;
                }
            }
            "delete" => {
                self.dialog = Some(Dialog::DeleteNote(id.to_string()));
            }
            _ => {}
        }
        if let Some((_, v)) = &self.editor {
            v.update(cx, |v, cx| v.refresh_meta(cx));
        }
        self.refresh(cx);
        if self.editor.is_none() {
            self.open_first(window, cx);
        }
    }

    fn open_menu(&mut self, menu: Menu, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.menu = Some((menu, at));
        cx.notify();
    }

    // ---- rendering -----------------------------------------------------------

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let c = &self.counts;
        let mut side = div()
            .id("sidebar")
            .w(px(232.))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .bg(t.sidebar_bg)
            .border_r_1()
            .border_color(t.border)
            .px(px(10.))
            .pt(px(14.))
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(8.))
                    .pb(px(14.))
                    .child(
                        div()
                            .size(px(26.))
                            .rounded(px(7.))
                            .bg(t.accent)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("pen", 16., t.accent_text)),
                    )
                    .child(
                        div()
                            .text_size(px(18.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Snot"),
                    ),
            );
        let entries = [
            (Scope::All, "notes", "All notes", c.all),
            (Scope::Favorites, "star", "Favourites", c.favorites),
            (Scope::Unfiled, "inbox", "No folder", c.unfiled),
        ];
        for (scope, ic, label, count) in entries {
            side = side.child(self.side_row(scope, ic, None, label.into(), count, cx));
        }
        side = side.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .mt(px(18.))
                .mb(px(4.))
                .pl(px(10.))
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.text_faint)
                .child("FOLDERS")
                .child(
                    icon_button("new-folder", "folder-plus", "New folder", false, &t)
                        .on_click(cx.listener(|this, _, w, cx| this.prompt_new_folder(w, cx))),
                ),
        );
        for f in &self.folders {
            let tint = f.color.as_deref().map(|c| argb(color::ink(c, t.dark)));
            let row = self.side_row(
                Scope::Folder(f.id.clone()),
                "folder",
                tint,
                f.name.clone().into(),
                f.note_count,
                cx,
            );
            let fid = f.id.clone();
            side = side.child(row.on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                    this.open_menu(Menu::Folder(fid.clone()), e.position, cx)
                }),
            ));
        }
        if self.folders.is_empty() {
            side = side.child(
                div()
                    .px(px(10.))
                    .py(px(6.))
                    .text_size(px(13.))
                    .text_color(t.text_faint)
                    .child("No folders yet"),
            );
        }
        side.child(div().flex_1().min_h(px(16.)))
            .child(self.side_row(Scope::Trash, "trash", None, "Trash".into(), c.trash, cx))
            .child(div().h(px(12.)))
    }

    fn side_row(
        &self,
        scope: Scope,
        ic: &str,
        tint: Option<gpui::Hsla>,
        label: SharedString,
        count: i64,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let t = self.theme;
        let active = self.scope == scope && self.query.is_empty();
        let id = SharedString::from(format!("scope-{label}-{scope:?}"));
        div()
            .id(id)
            .flex()
            .items_center()
            .gap(px(10.))
            .h(px(34.))
            .px(px(10.))
            .rounded(px(8.))
            .cursor_pointer()
            .text_size(px(14.))
            .when(active, |d| {
                d.bg(t.accent_soft)
                    .text_color(t.accent)
                    .font_weight(FontWeight::SEMIBOLD)
            })
            .when(!active, |d| d.hover(|s| s.bg(t.hover)))
            .child(icon(
                ic,
                17.,
                tint.unwrap_or(if active { t.accent } else { t.text_muted }),
            ))
            .child(div().flex_1().truncate().child(label))
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(t.text_faint)
                    .child(count.to_string()),
            )
            .on_click(cx.listener(move |this, _, w, cx| this.set_scope(scope.clone(), w, cx)))
    }

    fn scope_title(&self) -> String {
        if !self.query.trim().is_empty() {
            return "Search".into();
        }
        match &self.scope {
            Scope::All => "All notes".into(),
            Scope::Favorites => "Favourites".into(),
            Scope::Unfiled => "No folder".into(),
            Scope::Trash => "Trash".into(),
            Scope::Folder(id) => self
                .folders
                .iter()
                .find(|f| &f.id == id)
                .map(|f| f.name.clone())
                .unwrap_or_default(),
        }
    }

    fn note_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let trash = self.scope == Scope::Trash && self.query.is_empty();
        let header = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .px(px(14.))
            .pt(px(14.))
            .pb(px(10.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_size(px(20.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.scope_title()),
                    )
                    .when(!trash, |d| {
                        d.child(icon_button("sort", "sort", "Sort", false, &t).on_click(
                            cx.listener(|this, e: &ClickEvent, _, cx| {
                                this.open_menu(Menu::Sort, e.position(), cx)
                            }),
                        ))
                        .child(
                            icon_button(
                                "new-ink",
                                "pen",
                                "New handwriting note (Ctrl+Shift+N)",
                                false,
                                &t,
                            )
                            .on_click(cx.listener(|this, _, w, cx| this.new_note(true, w, cx))),
                        )
                        .child(
                            div()
                                .id("new-note")
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .h(px(32.))
                                .px(px(10.))
                                .rounded(px(8.))
                                .bg(t.accent)
                                .text_color(t.accent_text)
                                .text_size(px(13.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .cursor_pointer()
                                .hover(|s| s.opacity(0.9))
                                .child(icon("plus", 16., t.accent_text))
                                .child("New")
                                .tooltip(ui::tooltip("New note (Ctrl+N)"))
                                .on_click(
                                    cx.listener(|this, _, w, cx| this.new_note(false, w, cx)),
                                ),
                        )
                    })
                    .when(trash && self.counts.trash > 0, |d| {
                        d.child(
                            div()
                                .id("empty-trash")
                                .px(px(10.))
                                .h(px(30.))
                                .flex()
                                .items_center()
                                .rounded(px(8.))
                                .text_size(px(13.))
                                .text_color(t.danger)
                                .cursor_pointer()
                                .hover(|s| s.bg(t.hover))
                                .child("Empty trash")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.dialog = Some(Dialog::EmptyTrash);
                                    cx.notify();
                                })),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(36.))
                    .px(px(10.))
                    .rounded(px(9.))
                    .bg(t.card_bg)
                    .border_1()
                    .border_color(t.border)
                    .text_size(px(14.))
                    .child(icon("search", 16., t.text_faint))
                    .child(div().flex_1().child(self.search.clone())),
            )
            .when(trash, |d| {
                d.child(
                    div()
                        .text_size(px(12.5))
                        .text_color(t.text_faint)
                        .child("Notes in the trash are deleted after 30 days."),
                )
            });

        let mut list = div()
            .id("note-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.list_scroll)
            .px(px(10.))
            .pb(px(16.))
            .flex()
            .flex_col()
            .gap(px(6.));
        if self.notes.is_empty() {
            let msg = if !self.query.trim().is_empty() {
                "No notes match your search."
            } else if trash {
                "The trash is empty."
            } else {
                "No notes here yet."
            };
            list = list.child(
                div()
                    .pt(px(40.))
                    .flex()
                    .justify_center()
                    .text_size(px(14.))
                    .text_color(t.text_faint)
                    .child(msg),
            );
        }
        for n in &self.notes {
            list = list.child(self.card(n, cx));
        }
        div()
            .w(px(320.))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .bg(t.list_bg)
            .border_r_1()
            .border_color(t.border)
            .child(header)
            .child(list)
    }

    fn card(&self, n: &NoteSummary, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme;
        let selected = self.open_id() == Some(n.id.as_str());
        let tint = n
            .color
            .as_deref()
            .map(|c| argb(color::note_background(Some(c), t.dark)));
        let bg = if selected {
            t.card_selected
        } else {
            tint.unwrap_or(t.card_bg)
        };
        let title = if n.title.trim().is_empty() {
            "Untitled".to_string()
        } else {
            n.title.clone()
        };
        let body: AnyElement = match &n.snippet {
            Some(s) => snippet(s, &t).into_any_element(),
            None if n.preview.is_empty() => {
                let what = if n.has_ink {
                    "Handwriting"
                } else if n.has_image {
                    "Picture"
                } else {
                    "No text"
                };
                div()
                    .text_color(t.text_faint)
                    .child(what)
                    .into_any_element()
            }
            None => div().child(n.preview.clone()).into_any_element(),
        };
        let id = n.id.clone();
        let menu_id = n.id.clone();
        div()
            .id(SharedString::from(format!("card-{}", n.id)))
            .flex()
            .flex_col()
            .gap(px(4.))
            .p(px(12.))
            .rounded(px(10.))
            .bg(bg)
            .border_1()
            .border_color(if selected { t.accent } else { t.border })
            .when(tint.is_some() && !selected, |d| {
                d.border_l_4().border_color(argb(color::ink(
                    n.color.as_deref().unwrap_or("gray"),
                    t.dark,
                )))
            })
            .cursor_pointer()
            .when(!selected, |d| d.hover(|s| s.bg(t.card_hover)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_size(px(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .when(n.pinned, |d| d.child(icon("pin", 14., t.accent)))
                    .when(n.favorite, |d| d.child(icon("star-filled", 14., t.accent))),
            )
            .child(
                div()
                    .text_size(px(13.5))
                    .line_height(px(19.))
                    .text_color(t.text_muted)
                    .line_clamp(2)
                    .child(body),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(12.))
                    .text_color(t.text_faint)
                    .child(ui::date_label(n.updated_at))
                    .child(div().flex_1())
                    .when(n.has_ink, |d| d.child(icon("pen", 13., t.text_faint)))
                    .when(n.has_image, |d| d.child(icon("image", 13., t.text_faint))),
            )
            .on_click(cx.listener(move |this, _, w, cx| this.open(&id, false, w, cx)))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                    this.open_menu(Menu::Note(menu_id.clone()), e.position, cx)
                }),
            )
            .into_any_element()
    }

    fn render_menu(
        &self,
        menu: &Menu,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = self.theme;
        let mut p = panel(&t);
        match menu {
            Menu::Sort => {
                for (s, label) in [
                    (SortBy::Modified, "Date modified"),
                    (SortBy::Created, "Date created"),
                    (SortBy::Title, "Title"),
                ] {
                    let on = self.sort == s;
                    p = p.child(
                        menu_item(
                            SharedString::from(format!("sort-{label}")),
                            on.then_some("check"),
                            label,
                            &t,
                        )
                        .when(!on, |d| d.pl(px(36.)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.sort = s;
                            this.menu = None;
                            this.refresh(cx);
                        })),
                    );
                }
            }
            Menu::Note(id) => {
                let n = self.notes.iter().find(|n| &n.id == id);
                let trashed = n.is_some_and(|n| n.trashed_at.is_some());
                let item = |key: &'static str,
                            ic: &'static str,
                            label: &'static str,
                            cx: &mut Context<Self>| {
                    let id = id.clone();
                    menu_item(
                        SharedString::from(format!("note-{key}")),
                        Some(ic),
                        label,
                        &t,
                    )
                    .on_click(cx.listener(move |this, _, w, cx| this.note_action(&id, key, w, cx)))
                };
                if trashed {
                    p = p
                        .child(item("restore", "restore", "Restore", cx))
                        .child(item("delete", "trash", "Delete forever", cx).text_color(t.danger));
                } else {
                    let pinned = n.is_some_and(|n| n.pinned);
                    let fav = n.is_some_and(|n| n.favorite);
                    p = p
                        .child(item(
                            "pin",
                            "pin",
                            if pinned { "Unpin" } else { "Pin to top" },
                            cx,
                        ))
                        .child(item(
                            "favorite",
                            "star",
                            if fav {
                                "Remove from favourites"
                            } else {
                                "Add to favourites"
                            },
                            cx,
                        ))
                        .child(item("duplicate", "copy", "Duplicate", cx))
                        .child(separator(&t))
                        .child(self.folder_chooser(id, cx))
                        .child(separator(&t))
                        .child(item("trash", "trash", "Move to trash", cx).text_color(t.danger));
                }
            }
            Menu::Folder(id) => {
                let f = self.folders.iter().find(|f| &f.id == id).cloned();
                let (rid, did) = (id.clone(), id.clone());
                let mut colors = div()
                    .flex()
                    .flex_wrap()
                    .w(px(4. * 30.))
                    .gap(px(4.))
                    .p(px(4.));
                for token in color::TOKENS.iter().copied().filter(|c| *c != "ink") {
                    let fid = id.clone();
                    let on = f.as_ref().and_then(|f| f.color.as_deref()) == Some(token);
                    colors = colors.child(
                        swatch(
                            SharedString::from(format!("fc-{token}")),
                            argb(color::ink(token, t.dark)),
                            on,
                            &t,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let _ = this
                                .lib
                                .set_folder_color(&fid, if on { None } else { Some(token) });
                            this.menu = None;
                            this.refresh(cx);
                        })),
                    );
                }
                p = p
                    .child(
                        menu_item("folder-rename", Some("edit"), "Rename…", &t).on_click(
                            cx.listener(move |this, _, w, cx| {
                                this.menu = None;
                                let name = this
                                    .folders
                                    .iter()
                                    .find(|f| f.id == rid)
                                    .map(|f| f.name.clone())
                                    .unwrap_or_default();
                                let field = this.name_field("Folder name", &name, w, cx);
                                this.dialog = Some(Dialog::RenameFolder(rid.clone(), field));
                                cx.notify();
                            }),
                        ),
                    )
                    .child(colors)
                    .child(separator(&t))
                    .child(
                        menu_item("folder-delete", Some("trash"), "Delete folder…", &t)
                            .text_color(t.danger)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.menu = None;
                                this.dialog = Some(Dialog::DeleteFolder(did.clone()));
                                cx.notify();
                            })),
                    );
            }
        }
        let this = cx.entity().downgrade();
        ui::overlay(
            at,
            window.viewport_size(),
            p.into_any_element(),
            move |_, cx| {
                let _ = this.update(cx, |this, cx| {
                    this.menu = None;
                    cx.notify();
                });
            },
        )
        .into_any_element()
    }

    fn folder_chooser(&self, note: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let current = self
            .notes
            .iter()
            .find(|n| n.id == note)
            .and_then(|n| n.folder_id.clone());
        let mut col = div().flex().flex_col().gap(px(2.)).child(
            div()
                .px(px(10.))
                .py(px(2.))
                .text_size(px(12.))
                .text_color(t.text_faint)
                .child("Move to"),
        );
        let targets = std::iter::once((None, "No folder".to_string())).chain(
            self.folders
                .iter()
                .map(|f| (Some(f.id.clone()), f.name.clone())),
        );
        for (fid, name) in targets {
            let on = fid == current;
            let nid = note.to_string();
            let key = fid.clone().unwrap_or_default();
            col = col.child(
                menu_item(
                    SharedString::from(format!("move-{key}")),
                    Some(if fid.is_some() { "folder" } else { "inbox" }),
                    name,
                    &t,
                )
                .when(on, |d| d.text_color(t.accent))
                .on_click(cx.listener(move |this, _, _, cx| {
                    let _ = this.lib.move_note(&nid, fid.as_deref());
                    this.menu = None;
                    if let Some((_, v)) = &this.editor {
                        v.update(cx, |v, cx| v.refresh_meta(cx));
                    }
                    this.refresh(cx);
                })),
            );
        }
        col
    }

    fn render_dialog(
        &self,
        dialog: &Dialog,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = self.theme;
        let (title, body, field, confirm, danger): (
            &str,
            Option<String>,
            Option<Entity<TextField>>,
            &str,
            bool,
        ) = match dialog {
            Dialog::NewFolder(f) => ("New folder", None, Some(f.clone()), "Create", false),
            Dialog::RenameFolder(_, f) => ("Rename folder", None, Some(f.clone()), "Rename", false),
            Dialog::EmptyTrash => (
                "Empty the trash?",
                Some(format!(
                    "{} notes will be deleted for good.",
                    self.counts.trash
                )),
                None,
                "Empty trash",
                true,
            ),
            Dialog::DeleteNote(_) => (
                "Delete this note forever?",
                Some("This can't be undone.".into()),
                None,
                "Delete",
                true,
            ),
            Dialog::DeleteFolder(id) => {
                let n = self
                    .folders
                    .iter()
                    .find(|f| &f.id == id)
                    .map(|f| f.note_count)
                    .unwrap_or(0);
                (
                    "Delete this folder?",
                    Some(format!("Its {n} notes will move to the trash.")),
                    None,
                    "Delete",
                    true,
                )
            }
        };
        let viewport = window.viewport_size();
        let button = |id: &'static str, label: &str, primary: bool| {
            div()
                .id(id)
                .px(px(14.))
                .h(px(34.))
                .flex()
                .items_center()
                .rounded(px(8.))
                .text_size(px(14.))
                .cursor_pointer()
                .when(primary, |d| {
                    d.bg(if danger { t.danger } else { t.accent })
                        .text_color(t.accent_text)
                        .font_weight(FontWeight::SEMIBOLD)
                })
                .when(!primary, |d| d.hover(|s| s.bg(t.hover)))
                .child(label.to_string())
        };
        gpui::deferred(
            gpui::anchored().position(point(px(0.), px(0.))).child(
                div()
                    .id("dialog-backdrop")
                    .w(viewport.width)
                    .h(viewport.height)
                    .occlude()
                    .bg(t.shadow)
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, w, cx| this.cancel_dialog(w, cx)),
                    )
                    .child(
                        div()
                            .w(px(380.))
                            .flex()
                            .flex_col()
                            .gap(px(14.))
                            .p(px(20.))
                            .rounded(px(14.))
                            .bg(t.popover_bg)
                            .border_1()
                            .border_color(t.border)
                            .shadow_lg()
                            .text_color(t.text)
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(
                                div()
                                    .text_size(px(17.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(title),
                            )
                            .children(body.map(|b| {
                                div().text_size(px(14.)).text_color(t.text_muted).child(b)
                            }))
                            .children(field.map(|f| {
                                div()
                                    .h(px(38.))
                                    .px(px(10.))
                                    .flex()
                                    .items_center()
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(t.accent)
                                    .text_size(px(15.))
                                    .child(f)
                            }))
                            .child(
                                div()
                                    .flex()
                                    .justify_end()
                                    .gap(px(8.))
                                    .child(button("dialog-cancel", "Cancel", false).on_click(
                                        cx.listener(|this, _, w, cx| this.cancel_dialog(w, cx)),
                                    ))
                                    .child(button("dialog-ok", confirm, true).on_click(
                                        cx.listener(|this, _, w, cx| this.confirm_dialog(w, cx)),
                                    )),
                            ),
                    ),
            ),
        )
        .with_priority(3)
        .into_any_element()
    }

    fn empty_editor(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(12.))
            .bg(t.page_bg)
            .text_color(t.text_faint)
            .child(icon("notes", 44., t.text_faint))
            .child(
                div()
                    .text_size(px(15.))
                    .child("Pick a note, or start a new one"),
            )
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        menu_item("empty-new", Some("type"), "New note", &t)
                            .min_w(px(0.))
                            .border_1()
                            .border_color(t.border)
                            .text_color(t.text)
                            .on_click(cx.listener(|this, _, w, cx| this.new_note(false, w, cx))),
                    )
                    .child(
                        menu_item("empty-ink", Some("pen"), "Handwriting", &t)
                            .min_w(px(0.))
                            .border_1()
                            .border_color(t.border)
                            .text_color(t.text)
                            .on_click(cx.listener(|this, _, w, cx| this.new_note(true, w, cx))),
                    ),
            )
    }
}

/// A search snippet with the hits picked out.
fn snippet(s: &str, t: &Theme) -> StyledText {
    let mut text = String::new();
    let mut hits = Vec::new();
    let mut start = None;
    for ch in s.chars() {
        match ch {
            HIT_START => start = Some(text.len()),
            HIT_END => {
                if let Some(a) = start.take() {
                    hits.push((
                        a..text.len(),
                        HighlightStyle {
                            background_color: Some(t.accent_soft),
                            color: Some(t.text),
                            ..Default::default()
                        },
                    ));
                }
            }
            c => text.push(if c == '\n' { ' ' } else { c }),
        }
    }
    StyledText::new(text).with_highlights(hits)
}

impl Render for SnotApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let editor: AnyElement = match &self.editor {
            Some((_, v)) => v.clone().into_any_element(),
            None => self.empty_editor(cx).into_any_element(),
        };
        div()
            .id("snot")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .bg(t.app_bg)
            .text_color(t.text)
            .font_family(crate::assets::UI_FONT)
            .on_action(cx.listener(Self::act_new_note))
            .on_action(cx.listener(Self::act_new_handwriting))
            .on_action(cx.listener(Self::act_focus_search))
            .on_action(cx.listener(Self::act_save))
            .on_action(cx.listener(Self::act_quit))
            .on_action(cx.listener(Self::act_new_folder))
            .child(self.sidebar(cx))
            .child(self.note_list(cx))
            .child(div().flex_1().min_w_0().h_full().child(editor))
            .children(
                self.menu
                    .clone()
                    .map(|(m, at)| self.render_menu(&m, at, window, cx)),
            )
            .children(
                self.dialog
                    .as_ref()
                    .map(|d| self.render_dialog(d, window, cx)),
            )
    }
}

pub fn key_bindings() -> Vec<gpui::KeyBinding> {
    use gpui::KeyBinding;
    vec![
        KeyBinding::new("ctrl-n", NewNote, None),
        KeyBinding::new("ctrl-shift-n", NewHandwriting, None),
        KeyBinding::new("ctrl-f", FocusSearch, None),
        KeyBinding::new("ctrl-s", SaveNow, None),
        KeyBinding::new("ctrl-shift-f", NewFolder, None),
        KeyBinding::new("ctrl-q", Quit, None),
    ]
}
