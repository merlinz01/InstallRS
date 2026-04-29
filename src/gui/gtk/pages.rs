use gtk::prelude::*;

const PAD: i32 = 20;
const SPACING: i32 = 10;

fn set_page_margins(w: &gtk::Box) {
    w.set_margin_top(PAD);
    w.set_margin_bottom(PAD);
    w.set_margin_start(PAD);
    w.set_margin_end(PAD);
}

fn bold_heading(text: &str, size: &str) -> gtk::Label {
    let label = gtk::Label::new(None);
    label.set_markup(&format!(
        "<span weight='bold' size='{size}'>{}</span>",
        glib::markup_escape_text(text)
    ));
    label.set_xalign(0.0);
    label.set_halign(gtk::Align::Start);
    label
}

#[allow(dead_code)]
pub enum PageKind {
    Welcome(WelcomePage),
    License(LicensePage),
    Components(ComponentsPage),
    DirectoryPicker(DirectoryPickerPage),
    Install(InstallPage),
    Finish(FinishPage),
    Error(ErrorPage),
    Custom(CustomPage),
}

// ── Welcome Page ────────────────────────────────────────────────────────────

pub struct WelcomePage {
    widget: gtk::Box,
    controls: Vec<(String, CustomControl)>,
}

impl WelcomePage {
    pub fn new(
        title: &str,
        message: &str,
        widgets: &[crate::gui::CustomWidget],
        initial: &std::collections::HashMap<String, crate::OptionValue>,
    ) -> Self {
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, SPACING);
        set_page_margins(&vbox);

        vbox.pack_start(&bold_heading(title, "x-large"), false, false, 0);

        let msg = gtk::Label::new(Some(message));
        msg.set_xalign(0.0);
        msg.set_yalign(0.0);
        msg.set_halign(gtk::Align::Start);
        msg.set_valign(gtk::Align::Start);
        msg.set_line_wrap(true);
        vbox.pack_start(&msg, widgets.is_empty(), widgets.is_empty(), 0);

        let controls = if widgets.is_empty() {
            Vec::new()
        } else {
            let inner = gtk::Box::new(gtk::Orientation::Vertical, 6);
            inner.set_margin_top(12);
            inner.set_valign(gtk::Align::Start);
            let c = pack_widget_column(&inner, widgets, initial);
            vbox.pack_start(&inner, true, true, 0);
            c
        };

        Self {
            widget: vbox,
            controls,
        }
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.widget
    }

    pub fn collect_values(&self) -> Vec<(String, crate::OptionValue)> {
        collect_widget_values(&self.controls)
    }
}

// ── License Page ────────────────────────────────────────────────────────────

pub struct LicensePage {
    widget: gtk::Box,
    accept_check: gtk::CheckButton,
}

impl LicensePage {
    pub fn new(heading: &str, text: &str, accept_label: &str) -> Self {
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, SPACING);
        set_page_margins(&vbox);

        vbox.pack_start(&bold_heading(heading, "large"), false, false, 0);

        let scrolled = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .shadow_type(gtk::ShadowType::In)
            .build();
        let text_view = gtk::TextView::new();
        text_view.set_editable(false);
        text_view.set_cursor_visible(false);
        text_view.set_wrap_mode(gtk::WrapMode::Word);
        text_view
            .buffer()
            .expect("TextView has no buffer")
            .set_text(text);
        scrolled.add(&text_view);
        vbox.pack_start(&scrolled, true, true, 0);

        let accept_check = gtk::CheckButton::with_label(accept_label);
        vbox.pack_start(&accept_check, false, false, 0);

        Self {
            widget: vbox,
            accept_check,
        }
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.widget
    }

    pub fn is_accepted(&self) -> bool {
        self.accept_check.is_active()
    }

    pub fn on_accept_changed<F>(&self, f: F)
    where
        F: Fn() + 'static,
    {
        self.accept_check.connect_toggled(move |_| f());
    }
}

// ── Directory Picker Page ───────────────────────────────────────────────────

pub struct DirectoryPickerPage {
    widget: gtk::Box,
    entry: gtk::Entry,
    key: String,
}

impl DirectoryPickerPage {
    pub fn new(heading: &str, label_text: &str, key: &str, initial: &str) -> Self {
        let default = initial;
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, SPACING);
        set_page_margins(&vbox);

        vbox.pack_start(&bold_heading(heading, "large"), false, false, 0);

        let label = gtk::Label::new(Some(label_text));
        label.set_xalign(0.0);
        label.set_halign(gtk::Align::Start);
        vbox.pack_start(&label, false, false, 0);

        let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let entry = gtk::Entry::new();
        entry.set_text(default);
        entry.set_hexpand(true);
        hbox.pack_start(&entry, true, true, 0);

        let browse_btn = gtk::Button::with_label("Browse...");
        hbox.pack_start(&browse_btn, false, false, 0);
        vbox.pack_start(&hbox, false, false, 0);

        {
            let entry_c = entry.clone();
            browse_btn.connect_clicked(move |btn| {
                let parent = btn
                    .toplevel()
                    .and_then(|w| w.downcast::<gtk::Window>().ok());
                let dialog = gtk::FileChooserDialog::with_buttons(
                    Some("Select Folder"),
                    parent.as_ref(),
                    gtk::FileChooserAction::SelectFolder,
                    &[
                        ("Cancel", gtk::ResponseType::Cancel),
                        ("Select", gtk::ResponseType::Accept),
                    ],
                );
                let current = entry_c.text().to_string();
                if !current.is_empty() {
                    let _ = dialog.set_current_folder(&current);
                }
                let response = dialog.run();
                if response == gtk::ResponseType::Accept {
                    if let Some(path) = dialog.filename() {
                        entry_c.set_text(&path.to_string_lossy());
                    }
                }
                unsafe {
                    dialog.destroy();
                }
            });
        }

        Self {
            widget: vbox,
            entry,
            key: key.to_string(),
        }
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.widget
    }

    pub fn get_directory(&self) -> String {
        self.entry.text().to_string()
    }

    pub fn key(&self) -> &str {
        &self.key
    }
}

// ── Components Page ─────────────────────────────────────────────────────────

pub struct ComponentsPage {
    widget: gtk::Box,
    checks: Vec<(String, gtk::CheckButton)>,
    _desc_label: gtk::Label,
}

impl ComponentsPage {
    pub fn new(heading: &str, label_text: &str, components: &[crate::Component]) -> Self {
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, SPACING);
        set_page_margins(&vbox);

        vbox.pack_start(&bold_heading(heading, "large"), false, false, 0);

        let label = gtk::Label::new(Some(label_text));
        label.set_xalign(0.0);
        label.set_halign(gtk::Align::Start);
        vbox.pack_start(&label, false, false, 0);

        let scrolled = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .shadow_type(gtk::ShadowType::In)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .min_content_height(120)
            .build();
        let list_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
        list_box.set_margin_top(6);
        list_box.set_margin_bottom(6);
        list_box.set_margin_start(6);
        list_box.set_margin_end(6);
        list_box.set_valign(gtk::Align::Start);

        let desc_label = gtk::Label::new(None);
        desc_label.set_xalign(0.0);
        desc_label.set_yalign(0.0);
        desc_label.set_halign(gtk::Align::Start);
        desc_label.set_valign(gtk::Align::Start);
        desc_label.set_line_wrap(true);
        desc_label.set_size_request(-1, 40);

        let mut checks: Vec<(String, gtk::CheckButton)> = Vec::new();
        for c in components {
            let display = if c.required {
                format!("{} (required)", c.label)
            } else {
                c.label.clone()
            };
            let cb = gtk::CheckButton::with_label(&display);
            cb.set_active(c.selected);
            if c.required {
                // Keep the widget sensitive so it still receives hover events —
                // setting sensitive = false silently swallows enter-notify.
                // Instead, fade it via opacity and revert toggle attempts.
                cb.set_opacity(0.5);
                cb.connect_toggled(|btn| {
                    if !btn.is_active() {
                        btn.set_active(true);
                    }
                });
            }
            cb.add_events(
                gtk::gdk::EventMask::ENTER_NOTIFY_MASK | gtk::gdk::EventMask::LEAVE_NOTIFY_MASK,
            );
            let desc_enter = desc_label.clone();
            let description = if c.description.is_empty() {
                c.label.clone()
            } else {
                c.description.clone()
            };
            cb.connect_enter_notify_event(move |_, _| {
                desc_enter.set_text(&description);
                glib::Propagation::Proceed
            });
            let desc_leave_cb = desc_label.clone();
            cb.connect_leave_notify_event(move |_, _| {
                desc_leave_cb.set_text("");
                glib::Propagation::Proceed
            });
            list_box.pack_start(&cb, false, false, 0);
            checks.push((c.id.clone(), cb));
        }

        scrolled.add(&list_box);
        vbox.pack_start(&scrolled, true, true, 0);
        vbox.pack_start(&desc_label, false, false, 0);

        // Clear the description when the cursor leaves the list.
        scrolled.add_events(gtk::gdk::EventMask::LEAVE_NOTIFY_MASK);
        let desc_leave = desc_label.clone();
        scrolled.connect_leave_notify_event(move |_, _| {
            desc_leave.set_text("");
            glib::Propagation::Proceed
        });

        Self {
            widget: vbox,
            checks,
            _desc_label: desc_label,
        }
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.widget
    }

    pub fn selections(&self) -> Vec<(String, bool)> {
        self.checks
            .iter()
            .map(|(id, cb)| (id.clone(), cb.is_active()))
            .collect()
    }
}

// ── Install Page ────────────────────────────────────────────────────────────

pub struct InstallPage {
    widget: gtk::Box,
    status_label: gtk::Label,
    progress_bar: gtk::ProgressBar,
    log_buffer: gtk::TextBuffer,
    log_view: gtk::TextView,
}

impl InstallPage {
    pub fn new() -> Self {
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, SPACING);
        set_page_margins(&vbox);

        let status_label = gtk::Label::new(Some("Waiting to start..."));
        status_label.set_xalign(0.0);
        status_label.set_halign(gtk::Align::Start);
        vbox.pack_start(&status_label, false, false, 0);

        let progress_bar = gtk::ProgressBar::new();
        vbox.pack_start(&progress_bar, false, false, 0);

        let scrolled = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .shadow_type(gtk::ShadowType::In)
            .build();
        let log_view = gtk::TextView::new();
        log_view.set_editable(false);
        log_view.set_cursor_visible(false);
        log_view.set_wrap_mode(gtk::WrapMode::WordChar);
        let log_buffer = log_view.buffer().expect("TextView has no buffer");
        scrolled.add(&log_view);
        vbox.pack_start(&scrolled, true, true, 0);

        Self {
            widget: vbox,
            status_label,
            progress_bar,
            log_buffer,
            log_view,
        }
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.widget
    }

    pub fn set_status(&self, status: &str) {
        self.status_label.set_text(status);
    }

    pub fn set_progress(&self, progress: f64) {
        self.progress_bar.set_fraction(progress.clamp(0.0, 1.0));
    }

    pub fn append_log(&self, message: &str) {
        let mut end = self.log_buffer.end_iter();
        let text = if self.log_buffer.char_count() == 0 {
            message.to_string()
        } else {
            format!("\n{message}")
        };
        self.log_buffer.insert(&mut end, &text);
        if let Some(end_mark) =
            self.log_buffer
                .create_mark(None, &self.log_buffer.end_iter(), false)
        {
            self.log_view.scroll_to_mark(&end_mark, 0.0, true, 0.0, 1.0);
            self.log_buffer.delete_mark(&end_mark);
        }
    }
}

// ── Custom Page ─────────────────────────────────────────────────────────────

enum CustomControl {
    Text(gtk::Entry),
    Number(gtk::Entry),
    Multiline(gtk::TextBuffer),
    Checkbox(gtk::CheckButton),
    Dropdown {
        combo: gtk::ComboBoxText,
        values: Vec<String>,
    },
    Radio {
        buttons: Vec<gtk::RadioButton>,
        values: Vec<String>,
    },
    PathPicker(gtk::Entry),
}

pub struct CustomPage {
    widget: gtk::Box,
    controls: Vec<(String, CustomControl)>,
}

impl CustomPage {
    pub fn new(
        heading: &str,
        label_text: &str,
        widgets: &[crate::gui::CustomWidget],
        initial: &std::collections::HashMap<String, crate::OptionValue>,
    ) -> Self {
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, SPACING);
        set_page_margins(&vbox);

        vbox.pack_start(&bold_heading(heading, "large"), false, false, 0);

        let label = gtk::Label::new(Some(label_text));
        label.set_xalign(0.0);
        label.set_halign(gtk::Align::Start);
        vbox.pack_start(&label, false, false, 0);

        let inner = gtk::Box::new(gtk::Orientation::Vertical, 6);
        inner.set_margin_top(6);
        inner.set_margin_bottom(6);
        inner.set_valign(gtk::Align::Start);

        let controls = pack_widget_column(&inner, widgets, initial);

        vbox.pack_start(&inner, true, true, 0);

        Self {
            widget: vbox,
            controls,
        }
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.widget
    }

    /// Read the current value of every widget, keyed by option name.
    pub fn collect_values(&self) -> Vec<(String, crate::OptionValue)> {
        collect_widget_values(&self.controls)
    }
}

fn collect_widget_values(
    controls: &[(String, CustomControl)],
) -> Vec<(String, crate::OptionValue)> {
    let mut out = Vec::new();
    for (key, ctl) in controls {
        let val = match ctl {
            CustomControl::Text(entry) => crate::OptionValue::String(entry.text().to_string()),
            CustomControl::Number(entry) => {
                let t = entry.text().to_string();
                crate::OptionValue::Int(t.trim().parse::<i64>().unwrap_or(0))
            }
            CustomControl::Multiline(buffer) => {
                let (start, end) = (buffer.start_iter(), buffer.end_iter());
                let text = buffer.text(&start, &end, false);
                crate::OptionValue::String(text.map(|s| s.to_string()).unwrap_or_default())
            }
            CustomControl::Checkbox(check) => crate::OptionValue::Bool(check.is_active()),
            CustomControl::Dropdown { combo, values } => {
                let idx = combo.active().unwrap_or(0) as usize;
                let v = values.get(idx).cloned().unwrap_or_default();
                crate::OptionValue::String(v)
            }
            CustomControl::Radio { buttons, values } => {
                let idx = buttons.iter().position(|b| b.is_active()).unwrap_or(0);
                let v = values.get(idx).cloned().unwrap_or_default();
                crate::OptionValue::String(v)
            }
            CustomControl::PathPicker(entry) => {
                crate::OptionValue::String(entry.text().to_string())
            }
        };
        out.push((key.clone(), val));
    }
    out
}

fn pack_widget_column(
    inner: &gtk::Box,
    widgets: &[crate::gui::CustomWidget],
    initial: &std::collections::HashMap<String, crate::OptionValue>,
) -> Vec<(String, CustomControl)> {
    let mut controls: Vec<(String, CustomControl)> = Vec::new();
    {
        for w in widgets {
            use crate::gui::CustomWidget;
            match w {
                CustomWidget::Text {
                    key,
                    label: lbl,
                    default,
                    password,
                } => {
                    let lbl_ctl = gtk::Label::new(Some(lbl));
                    lbl_ctl.set_xalign(0.0);
                    lbl_ctl.set_halign(gtk::Align::Start);
                    inner.pack_start(&lbl_ctl, false, false, 0);

                    let entry = gtk::Entry::new();
                    let initial_text = match initial.get(key) {
                        Some(crate::OptionValue::String(s)) => s.clone(),
                        _ => default.clone(),
                    };
                    entry.set_text(&initial_text);
                    if *password {
                        entry.set_visibility(false);
                        entry.set_invisible_char(Some('\u{2022}'));
                    }
                    inner.pack_start(&entry, false, false, 0);
                    controls.push((key.clone(), CustomControl::Text(entry)));
                }
                CustomWidget::Checkbox {
                    key,
                    label: lbl,
                    default,
                } => {
                    let check = gtk::CheckButton::with_label(lbl);
                    let initial_val = match initial.get(key) {
                        Some(crate::OptionValue::Flag(b)) | Some(crate::OptionValue::Bool(b)) => *b,
                        _ => *default,
                    };
                    check.set_active(initial_val);
                    inner.pack_start(&check, false, false, 0);
                    controls.push((key.clone(), CustomControl::Checkbox(check)));
                }
                CustomWidget::Dropdown {
                    key,
                    label: lbl,
                    choices,
                    default,
                } => {
                    let lbl_ctl = gtk::Label::new(Some(lbl));
                    lbl_ctl.set_xalign(0.0);
                    lbl_ctl.set_halign(gtk::Align::Start);
                    inner.pack_start(&lbl_ctl, false, false, 0);

                    let combo = gtk::ComboBoxText::new();
                    let values: Vec<String> = choices.iter().map(|(v, _)| v.clone()).collect();
                    for (_, display) in choices {
                        combo.append_text(display);
                    }
                    let current = match initial.get(key) {
                        Some(crate::OptionValue::String(s)) => s.clone(),
                        _ => default.clone(),
                    };
                    let idx = values.iter().position(|v| v == &current).unwrap_or(0);
                    combo.set_active(Some(idx as u32));
                    inner.pack_start(&combo, false, false, 0);
                    controls.push((key.clone(), CustomControl::Dropdown { combo, values }));
                }
                CustomWidget::Radio {
                    key,
                    label: lbl,
                    choices,
                    default,
                } => {
                    let lbl_ctl = gtk::Label::new(Some(lbl));
                    lbl_ctl.set_xalign(0.0);
                    lbl_ctl.set_halign(gtk::Align::Start);
                    inner.pack_start(&lbl_ctl, false, false, 0);

                    let current = match initial.get(key) {
                        Some(crate::OptionValue::String(s)) => s.clone(),
                        _ => default.clone(),
                    };
                    let values: Vec<String> = choices.iter().map(|(v, _)| v.clone()).collect();
                    let mut buttons: Vec<gtk::RadioButton> = Vec::new();
                    let rb_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
                    rb_box.set_margin_start(8);
                    for (idx, (val, disp)) in choices.iter().enumerate() {
                        let rb = if idx == 0 {
                            gtk::RadioButton::with_label(disp)
                        } else {
                            gtk::RadioButton::with_label_from_widget(&buttons[0], disp)
                        };
                        if *val == current {
                            rb.set_active(true);
                        }
                        rb_box.pack_start(&rb, false, false, 0);
                        buttons.push(rb);
                    }
                    inner.pack_start(&rb_box, false, false, 0);
                    controls.push((key.clone(), CustomControl::Radio { buttons, values }));
                }
                CustomWidget::Number {
                    key,
                    label: lbl,
                    default,
                } => {
                    let lbl_ctl = gtk::Label::new(Some(lbl));
                    lbl_ctl.set_xalign(0.0);
                    lbl_ctl.set_halign(gtk::Align::Start);
                    inner.pack_start(&lbl_ctl, false, false, 0);

                    let entry = gtk::Entry::new();
                    // Restrict to optional sign + digits.
                    entry.connect_insert_text(|entry, text, position| {
                        if !text.chars().all(|c| c.is_ascii_digit() || c == '-') {
                            gtk::glib::signal::signal_stop_emission_by_name(entry, "insert-text");
                            let _ = position;
                        }
                    });
                    let initial_text = match initial.get(key) {
                        Some(crate::OptionValue::Int(n)) => n.to_string(),
                        Some(crate::OptionValue::String(s)) => s.clone(),
                        _ => default.to_string(),
                    };
                    entry.set_text(&initial_text);
                    inner.pack_start(&entry, false, false, 0);
                    controls.push((key.clone(), CustomControl::Number(entry)));
                }
                CustomWidget::Multiline {
                    key,
                    label: lbl,
                    default,
                    rows,
                } => {
                    let lbl_ctl = gtk::Label::new(Some(lbl));
                    lbl_ctl.set_xalign(0.0);
                    lbl_ctl.set_halign(gtk::Align::Start);
                    inner.pack_start(&lbl_ctl, false, false, 0);

                    let sw = gtk::ScrolledWindow::builder()
                        .hexpand(true)
                        .shadow_type(gtk::ShadowType::In)
                        .min_content_height((*rows as i32).max(2) * 22)
                        .build();
                    let tv = gtk::TextView::new();
                    tv.set_wrap_mode(gtk::WrapMode::WordChar);
                    tv.set_left_margin(6);
                    tv.set_right_margin(6);
                    tv.set_top_margin(4);
                    tv.set_bottom_margin(4);
                    let buffer = tv.buffer().expect("TextView has no buffer");
                    let initial_text = match initial.get(key) {
                        Some(crate::OptionValue::String(s)) => s.clone(),
                        _ => default.clone(),
                    };
                    buffer.set_text(&initial_text);
                    sw.add(&tv);
                    inner.pack_start(&sw, false, false, 0);
                    controls.push((key.clone(), CustomControl::Multiline(buffer)));
                }
                CustomWidget::FilePicker {
                    key,
                    label: lbl,
                    default,
                    filters,
                } => {
                    let lbl_ctl = gtk::Label::new(Some(lbl));
                    lbl_ctl.set_xalign(0.0);
                    lbl_ctl.set_halign(gtk::Align::Start);
                    inner.pack_start(&lbl_ctl, false, false, 0);

                    let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                    let entry = gtk::Entry::new();
                    entry.set_hexpand(true);
                    let initial_text = match initial.get(key) {
                        Some(crate::OptionValue::String(s)) => s.clone(),
                        _ => default.clone(),
                    };
                    entry.set_text(&initial_text);
                    hbox.pack_start(&entry, true, true, 0);
                    let browse_btn = gtk::Button::with_label("Browse...");
                    hbox.pack_start(&browse_btn, false, false, 0);
                    inner.pack_start(&hbox, false, false, 0);

                    let entry_c = entry.clone();
                    let filters_owned: Vec<(String, String)> = filters.clone();
                    browse_btn.connect_clicked(move |btn| {
                        let parent = btn
                            .toplevel()
                            .and_then(|w| w.downcast::<gtk::Window>().ok());
                        let dialog = gtk::FileChooserDialog::with_buttons(
                            Some("Select File"),
                            parent.as_ref(),
                            gtk::FileChooserAction::Open,
                            &[
                                ("Cancel", gtk::ResponseType::Cancel),
                                ("Select", gtk::ResponseType::Accept),
                            ],
                        );
                        for (name, glob) in &filters_owned {
                            let f = gtk::FileFilter::new();
                            f.set_name(Some(name));
                            for pat in glob.split(';') {
                                let pat = pat.trim();
                                if !pat.is_empty() {
                                    f.add_pattern(pat);
                                }
                            }
                            dialog.add_filter(f);
                        }
                        if dialog.run() == gtk::ResponseType::Accept {
                            if let Some(path) = dialog.filename() {
                                entry_c.set_text(&path.to_string_lossy());
                            }
                        }
                        unsafe {
                            dialog.destroy();
                        }
                    });
                    controls.push((key.clone(), CustomControl::PathPicker(entry)));
                }
                CustomWidget::DirPicker {
                    key,
                    label: lbl,
                    default,
                } => {
                    let lbl_ctl = gtk::Label::new(Some(lbl));
                    lbl_ctl.set_xalign(0.0);
                    lbl_ctl.set_halign(gtk::Align::Start);
                    inner.pack_start(&lbl_ctl, false, false, 0);

                    let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                    let entry = gtk::Entry::new();
                    entry.set_hexpand(true);
                    let initial_text = match initial.get(key) {
                        Some(crate::OptionValue::String(s)) => s.clone(),
                        _ => default.clone(),
                    };
                    entry.set_text(&initial_text);
                    hbox.pack_start(&entry, true, true, 0);
                    let browse_btn = gtk::Button::with_label("Browse...");
                    hbox.pack_start(&browse_btn, false, false, 0);
                    inner.pack_start(&hbox, false, false, 0);

                    let entry_c = entry.clone();
                    browse_btn.connect_clicked(move |btn| {
                        let parent = btn
                            .toplevel()
                            .and_then(|w| w.downcast::<gtk::Window>().ok());
                        let dialog = gtk::FileChooserDialog::with_buttons(
                            Some("Select Folder"),
                            parent.as_ref(),
                            gtk::FileChooserAction::SelectFolder,
                            &[
                                ("Cancel", gtk::ResponseType::Cancel),
                                ("Select", gtk::ResponseType::Accept),
                            ],
                        );
                        let current = entry_c.text().to_string();
                        if !current.is_empty() {
                            let _ = dialog.set_current_folder(&current);
                        }
                        if dialog.run() == gtk::ResponseType::Accept {
                            if let Some(path) = dialog.filename() {
                                entry_c.set_text(&path.to_string_lossy());
                            }
                        }
                        unsafe {
                            dialog.destroy();
                        }
                    });
                    controls.push((key.clone(), CustomControl::PathPicker(entry)));
                }
            }
        }
    }
    controls
}

// ── Error Page ──────────────────────────────────────────────────────────────

pub struct ErrorPage {
    widget: gtk::Box,
    error_buffer: gtk::TextBuffer,
}

impl ErrorPage {
    pub fn new(title: &str, message: &str) -> Self {
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, SPACING);
        set_page_margins(&vbox);

        vbox.pack_start(&bold_heading(title, "x-large"), false, false, 0);

        let msg = gtk::Label::new(Some(message));
        msg.set_xalign(0.0);
        msg.set_halign(gtk::Align::Start);
        msg.set_line_wrap(true);
        vbox.pack_start(&msg, false, false, 0);

        let scrolled = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .shadow_type(gtk::ShadowType::In)
            .build();
        let text_view = gtk::TextView::new();
        text_view.set_editable(false);
        text_view.set_cursor_visible(false);
        text_view.set_wrap_mode(gtk::WrapMode::WordChar);
        text_view.set_monospace(true);
        text_view.set_left_margin(8);
        text_view.set_right_margin(8);
        text_view.set_top_margin(6);
        text_view.set_bottom_margin(6);
        let error_buffer = text_view.buffer().expect("TextView has no buffer");
        scrolled.add(&text_view);
        vbox.pack_start(&scrolled, true, true, 0);

        Self {
            widget: vbox,
            error_buffer,
        }
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.widget
    }

    pub fn set_error_text(&self, text: &str) {
        self.error_buffer.set_text(text);
    }
}

// ── Finish Page ─────────────────────────────────────────────────────────────

pub struct FinishPage {
    widget: gtk::Box,
    controls: Vec<(String, CustomControl)>,
}

impl FinishPage {
    pub fn new(
        title: &str,
        message: &str,
        widgets: &[crate::gui::CustomWidget],
        initial: &std::collections::HashMap<String, crate::OptionValue>,
    ) -> Self {
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, SPACING);
        set_page_margins(&vbox);

        vbox.pack_start(&bold_heading(title, "x-large"), false, false, 0);

        let msg = gtk::Label::new(Some(message));
        msg.set_xalign(0.0);
        msg.set_yalign(0.0);
        msg.set_halign(gtk::Align::Start);
        msg.set_valign(gtk::Align::Start);
        msg.set_line_wrap(true);
        vbox.pack_start(&msg, widgets.is_empty(), widgets.is_empty(), 0);

        let controls = if widgets.is_empty() {
            Vec::new()
        } else {
            let inner = gtk::Box::new(gtk::Orientation::Vertical, 6);
            inner.set_margin_top(12);
            inner.set_valign(gtk::Align::Start);
            let c = pack_widget_column(&inner, widgets, initial);
            vbox.pack_start(&inner, true, true, 0);
            c
        };

        Self {
            widget: vbox,
            controls,
        }
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.widget
    }

    pub fn collect_values(&self) -> Vec<(String, crate::OptionValue)> {
        collect_widget_values(&self.controls)
    }
}
