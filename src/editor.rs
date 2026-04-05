//! editor rel_line_nums neovim_style avr_syntax

use eframe::egui::{
    self, Align, Color32, Id, Key, Layout, Margin, Modifiers, Order, RichText,
    ScrollArea, Stroke, TextEdit, TextStyle, Ui, Vec2,
};

use crate::syntax::highlight_avr;
use crate::welcome::START_GREEN;

const MATCH_DIM:  Color32 = Color32::from_rgb(100, 75,  0);
const MATCH_CUR:  Color32 = Color32::from_rgb(220, 170, 0);
const SEARCH_BG:  Color32 = Color32::from_rgb(0,   12,  0);

pub struct SearchBar {
    pub visible:      bool,
    pub query:        String,
    prev_query:       String,
    /// char-index start of each match
    pub matches:      Vec<usize>,
    pub current:      usize,
    needs_focus:      bool,
    /// If Some(y), scroll area jumps to this offset once.
    pub next_scroll:  Option<f32>,
    /// Set after navigate(); cleared after cursor is pushed once.
    pending_cursor:   bool,
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
            id: parent_id.with("search_input"),
        }
    }

    pub fn open(&mut self) {
        self.visible     = true;
        self.needs_focus = true;
    }

    pub fn close(&mut self) {
        self.visible        = false;
        self.query.clear();
        self.prev_query.clear();
        self.matches.clear();
        self.current        = 0;
        self.pending_cursor = false;
    }

    /// Rebuild match list from source (case-insensitive).
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

    /// Navigate ±1, wrapping, then schedule a scroll and cursor push.
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

/// Split LayoutJob sections at [byte_start, byte_end) and colour only that slice.
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
    pub fn focus_next_frame(&mut self) { self.needs_focus = true; }

    pub fn request_initial_focus(&mut self, ctx: &egui::Context) {
        if self.needs_focus {
            ctx.memory_mut(|mem| mem.request_focus(self.id));
            self.needs_focus = false;
        }
    }

    pub fn show(&mut self, ui: &mut Ui) {
        // -- keyboard shortcuts ------------------------------------------------
        let cmd_f = ui.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                if cfg!(target_os = "macos") { Modifiers::MAC_CMD } else { Modifiers::CTRL },
                Key::F,
            ))
        });
        if cmd_f { self.search.open(); }

        // Escape only closes search when the search input actually has keyboard focus,
        // so normal editor typing / other shortcuts are never swallowed.
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

        let font_id = TextStyle::Monospace.resolve(ui.style());
        let row_h   = ui.fonts(|f| f.row_height(&font_id));
        let n       = line_count(&self.source);

        let digit_cols = (n.max(1).ilog10() + 1).max(3) as usize;
        let gutter_w   =
            ui.fonts(|f| f.glyph_width(&font_id, '0') * digit_cols as f32 + 14.0);

        // capture rect before any allocation (for Area positioning later)
        let editor_rect = ui.available_rect_before_wrap();
        let bg_resp = ui.interact(editor_rect, self.id.with("bg"), egui::Sense::click());

        let current_line = self.cursor_line.min(n.saturating_sub(1));

        // -- rebuild matches ---------------------------------------------------
        if self.search.visible {
            let snap = self.source.clone();
            self.search.rebuild(&snap);
        }

        // -- push TextEdit cursor only when the user explicitly navigated ------
        // (not every frame — that would override normal editor cursor movement)
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

        // -- build layouter with exact match highlights -----------------------
        // IMPORTANT: highlights are re-derived from `text` (the ground-truth
        // string passed to the layouter each call) rather than a pre-computed
        // snapshot. This avoids the "shift on insert/delete" bug where the
        // snapshot char-offsets become stale once the TextEdit mutates the
        // buffer during the same frame.
        let search_vis     = self.search.visible;
        let search_query   = if search_vis { self.search.query.clone() } else { String::new() };
        let search_current = self.search.current;
        let font_id_cap    = font_id.clone();

        let mut layouter = move |ui: &egui::Ui, text: &str, wrap_width: f32| {
            let mut job = highlight_avr(text, &font_id_cap);

            if !search_query.is_empty() {
                // Re-search directly in `text` so offsets are always correct.
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
                        let col = if match_idx == search_current { MATCH_CUR } else { MATCH_DIM };
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

        // -- scroll area -------------------------------------------------------
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

                if let Some(cursor_range) = output.cursor_range {
                    self.cursor_line = cursor_range.primary.pcursor.paragraph;
                }
            });
        });

        if bg_resp.clicked() {
            ui.ctx().memory_mut(|mem| mem.request_focus(self.id));
        }

        // -- floating search widget (top-right corner, over editor) -----------
        if self.search.visible {
            // Dynamic input width: grows with query length, minimum 4 chars wide.
            let char_w   = ui.fonts(|f| f.glyph_width(&font_id, '0'));
            let input_w  = (char_w * (self.search.query.len().max(4) + 2) as f32)
                               .max(50.0)
                               .min(360.0);

            // Area auto-sizes to its content; we position its top-right corner.
            // We'll estimate widget width to anchor it correctly and update each frame.
            let margin_x  = 8.0_f32;
            let margin_y  = 8.0_f32;

            let snap_src  = self.source.clone();
            let row_h_cap = row_h;

            // Use pivot = (1, 0) so the right edge stays pinned to editor_rect.right().
            egui::Area::new(self.id.with("search_area"))
                .anchor(
                    egui::Align2::RIGHT_TOP,
                    Vec2::new(-margin_x, margin_y),
                )
                .order(Order::Foreground)
                .interactable(true)
                .show(ui.ctx(), |ui| {
                    egui::Frame::NONE
                        .fill(SEARCH_BG)
                        .stroke(Stroke::new(1.0, START_GREEN))
                        .inner_margin(Margin::symmetric(6, 5))
                        .corner_radius(egui::CornerRadius::ZERO)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing = Vec2::new(3.0, 0.0);

                                // query input — sized to content
                                let query_resp = ui.add(
                                    TextEdit::singleline(&mut self.search.query)
                                        .id(self.search.id)
                                        .desired_width(input_w)
                                        .frame(false)
                                        .text_color(Color32::WHITE)
                                        .font(TextStyle::Monospace),
                                );

                                if self.search.needs_focus {
                                    query_resp.request_focus();
                                    self.search.needs_focus = false;
                                }

                                if query_resp.lost_focus()
                                    && ui.input(|i| i.key_pressed(Key::Enter))
                                {
                                    self.search.navigate(1, &snap_src, row_h_cap);
                                    query_resp.request_focus();
                                }

                                // tight ▲▼ stacked with zero gap
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing = Vec2::ZERO;
                                    if ui.add(
                                        egui::Button::new(
                                            RichText::new("▲")
                                                .monospace().size(9.0).color(START_GREEN),
                                        )
                                        .frame(false)
                                        .min_size(Vec2::new(13.0, 10.0)),
                                    ).clicked() {
                                        self.search.navigate(-1, &snap_src, row_h_cap);
                                    }
                                    if ui.add(
                                        egui::Button::new(
                                            RichText::new("▼")
                                                .monospace().size(9.0).color(START_GREEN),
                                        )
                                        .frame(false)
                                        .min_size(Vec2::new(13.0, 10.0)),
                                    ).clicked() {
                                        self.search.navigate(1, &snap_src, row_h_cap);
                                    }
                                });

                                // count
                                let count_str = if self.search.query.is_empty() {
                                    "—".to_string()
                                } else if self.search.matches.is_empty() {
                                    "0/0".to_string()
                                } else {
                                    format!("{}/{}", self.search.current + 1, self.search.matches.len())
                                };
                                ui.label(
                                    RichText::new(count_str)
                                        .monospace().size(11.0).color(START_GREEN),
                                );

                                // close button
                                if ui.add(
                                    egui::Button::new(
                                        RichText::new("✕")
                                            .monospace().size(11.0).color(START_GREEN),
                                    )
                                    .frame(false),
                                ).clicked() {
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
