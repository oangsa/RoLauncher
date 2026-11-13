//! Focusable single-line and multiline text input supporting IME, selection, read-only copy, and secret masking.

use crate::platform::appearance::{AppearancePreference, os_high_contrast_enabled};
use crate::theme::{AppTheme, DensityTokens};
use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, Element, ElementId, ElementInputHandler,
    Entity, EntityInputHandler, FocusHandle, Focusable, Global, GlobalElementId, Hsla,
    InteractiveElement, KeyBinding, KeyDownEvent, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, ShapedLine, SharedString, Style,
    TextRun, UTF16Selection, UnderlineStyle, Window, actions, div, fill, point, prelude::*, px,
    size,
};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;
use zeroize::Zeroize;

actions!(
    text_input,
    [
        Backspace,
        Delete,
        Left,
        Right,
        Up,
        Down,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        Paste,
        Cut,
        Copy,
        InsertNewline
    ]
);

/// Registers Windows-friendly keyboard bindings for every [`TextInput`] instance.
pub fn bind_text_input_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, Some("TextInput")),
        KeyBinding::new("delete", Delete, Some("TextInput")),
        KeyBinding::new("left", Left, Some("TextInput")),
        KeyBinding::new("right", Right, Some("TextInput")),
        KeyBinding::new("up", Up, Some("TextInput")),
        KeyBinding::new("down", Down, Some("TextInput")),
        KeyBinding::new("shift-left", SelectLeft, Some("TextInput")),
        KeyBinding::new("shift-right", SelectRight, Some("TextInput")),
        KeyBinding::new("ctrl-a", SelectAll, Some("TextInput")),
        KeyBinding::new("ctrl-v", Paste, Some("TextInput")),
        KeyBinding::new("ctrl-c", Copy, Some("TextInput")),
        KeyBinding::new("ctrl-x", Cut, Some("TextInput")),
        KeyBinding::new("home", Home, Some("TextInput")),
        KeyBinding::new("end", End, Some("TextInput")),
        KeyBinding::new("enter", InsertNewline, Some("TextInput")),
    ]);
}

/// Global UI theme setting used when rendering [`TextInput`] directly as a GPUI view.
#[derive(Debug, Clone, Copy, Default)]
pub struct UiStyleGlobal {
    pub appearance: AppearancePreference,
}

impl Global for UiStyleGlobal {}

/// Configuration flags for constructing a [`TextInput`] entity.
#[derive(Debug, Clone, Default)]
pub struct TextInputOptions {
    pub placeholder: String,
    pub masked: bool,
    pub read_only: bool,
    pub multiline: bool,
    pub height_px: Option<f32>,
    pub monospace: bool,
    pub max_len: Option<usize>,
}

type ChangeCallback = Box<dyn Fn(&str, &mut Window, &mut App)>;

#[derive(Clone)]
struct RenderedLine {
    plain_range: Range<usize>,
    shaped: ShapedLine,
}

/// Interactive text box entity implementing GPUI's native `EntityInputHandler` for IME and editing.
pub struct TextInput {
    focus_handle: FocusHandle,
    content: String,
    pub placeholder: String,
    masked: bool,
    read_only: bool,
    multiline: bool,
    height_px: Option<f32>,
    monospace: bool,
    max_len: Option<usize>,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    rendered_lines: Vec<RenderedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    scroll_x: Pixels,
    is_selecting: bool,
    on_change: Option<ChangeCallback>,
}

impl Drop for TextInput {
    fn drop(&mut self) {
        if self.masked {
            self.content.zeroize();
        }
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl TextInput {
    /// Creates a new [`TextInput`] entity inside GPUI state.
    pub fn new(cx: &mut Context<Self>, options: TextInputOptions) -> Self {
        Self {
            focus_handle: cx.focus_handle().tab_index(0).tab_stop(true),
            content: String::new(),
            placeholder: options.placeholder,
            masked: options.masked,
            read_only: options.read_only,
            multiline: options.multiline,
            height_px: options.height_px,
            monospace: options.monospace,
            max_len: options.max_len,
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            rendered_lines: Vec::new(),
            last_bounds: None,
            scroll_x: px(0.0),
            is_selecting: false,
            on_change: None,
        }
    }

    /// Registers a change listener invoked on edits.
    pub fn set_on_change(&mut self, callback: impl Fn(&str, &mut Window, &mut App) + 'static) {
        self.on_change = Some(Box::new(callback));
    }

    /// Returns the current text content.
    pub fn text(&self) -> &str {
        &self.content
    }

    /// Replaces current text programmatically and resets cursor to the end.
    pub fn set_text(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        if self.masked {
            self.content.zeroize();
        }
        self.content = text.into();
        let end = self.content.len();
        self.selected_range = end..end;
        self.selection_reversed = false;
        self.marked_range = None;
        self.scroll_x = px(0.0);
        cx.notify();
    }

    /// Focuses this input and optionally selects all text.
    pub fn focus_and_select_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_handle.focus(window, cx);
        self.selected_range = 0..self.content.len();
        self.selection_reversed = false;
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify();
    }

    fn emit_change(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(cb) = &self.on_change {
            let value = self.content.clone();
            cb(&value, window, cx);
        }
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn move_to(&mut self, offset: usize, window: &Window, cx: &mut Context<Self>) {
        let clamped = offset.min(self.content.len());
        self.selected_range = clamped..clamped;
        self.selection_reversed = false;
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, window: &Window, cx: &mut Context<Self>) {
        let clamped = offset.min(self.content.len());
        if self.selection_reversed {
            self.selected_range.start = clamped;
        } else {
            self.selected_range.end = clamped;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify();
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        let clamped = offset.min(self.content.len());
        self.content[..clamped]
            .grapheme_indices(true)
            .next_back()
            .map(|(idx, _)| idx)
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        let clamped = offset.min(self.content.len());
        self.content[clamped..]
            .grapheme_indices(true)
            .nth(1)
            .map(|(idx, _)| clamped + idx)
            .unwrap_or(self.content.len())
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;
        for ch in self.content.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }
        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;
        for ch in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }
        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }
        let Some(bounds) = self.last_bounds else {
            return 0;
        };
        if self.rendered_lines.is_empty() {
            return 0;
        }
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.content.len();
        }

        let line_h = if self.rendered_lines.len() > 1 {
            (f32::from(bounds.size.height) / self.rendered_lines.len() as f32).max(16.0)
        } else {
            f32::from(bounds.size.height).max(16.0)
        };
        let line_idx = (((f32::from(position.y - bounds.top())) / line_h).floor() as usize)
            .min(self.rendered_lines.len() - 1);
        let line = &self.rendered_lines[line_idx];
        let rel_x = position.x - bounds.left() + self.scroll_x;
        let visual_idx = line.shaped.closest_index_for_x(rel_x);
        visual_to_plain_offset(
            &self.content[line.plain_range.clone()],
            visual_idx,
            self.masked,
        ) + line.plain_range.start
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        if self.selected_range.is_empty() {
            let prev = self.previous_boundary(self.cursor_offset());
            self.select_to(prev, window, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        if self.selected_range.is_empty() {
            let next = self.next_boundary(self.cursor_offset());
            self.select_to(next, window, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn left(&mut self, _: &Left, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), window, cx);
        } else {
            self.move_to(self.selected_range.start, window, cx);
        }
    }

    fn right(&mut self, _: &Right, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.cursor_offset()), window, cx);
        } else {
            self.move_to(self.selected_range.end, window, cx);
        }
    }

    fn up(&mut self, _: &Up, window: &mut Window, cx: &mut Context<Self>) {
        if !self.multiline {
            self.move_to(0, window, cx);
            return;
        }
        let target = vertical_move_offset(&self.content, self.cursor_offset(), -1);
        self.move_to(target, window, cx);
    }

    fn down(&mut self, _: &Down, window: &mut Window, cx: &mut Context<Self>) {
        if !self.multiline {
            self.move_to(self.content.len(), window, cx);
            return;
        }
        let target = vertical_move_offset(&self.content, self.cursor_offset(), 1);
        self.move_to(target, window, cx);
    }

    fn select_left(&mut self, _: &SelectLeft, window: &mut Window, cx: &mut Context<Self>) {
        let prev = self.previous_boundary(self.cursor_offset());
        self.select_to(prev, window, cx);
    }

    fn select_right(&mut self, _: &SelectRight, window: &mut Window, cx: &mut Context<Self>) {
        let next = self.next_boundary(self.cursor_offset());
        self.select_to(next, window, cx);
    }

    fn select_all(&mut self, _: &SelectAll, window: &mut Window, cx: &mut Context<Self>) {
        self.selected_range = 0..self.content.len();
        self.selection_reversed = false;
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify();
    }

    fn home(&mut self, _: &Home, window: &mut Window, cx: &mut Context<Self>) {
        if self.multiline {
            let start = self.content[..self.cursor_offset()]
                .rfind('\n')
                .map(|idx| idx + 1)
                .unwrap_or(0);
            self.move_to(start, window, cx);
        } else {
            self.move_to(0, window, cx);
        }
    }

    fn end(&mut self, _: &End, window: &mut Window, cx: &mut Context<Self>) {
        if self.multiline {
            let cur = self.cursor_offset();
            let end = self.content[cur..]
                .find('\n')
                .map(|rel| cur + rel)
                .unwrap_or(self.content.len());
            self.move_to(end, window, cx);
        } else {
            self.move_to(self.content.len(), window, cx);
        }
    }

    fn insert_newline(&mut self, _: &InsertNewline, window: &mut Window, cx: &mut Context<Self>) {
        if self.multiline && !self.read_only {
            self.replace_text_in_range(None, "\n", window, cx);
        }
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let sanitized = if self.multiline {
                text.replace("\r\n", "\n").replace('\r', "\n")
            } else {
                text.replace("\r\n", " ").replace(['\r', '\n'], " ")
            };
            self.replace_text_in_range(None, &sanitized, window, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if self.masked || self.selected_range.is_empty() {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(
            self.content[self.selected_range.clone()].to_string(),
        ));
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if self.masked || self.read_only || self.selected_range.is_empty() {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(
            self.content[self.selected_range.clone()].to_string(),
        ));
        self.replace_text_in_range(None, "", window, cx);
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.is_selecting = true;
        window.focus(&self.focus_handle, cx);
        let idx = self.index_for_mouse_position(event.position);
        if event.modifiers.shift {
            self.select_to(idx, window, cx);
        } else {
            self.move_to(idx, window, cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_selecting {
            let idx = self.index_for_mouse_position(event.position);
            self.select_to(idx, window, cx);
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.focus_handle.is_focused(window) {
            return;
        }
        if event.keystroke.key == "escape" {
            return;
        }
        let _ = cx;
    }

    /// Renders this text input with theme-aware surface, placeholder, selection highlight, and cursor.
    pub fn view(
        &self,
        id: impl Into<ElementId>,
        theme: &AppTheme,
        density: &DensityTokens,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let focused = self.focus_handle.is_focused(window);
        let h = self.height_px.unwrap_or(if self.multiline {
            132.0
        } else {
            density.control_h
        });

        div()
            .id(id)
            .key_context("TextInput")
            .track_focus(&self.focus_handle)
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::insert_newline))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .w_full()
            .h(px(h))
            .px(px(10.0))
            .py(px(if self.multiline { 8.0 } else { 0.0 }))
            .flex()
            .items_center()
            .rounded(px(8.0))
            .border_1()
            .border_color(if focused {
                theme.border_focus
            } else {
                theme.border_strong
            })
            .bg(if self.read_only {
                theme.bg_surface_elevated
            } else {
                theme.bg_input
            })
            .overflow_hidden()
            .text_size(px(density.font_body))
            .text_color(theme.text_primary)
            .when(self.monospace, |d| d.font_family("Consolas"))
            .child(TextCanvasElement {
                input: cx.entity(),
                placeholder_color: theme.text_muted,
                text_color: theme.text_primary,
                selection_color: theme.bg_selected,
                cursor_color: theme.border_focus,
            })
    }
}

impl gpui::Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let style = cx
            .try_global::<UiStyleGlobal>()
            .copied()
            .unwrap_or_default();
        let theme = AppTheme::resolve(
            style.appearance,
            window.appearance(),
            os_high_contrast_enabled(),
        );
        let density = DensityTokens::default();
        let id = ElementId::Name(format!("ti-{}", cx.entity_id().as_u64()).into());
        self.view(id, &theme, &density, window, cx)
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn set_selected_text_range(
        &mut self,
        range_utf16: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected_range = self.range_from_utf16(&range_utf16);
        self.selection_reversed = false;
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify();
    }

    fn text_length_utf16(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.content.encode_utf16().count())
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.marked_range = None;
        window.notify_text_input_changed(&self.focus_handle);
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only {
            return;
        }
        let mut filtered = if self.multiline {
            new_text.replace("\r\n", "\n").replace('\r', "\n")
        } else {
            new_text.replace(['\r', '\n'], "")
        };
        let range = range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());
        if let Some(max_len) = self.max_len {
            let kept = self.content[..range.start].chars().count()
                + self.content[range.end..].chars().count();
            let remaining = max_len.saturating_sub(kept);
            if filtered.chars().count() > remaining {
                filtered = filtered.chars().take(remaining).collect();
            }
        }

        self.content = format!(
            "{}{}{}",
            &self.content[..range.start],
            filtered,
            &self.content[range.end..]
        );
        let new_caret = range.start + filtered.len();
        self.selected_range = new_caret..new_caret;
        self.selection_reversed = false;
        self.marked_range = None;
        window.notify_text_input_changed(&self.focus_handle);
        self.emit_change(window, cx);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only {
            return;
        }
        let range = range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());

        self.content = format!(
            "{}{}{}",
            &self.content[..range.start],
            new_text,
            &self.content[range.end..]
        );
        self.marked_range =
            (!new_text.is_empty()).then_some(range.start..range.start + new_text.len());
        let ins_start_utf16 = self.offset_to_utf16(range.start);
        let ins_len_utf16 = new_text.encode_utf16().count();
        self.selected_range = new_selected_range_utf16
            .map(|sel| {
                self.range_from_utf16(
                    &(ins_start_utf16 + sel.start.min(ins_len_utf16)
                        ..ins_start_utf16 + sel.end.min(ins_len_utf16)),
                )
            })
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());
        self.selection_reversed = false;
        window.notify_text_input_changed(&self.focus_handle);
        self.emit_change(window, cx);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = self.range_from_utf16(&range_utf16);
        let line_h = window.line_height();
        for (idx, line) in self.rendered_lines.iter().enumerate() {
            if range.start >= line.plain_range.start && range.start <= line.plain_range.end {
                let local_start = plain_to_visual_offset(
                    &self.content[line.plain_range.clone()],
                    range.start - line.plain_range.start,
                    self.masked,
                );
                let local_end = plain_to_visual_offset(
                    &self.content[line.plain_range.clone()],
                    range.end.min(line.plain_range.end) - line.plain_range.start,
                    self.masked,
                );
                let top = bounds.top() + line_h * idx as f32;
                return Some(Bounds::from_corners(
                    point(
                        bounds.left() + line.shaped.x_for_index(local_start) - self.scroll_x,
                        top,
                    ),
                    point(
                        bounds.left() + line.shaped.x_for_index(local_end) - self.scroll_x,
                        top + line_h,
                    ),
                ));
            }
        }
        None
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let idx = self.index_for_mouse_position(point);
        Some(self.offset_to_utf16(idx))
    }
}

struct TextCanvasElement {
    input: Entity<TextInput>,
    placeholder_color: Hsla,
    text_color: Hsla,
    selection_color: Hsla,
    cursor_color: Hsla,
}

struct TextPrepaintState {
    lines: Vec<RenderedLine>,
    selections: Vec<PaintQuad>,
    cursor: Option<PaintQuad>,
    line_height: Pixels,
    scroll_x: Pixels,
    is_placeholder: bool,
}

impl Element for TextCanvasElement {
    type RequestLayoutState = ();
    type PrepaintState = TextPrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let multiline = self.input.read(cx).multiline;
        let mut style = Style::default();
        style.size.width = gpui::relative(1.0).into();
        if multiline {
            style.size.height = gpui::relative(1.0).into();
        } else {
            style.size.height = window.line_height().into();
        }
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();
        let is_placeholder = input.content.is_empty();

        let mut lines = Vec::new();
        if is_placeholder {
            let text: SharedString = input.placeholder.clone().into();
            let run = TextRun {
                len: text.len(),
                font: style.font(),
                color: self.placeholder_color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let shaped = window
                .text_system()
                .shape_line(text, font_size, &[run], None);
            lines.push(RenderedLine {
                plain_range: 0..0,
                shaped,
            });
        } else if input.multiline {
            let mut offset = 0usize;
            for part in input.content.split('\n') {
                let end = offset + part.len();
                let text: SharedString = part.to_string().into();
                let run = TextRun {
                    len: text.len(),
                    font: style.font(),
                    color: self.text_color,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let shaped = window
                    .text_system()
                    .shape_line(text, font_size, &[run], None);
                lines.push(RenderedLine {
                    plain_range: offset..end,
                    shaped,
                });
                offset = end + 1;
            }
        } else {
            let visual_text: SharedString = if input.masked {
                "•".repeat(input.content.chars().count()).into()
            } else {
                input.content.clone().into()
            };
            let base_run = TextRun {
                len: visual_text.len(),
                font: style.font(),
                color: self.text_color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let runs = if !input.masked
                && let Some(marked) = input.marked_range.as_ref()
            {
                vec![
                    TextRun {
                        len: marked.start,
                        ..base_run.clone()
                    },
                    TextRun {
                        len: marked.end - marked.start,
                        underline: Some(UnderlineStyle {
                            color: Some(self.text_color),
                            thickness: px(1.0),
                            wavy: false,
                        }),
                        ..base_run.clone()
                    },
                    TextRun {
                        len: visual_text.len().saturating_sub(marked.end),
                        ..base_run
                    },
                ]
                .into_iter()
                .filter(|r| r.len > 0)
                .collect()
            } else {
                vec![base_run]
            };
            let shaped = window
                .text_system()
                .shape_line(visual_text, font_size, &runs, None);
            lines.push(RenderedLine {
                plain_range: 0..input.content.len(),
                shaped,
            });
        }

        // Compute horizontal scroll offset for single-line fields so cursor stays in viewport.
        let mut scroll_x = if input.multiline {
            px(0.0)
        } else {
            input.scroll_x
        };
        let cursor_plain = input.cursor_offset();
        if !input.multiline && !is_placeholder && !lines.is_empty() {
            let visual_cursor = plain_to_visual_offset(&input.content, cursor_plain, input.masked);
            let cursor_x = lines[0].shaped.x_for_index(visual_cursor);
            let max_w = (bounds.size.width - px(4.0)).max(px(20.0));
            if cursor_x - scroll_x > max_w {
                scroll_x = cursor_x - max_w;
            } else if cursor_x < scroll_x {
                scroll_x = (cursor_x - px(12.0)).max(px(0.0));
            }
            if lines[0].shaped.width <= bounds.size.width {
                scroll_x = px(0.0);
            }
        } else if is_placeholder {
            scroll_x = px(0.0);
        }

        let mut selections = Vec::new();
        let mut cursor_quad = None;

        if !is_placeholder {
            let sel = input.selected_range.clone();
            if sel.is_empty() {
                for (idx, line) in lines.iter().enumerate() {
                    if cursor_plain >= line.plain_range.start
                        && cursor_plain <= line.plain_range.end
                    {
                        let local_plain = cursor_plain - line.plain_range.start;
                        let local_vis = plain_to_visual_offset(
                            &input.content[line.plain_range.clone()],
                            local_plain,
                            input.masked,
                        );
                        let cx_pos = bounds.left() + line.shaped.x_for_index(local_vis) - scroll_x;
                        let top = bounds.top() + line_height * idx as f32;
                        cursor_quad = Some(fill(
                            Bounds::new(point(cx_pos, top), size(px(2.0), line_height)),
                            self.cursor_color,
                        ));
                        break;
                    }
                }
            } else {
                for (idx, line) in lines.iter().enumerate() {
                    let s_start = sel.start.max(line.plain_range.start);
                    let s_end = sel.end.min(line.plain_range.end);
                    if s_start <= s_end && !(s_start == s_end && s_start == line.plain_range.end) {
                        let vis_start = plain_to_visual_offset(
                            &input.content[line.plain_range.clone()],
                            s_start - line.plain_range.start,
                            input.masked,
                        );
                        let vis_end = plain_to_visual_offset(
                            &input.content[line.plain_range.clone()],
                            s_end - line.plain_range.start,
                            input.masked,
                        );
                        let x0 = bounds.left() + line.shaped.x_for_index(vis_start) - scroll_x;
                        let mut x1 = bounds.left() + line.shaped.x_for_index(vis_end) - scroll_x;
                        if x1 <= x0 {
                            x1 = x0 + px(4.0);
                        }
                        let top = bounds.top() + line_height * idx as f32;
                        selections.push(fill(
                            Bounds::from_corners(point(x0, top), point(x1, top + line_height)),
                            self.selection_color,
                        ));
                    }
                }
            }
        } else {
            cursor_quad = Some(fill(
                Bounds::new(bounds.origin, size(px(2.0), line_height)),
                self.cursor_color,
            ));
        }

        TextPrepaintState {
            lines,
            selections,
            cursor: cursor_quad,
            line_height,
            scroll_x,
            is_placeholder,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );

        for quad in prepaint.selections.drain(..) {
            window.paint_quad(quad);
        }

        let lines = std::mem::take(&mut prepaint.lines);
        for (idx, line) in lines.iter().enumerate() {
            let origin = point(
                bounds.left() - prepaint.scroll_x,
                bounds.top() + prepaint.line_height * idx as f32,
            );
            let _ = line.shaped.paint(
                origin,
                prepaint.line_height,
                gpui::TextAlign::Left,
                None,
                window,
                cx,
            );
        }

        if focus_handle.is_focused(window)
            && let Some(cursor) = prepaint.cursor.take()
        {
            window.paint_quad(cursor);
        }

        let scroll_x = prepaint.scroll_x;
        let is_placeholder = prepaint.is_placeholder;
        self.input.update(cx, |input, _| {
            input.last_bounds = Some(bounds);
            input.scroll_x = scroll_x;
            input.rendered_lines = if is_placeholder { Vec::new() } else { lines };
        });
    }
}

fn plain_to_visual_offset(plain: &str, plain_byte_offset: usize, masked: bool) -> usize {
    if !masked {
        return plain_byte_offset.min(plain.len());
    }
    let clamped = plain_byte_offset.min(plain.len());
    plain[..clamped].chars().count() * '•'.len_utf8()
}

fn visual_to_plain_offset(plain: &str, visual_byte_offset: usize, masked: bool) -> usize {
    if !masked {
        return visual_byte_offset.min(plain.len());
    }
    let bullet_len = '•'.len_utf8();
    let target_chars = (visual_byte_offset + bullet_len / 2) / bullet_len;
    plain
        .char_indices()
        .nth(target_chars)
        .map(|(idx, _)| idx)
        .unwrap_or(plain.len())
}

fn vertical_move_offset(content: &str, cursor: usize, delta_lines: isize) -> usize {
    let lines: Vec<(usize, &str)> = content
        .split('\n')
        .scan(0usize, |pos, s| {
            let start = *pos;
            *pos = start + s.len() + 1;
            Some((start, s))
        })
        .collect();
    if lines.is_empty() {
        return 0;
    }
    let cur_idx = lines
        .iter()
        .position(|(start, s)| cursor >= *start && cursor <= *start + s.len())
        .unwrap_or(lines.len() - 1);
    let col = cursor - lines[cur_idx].0;
    let target_idx = (cur_idx as isize + delta_lines).clamp(0, lines.len() as isize - 1) as usize;
    let (target_start, target_str) = lines[target_idx];
    target_start + col.min(target_str.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masked_offset_mapping_handles_multibyte_chars() {
        let sample = "aβc";
        assert_eq!(plain_to_visual_offset(sample, 0, true), 0);
        assert_eq!(plain_to_visual_offset(sample, 1, true), 3);
        assert_eq!(plain_to_visual_offset(sample, 3, true), 6);
        assert_eq!(visual_to_plain_offset(sample, 0, true), 0);
        assert_eq!(visual_to_plain_offset(sample, 3, true), 1);
        assert_eq!(visual_to_plain_offset(sample, 6, true), 3);
        assert_eq!(visual_to_plain_offset(sample, 99, true), 4);
    }
}
