//! editor rel_line_nums neovim_style avr_syntax

use eframe::egui::{
    self,
    gui_zoom::kb_shortcuts,
    text::{CCursor, CCursorRange, CursorRange},
    Align, Align2, Color32, FontFamily, FontId, Id, Key, Layout, Margin, Modifiers, Order, RichText,
    ScrollArea, TextEdit, Ui, Vec2,
};

use crate::modal_chrome::{
    modal_btn_secondary, modal_single_line_edit_with_id, search_bar_frame,
};
use crate::syntax::highlight_avr;
use crate::theme;
use crate::theme::START_GREEN;

pub struct SearchBar {
    pub visible:      bool,
    pub query:        String,
    prev_query:       String,
    /// char-index start of each match
    pub matches:      Vec<usize>,
    pub current:      usize,
    needs_focus:      bool,
    /// if idk(y), scroll area jumps to this offset once
    pub next_scroll:  Option<f32>,
    /// set after navigate(); cleared after cursor is pushed once
    pending_cursor:   bool,
    /// After Cmd+F / open: select all query text once focus lands (replace-on-type).
    select_all_on_focus: bool,
    id: Id,
}

impl SearchBar {
    fn new(parent_id: Id) -> Self {
        Self {
            visible:        false,
            query:          String::new(),
            prev_query:     String::new(),
            matches:        Vec::new(),
            current:        0,
            needs_focus:    false,
            next_scroll:    None,
            pending_cursor: false,
            select_all_on_focus: false,
            id: parent_id.with("search_input"),
        }
    }

    pub fn open(&mut self) {
        self.visible     = true;
        self.needs_focus = true;
        self.select_all_on_focus = true;
    }

    pub fn close(&mut self) {
        self.visible        = false;
        self.query.clear();
        self.prev_query.clear();
        self.matches.clear();
        self.current        = 0;
        self.pending_cursor = false;
    }

    /// rebuild match list from source (its not case sensitive)
    pub fn rebuild(&mut self, source: &str) {
        if self.query == self.prev_query { return; }
        self.prev_query = self.query.clone();
        self.matches.clear();
        self.current = 0;
        if self.query.is_empty() { return; }
        let q_lo: Vec<char> = self.query.to_lowercase().chars().collect();
        let s_lo: Vec<char> = source.to_lowercase().chars().collect();
        let m = q_lo.len();
        let n = s_lo.len();
        let mut ci = 0usize;
        while ci + m <= n {
            if s_lo[ci..ci + m] == q_lo[..] {
                self.matches.push(ci);
                ci += m;
            } else {
                ci += 1;
            }
        }
    }

    /// navigate ±1, wrapping, then schedule a scroll and cursor push
    pub fn navigate(&mut self, delta: i32, source: &str, row_h: f32) {
        if self.matches.is_empty() { return; }
        let n = self.matches.len();
        self.current = ((self.current as i64 + delta as i64).rem_euclid(n as i64)) as usize;
        self.pending_cursor = true;
        self.schedule_scroll(source, row_h);
    }

    fn schedule_scroll(&mut self, source: &str, row_h: f32) {
        if let Some(&ci) = self.matches.get(self.current) {
            let byte = char_to_byte(source, ci);
            let line = source[..byte].chars().filter(|&c| c == '\n').count();
            self.next_scroll = Some((line as f32 * row_h).max(0.0));
        }
    }
}

// char-index → byte-index
fn char_to_byte(s: &str, ci: usize) -> usize {
    s.char_indices().nth(ci).map(|(b, _)| b).unwrap_or(s.len())
}

fn char_slice(s: &str, start_c: usize, end_c: usize) -> &str {
    let start_b = char_to_byte(s, start_c);
    let end_b = char_to_byte(s, end_c);
    &s[start_b..end_b]
}

fn line_char_range_at_cursor(text: &str, cursor_c: usize) -> (usize, usize) {
    let mut line_start = 0usize;
    let mut pos = 0usize;
    for ch in text.chars() {
        if ch == '\n' {
            if cursor_c <= pos {
                return (line_start, pos);
            }
            line_start = pos + 1;
        }
        pos += 1;
    }
    (line_start, pos)
}

fn leading_tab_prefix(line: &str) -> &str {
    let n = line.as_bytes().iter().take_while(|&&b| b == b'\t').count();
    &line[..n]
}

fn try_smart_enter_insert(source: &mut String, cursor_c: usize) -> Option<usize> {
    let (line_start, line_end) = line_char_range_at_cursor(source, cursor_c);
    let full_line = char_slice(source, line_start, line_end);
    let leading_tabs = leading_tab_prefix(full_line);
    if leading_tabs.is_empty() {
        return None;
    }
    let rest = full_line.strip_prefix(leading_tabs).unwrap_or("");
    let insert = if rest.trim().is_empty() {
        "\n".to_string()
    } else {
        format!("\n{}", leading_tabs)
    };
    let insert_len = insert.chars().count();
    let b = char_to_byte(source, cursor_c);
    source.insert_str(b, &insert);
    Some(cursor_c + insert_len)
}

/// split LayoutJob sections at [byte_start, byte_end) and colour only that slice
fn apply_highlight(
    job:        &mut egui::text::LayoutJob,
    byte_start: usize,
    byte_end:   usize,
    bg:         Color32,
) {
    let mut out = Vec::with_capacity(job.sections.len() + 2);
    for sec in job.sections.drain(..) {
        let ss = sec.byte_range.start;
        let se = sec.byte_range.end;
        if se <= byte_start || ss >= byte_end {
            out.push(sec);
        } else {
            // part before the match
            if ss < byte_start {
                let mut before = sec.clone();
                before.byte_range = ss..byte_start;
                out.push(before);
            }
            // exact match slice
            let mut mid = sec.clone();
            mid.byte_range    = byte_start.max(ss)..byte_end.min(se);
            mid.format.background = bg;
            out.push(mid);
            // part after the match
            if se > byte_end {
                let mut after = sec.clone();
                after.byte_range = byte_end..se;
                out.push(after);
            }
        }
    }
    job.sections = out;
}

pub struct TextEditor {
    source:       String,
    saved_source: String,
    id:           Id,
    needs_focus:  bool,
    cursor_line:  usize,
    board_inline_accept_ok: bool,
    monospace_px: f32,
    pub search:   SearchBar,
}

impl TextEditor {
    pub fn new(id: Id) -> Self {
        let search = SearchBar::new(id);
        Self {
            source:       String::new(),
            saved_source: String::new(),
            id,
            needs_focus:  false,
            cursor_line:  0,
            board_inline_accept_ok: false,
            monospace_px: 14.0,
            search,
        }
    }

    pub fn reset_for_session(&mut self) {
        self.cursor_line = 0;
        self.focus_next_frame();
    }

    pub fn set_source(&mut self, source: String) {
        self.saved_source = source.clone();
        self.source       = source;
        self.cursor_line  = 0;
    }

    pub fn source(&self) -> &str { &self.source }
    pub fn is_dirty(&self) -> bool { self.source != self.saved_source }
    pub fn mark_saved(&mut self) { self.saved_source = self.source.clone(); }

    pub fn discard_unsaved_changes(&mut self) {
        self.source = self.saved_source.clone();
    }
    pub fn focus_next_frame(&mut self) { self.needs_focus = true; }

    pub fn request_initial_focus(&mut self, ctx: &egui::Context) {
        if self.needs_focus {
            ctx.memory_mut(|mem| mem.request_focus(self.id));
            self.needs_focus = false;
        }
    }

    pub fn text_edit_id(&self) -> Id {
        self.id
    }

    pub fn board_inline_accept_ok(&self) -> bool {
        self.board_inline_accept_ok
    }

    /// Cmd+Plus / Cmd+Minus / Cmd+0 adjust monospace size for this editor only (global GUI zoom is off).
    pub fn apply_editor_zoom_keyboard(&mut self, ctx: &egui::Context) {
        if !ctx.memory(|m| m.has_focus(self.id)) {
            return;
        }
        if ctx.input_mut(|i| i.consume_shortcut(&kb_shortcuts::ZOOM_RESET)) {
            self.monospace_px = 14.0;
            ctx.request_repaint();
            return;
        }
        if ctx.input_mut(|i| i.consume_shortcut(&kb_shortcuts::ZOOM_IN))
            || ctx.input_mut(|i| i.consume_shortcut(&kb_shortcuts::ZOOM_IN_SECONDARY))
        {
            self.monospace_px = (self.monospace_px + 1.0).min(28.0);
            ctx.request_repaint();
            return;
        }
        if ctx.input_mut(|i| i.consume_shortcut(&kb_shortcuts::ZOOM_OUT)) {
            self.monospace_px = (self.monospace_px - 1.0).max(8.0);
            ctx.request_repaint();
        }
    }

    pub fn apply_board_inline_completion(&mut self, ctx: &egui::Context) {
        let lines = text_lines(&self.source);
        let line_idx = self.cursor_line.min(lines.len().saturating_sub(1));
        let Some(line) = lines.get(line_idx) else {
            return;
        };
        let Some((indent, partial)) = parse_dot_board_line(line) else {
            return;
        };
        let Some(suffix) = board_ghost_suffix(partial) else {
            return;
        };
        let p = partial.trim();
        let chip = format!("{p}{suffix}");
        let new_line = format!("{indent}.board {chip}");
        replace_line_in_source(&mut self.source, line_idx, &new_line);
        let eol = line_end_char_index(&self.source, line_idx);
        if let Some(mut ts) = TextEdit::load_state(ctx, self.id) {
            ts.cursor
                .set_char_range(Some(CCursorRange::one(CCursor::new(eol))));
            TextEdit::store_state(ctx, self.id, ts);
        }
        ctx.request_repaint();
    }

    pub fn show(&mut self, ui: &mut Ui, show_ghost_hint: bool) {
        // shortcuts hooray
        let cmd_f = ui.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                if cfg!(target_os = "macos") { Modifiers::MAC_CMD } else { Modifiers::CTRL },
                Key::F,
            ))
        });
        if cmd_f { self.search.open(); }

        // escape only closes search when the search input actually has keyboard focus,
        // so normal editor typing / other shortcuts are never broken
        if self.search.visible {
            let search_focused = ui.ctx().memory(|m| m.has_focus(self.search.id));
            if search_focused {
                let esc = ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
                if esc {
                    self.search.close();
                    self.focus_next_frame();
                }
            }
        }

        let font_id = FontId::new(self.monospace_px, FontFamily::Monospace);
        let row_h   = ui.fonts(|f| f.row_height(&font_id));

        if self.search.visible && ui.ctx().memory(|m| m.has_focus(self.search.id)) {
            if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
                let snap = self.source.clone();
                self.search.navigate(1, &snap, row_h);
            }
        }

        if ui.ctx().memory(|m| m.has_focus(self.id)) {
            let enter_no_shift = ui.input(|i| i.key_pressed(Key::Enter) && !i.modifiers.shift);
            if enter_no_shift {
                if let Some(mut ts) = TextEdit::load_state(ui.ctx(), self.id) {
                    if let Some(ccr) = ts.cursor.char_range() {
                        let collapsed = ccr.primary.index == ccr.secondary.index
                            && ccr.primary.prefer_next_row == ccr.secondary.prefer_next_row;
                        if collapsed {
                            let cursor_c = ccr.primary.index;
                            if let Some(new_cursor) = try_smart_enter_insert(&mut self.source, cursor_c) {
                                ts.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(new_cursor))));
                                TextEdit::store_state(ui.ctx(), self.id, ts);
                                ui.input_mut(|i| {
                                    i.consume_key(Modifiers::NONE, Key::Enter);
                                });
                            }
                        }
                    }
                }
            }
        }

        let n       = line_count(&self.source);

        let digit_cols = (n.max(1).ilog10() + 1).max(3) as usize;
        let gutter_w   =
            ui.fonts(|f| f.glyph_width(&font_id, '0') * digit_cols as f32 + 14.0);

        // capture rect before any allocation (for Area positioning later)
        let editor_rect = ui.available_rect_before_wrap();
        let bg_resp = ui.interact(editor_rect, self.id.with("bg"), egui::Sense::click());

        let current_line = self.cursor_line.min(n.saturating_sub(1));

        // rebuild
        if self.search.visible {
            let snap = self.source.clone();
            self.search.rebuild(&snap);
        }

        // push TextEdit cursor only when the user explicitly navigated
        // (not every frame bc that would override normal editor cursor movement)
        if self.search.pending_cursor && !self.search.matches.is_empty() {
            let ci   = self.search.matches[self.search.current];
            let qlen = self.search.query.chars().count();
            if let Some(mut ts) = egui::TextEdit::load_state(ui.ctx(), self.id) {
                use egui::text::CCursor;
                ts.cursor.set_char_range(Some(egui::text::CCursorRange::two(
                    CCursor { index: ci,        prefer_next_row: false },
                    CCursor { index: ci + qlen, prefer_next_row: false },
                )));
                egui::TextEdit::store_state(ui.ctx(), self.id, ts);
            }
            self.search.pending_cursor = false;
        }

        // build layouter with exact match highlights
        // IMPORTANT: highlights are re-derived from `text` (the ground-truth
        // string passed to the layouter each call) rather than pre-computed snapshot
        // my reasoning for this is to avoid the "shift on insert/delete" thing where the
        // snapshot char-offsets breaks once the TextEdit mutates the
        // buffer during the same frame
        let search_vis     = self.search.visible;
        let search_query   = if search_vis { self.search.query.clone() } else { String::new() };
        let search_current = self.search.current;
        let font_id_cap    = font_id.clone();

        let mut layouter = move |ui: &egui::Ui, text: &str, wrap_width: f32| {
            let mut job = highlight_avr(text, &font_id_cap);

            if !search_query.is_empty() {
                let q_lo: Vec<char> = search_query.to_lowercase().chars().collect();
                let t_lo: Vec<char> = text.to_lowercase().chars().collect();
                let qm = q_lo.len();
                let tn = t_lo.len();
                let mut ci        = 0usize;
                let mut match_idx = 0usize;
                while ci + qm <= tn {
                    if t_lo[ci..ci + qm] == q_lo[..] {
                        let bs  = char_to_byte(text, ci);
                        let be  = char_to_byte(text, ci + qm);
                        let col = if match_idx == search_current {
                            theme::MATCH_CUR
                        } else {
                            theme::MATCH_DIM
                        };
                        apply_highlight(&mut job, bs, be, col);
                        match_idx += 1;
                        ci += qm;
                    } else {
                        ci += 1;
                    }
                }
            }

            job.wrap.max_width = wrap_width;
            ui.fonts(|f| f.layout_job(job))
        };

        // scrol area
        let scroll_offset = self.search.next_scroll.take();
        let mut sa = ScrollArea::vertical()
            .id_salt("editor_scroll")
            .auto_shrink([false, false]);
        if let Some(y) = scroll_offset {
            sa = sa.scroll_offset(Vec2::new(0.0, y));
        }

        sa.show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal_top(|ui| {
                // gutter
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                    ui.set_width(gutter_w);
                    for i in 0..n {
                        let is_current = i == current_line;
                        let display = if is_current {
                            format!("{}", i + 1)
                        } else {
                            let dist = (i as isize - current_line as isize).unsigned_abs();
                            format!("{dist}")
                        };
                        let color = if is_current { Color32::WHITE } else { START_GREEN };
                        ui.allocate_ui_with_layout(
                            egui::vec2(gutter_w, row_h),
                            Layout::right_to_left(Align::Center),
                            |ui| {
                                ui.label(
                                    egui::RichText::new(display)
                                        .font(font_id.clone())
                                        .color(color),
                                );
                            },
                        );
                    }
                });

                // text edit
                let output = TextEdit::multiline(&mut self.source)
                    .id(self.id)
                    .frame(false)
                    .code_editor()
                    .margin(Margin::ZERO)
                    .background_color(Color32::BLACK)
                    .desired_width(ui.available_width())
                    .desired_rows(1)
                    .layouter(&mut layouter)
                    .show(ui);

                let galley = output.galley.clone();
                let galley_pos = output.galley_pos;
                let cursor_range: Option<CursorRange> = output.cursor_range;

                self.board_inline_accept_ok = false;

                if let Some(ref cr_line) = cursor_range {
                    self.cursor_line = cr_line.primary.pcursor.paragraph;
                }

                if let Some(ref cr) = cursor_range {
                    if cr.is_empty() {
                        let line_idx = cr.primary.pcursor.paragraph;
                        let cursor_c = cr.primary.ccursor.index;
                        if let Some(line) = text_lines(&self.source).get(line_idx) {
                            if parse_dot_board_line(line)
                                .and_then(|(_, p)| board_ghost_suffix(p))
                                .is_some()
                                && cursor_at_line_end(&self.source, line_idx, cursor_c)
                            {
                                self.board_inline_accept_ok = true;
                            }
                        }
                    }
                }

                if show_ghost_hint && self.source.is_empty() {
                    use eframe::egui::text::{LayoutJob, TextFormat};
                    // VS Code–style: one quiet line in the first cell, same font as editor (no busy block).
                    let mut job = LayoutJob::default();
                    job.append(
                        ".board ATmega328P",
                        0.0,
                        TextFormat {
                            font_id: font_id.clone(),
                            color: theme::EDITOR_PLACEHOLDER,
                            italics: false,
                            ..Default::default()
                        },
                    );
                    let hint_galley = ui.fonts(|f| f.layout_job(job));
                    let pos = galley_pos + Vec2::new(4.0, 2.0);
                    ui.painter().galley(pos, hint_galley, Color32::WHITE);
                } else {
                    if let Some(ref cr) = cursor_range {
                        if cr.is_empty() {
                            let line_idx = cr.primary.pcursor.paragraph;
                            let cursor_c = cr.primary.ccursor.index;
                            if let Some(line) = text_lines(&self.source).get(line_idx) {
                                if let Some(suffix) = parse_dot_board_line(line)
                                    .and_then(|(_, p)| board_ghost_suffix(p))
                                {
                                    if cursor_at_line_end(&self.source, line_idx, cursor_c) {
                                        use eframe::egui::text::{LayoutJob, TextFormat};
                                        let ghost_color = theme::EDITOR_PLACEHOLDER;
                                        let mut job = LayoutJob::default();
                                        job.append(
                                            suffix,
                                            0.0,
                                            TextFormat {
                                                font_id: font_id.clone(),
                                                color: ghost_color,
                                                italics: true,
                                                ..Default::default()
                                            },
                                        );
                                        let g = ui.fonts(|f| f.layout_job(job));
                                        let r = galley.pos_from_cursor(&cr.primary);
                                        let pos = galley_pos + r.min.to_vec2();
                                        ui.painter().galley(pos, g, Color32::WHITE);
                                    }
                                }
                            }
                        }
                    }
                }
            });
        });

        if bg_resp.clicked() {
            ui.ctx().memory_mut(|mem| mem.request_focus(self.id));
        }

        if self.search.visible {
            let char_w = ui.fonts(|f| f.glyph_width(&font_id, '0'));
            // Width tracks query length; keep empty state compact (≈2+ chars), cap very long queries.
            let q_chars = self.search.query.chars().count();
            let input_w = (char_w * (q_chars.max(2) + 2) as f32).clamp(36.0, 520.0);

            // Inset from the editor panel’s top-right (not the window — avoids sitting on the toolbar).
            let margin_x = 8.0_f32;
            let margin_y = 6.0_f32;
            let snap_src = self.source.clone();
            let find_pivot = editor_rect.right_top() + Vec2::new(-margin_x, margin_y);

            egui::Area::new(self.id.with("search_area"))
                .fixed_pos(find_pivot)
                .pivot(Align2::RIGHT_TOP)
                .constrain_to(editor_rect)
                .order(Order::Foreground)
                .interactable(true)
                .show(ui.ctx(), |ui| {
                    search_bar_frame().show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing = Vec2::new(10.0, 0.0);
                            ui.label(
                                RichText::new("Find")
                                    .monospace()
                                    .size(11.0)
                                    .color(theme::ACCENT_DIM),
                            );

                            let query_resp = modal_single_line_edit_with_id(
                                ui,
                                &mut self.search.query,
                                Some(self.search.id),
                                input_w,
                            );

                            if self.search.needs_focus {
                                query_resp.request_focus();
                                self.search.needs_focus = false;
                            }

                            if self.search.select_all_on_focus
                                && ui.ctx().memory(|m| m.has_focus(self.search.id))
                            {
                                if let Some(mut ts) = TextEdit::load_state(ui.ctx(), self.search.id) {
                                    let n = self.search.query.chars().count();
                                    ts.cursor.set_char_range(Some(CCursorRange::two(
                                        CCursor::new(0),
                                        CCursor::new(n),
                                    )));
                                    TextEdit::store_state(ui.ctx(), self.search.id, ts);
                                    self.search.select_all_on_focus = false;
                                }
                            }

                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing = Vec2::new(0.0, 2.0);
                                if modal_btn_secondary(ui, "▲").clicked() {
                                    self.search.navigate(-1, &snap_src, row_h);
                                }
                                if modal_btn_secondary(ui, "▼").clicked() {
                                    self.search.navigate(1, &snap_src, row_h);
                                }
                            });

                            let count_str = if self.search.query.is_empty() {
                                "—".to_string()
                            } else if self.search.matches.is_empty() {
                                "0/0".to_string()
                            } else {
                                format!(
                                    "{}/{}",
                                    self.search.current + 1,
                                    self.search.matches.len()
                                )
                            };
                            ui.label(
                                RichText::new(count_str)
                                    .monospace()
                                    .size(11.0)
                                    .color(theme::ACCENT_DIM),
                            );

                            if modal_btn_secondary(ui, "✕").clicked() {
                                self.search.close();
                                self.focus_next_frame();
                            }
                        });
                    });
                });
        }
    }
}

fn line_count(text: &str) -> usize {
    if text.is_empty() { 1 } else { text.split('\n').count() }
}

/// `str::lines()` yields nothing for `""`, but the editor has one logical line.
fn text_lines(source: &str) -> Vec<&str> {
    if source.is_empty() {
        return vec![""];
    }
    source.lines().collect()
}

fn parse_dot_board_line(line: &str) -> Option<(&str, &str)> {
    let indent_len = line.len().saturating_sub(line.trim_start().len());
    let indent = &line[..indent_len];
    let rest = line[indent_len..].trim_start();
    if rest.len() < 6 || !rest[..6].eq_ignore_ascii_case(".board") {
        return None;
    }
    Some((indent, rest[6..].trim_start()))
}

/// Inline ghost suffix after `.board` (VS Code style). Default chip is ATmega328P when empty.
fn board_ghost_suffix(partial: &str) -> Option<&'static str> {
    let p = partial.trim();
    if p.is_empty() {
        return Some("ATmega328P");
    }
    let p_low = p.to_ascii_lowercase();
    let candidates = ["ATmega328P", "ATmega128A"];
    let mut suffs: Vec<&'static str> = Vec::new();
    for c in candidates {
        if c.to_ascii_lowercase().starts_with(&p_low) && c.len() > p.len() {
            suffs.push(&c[p.len()..]);
        }
    }
    match suffs.len() {
        0 => None,
        1 => Some(suffs[0]),
        2 if p.len() == 6 && p.eq_ignore_ascii_case("ATmega") => Some("328P"),
        2 => Some(suffs[0]),
        _ => None,
    }
}

fn replace_line_in_source(source: &mut String, line_idx: usize, new_line: &str) {
    let lines = text_lines(source);
    let had_trailing_nl = source.ends_with('\n');
    let mut out = String::new();
    for (i, l) in lines.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if i == line_idx {
            out.push_str(new_line);
        } else {
            out.push_str(l);
        }
    }
    if had_trailing_nl {
        out.push('\n');
    }
    *source = out;
}

fn line_end_char_index(source: &str, line_idx: usize) -> usize {
    let lines = text_lines(source);
    let mut pos = 0usize;
    for (i, l) in lines.iter().enumerate() {
        if i == line_idx {
            return pos + l.chars().count();
        }
        pos += l.chars().count() + 1;
    }
    0
}

fn cursor_at_line_end(source: &str, line_idx: usize, cursor_char: usize) -> bool {
    let lines = text_lines(source);
    let mut pos = 0usize;
    for (i, l) in lines.iter().enumerate() {
        if i == line_idx {
            let line_end = pos + l.chars().count();
            return cursor_char == line_end;
        }
        pos += l.chars().count() + 1;
    }
    false
}

