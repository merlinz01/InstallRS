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
    DirectoryPicker(DirectoryPickerPage),
    Install(InstallPage),
    Finish(FinishPage),
}

// ── Welcome Page ────────────────────────────────────────────────────────────

pub struct WelcomePage {
    widget: gtk::Box,
}

impl WelcomePage {
    pub fn new(title: &str, message: &str) -> Self {
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, SPACING);
        set_page_margins(&vbox);

        vbox.pack_start(&bold_heading(title, "x-large"), false, false, 0);

        let msg = gtk::Label::new(Some(message));
        msg.set_xalign(0.0);
        msg.set_halign(gtk::Align::Start);
        msg.set_line_wrap(true);
        vbox.pack_start(&msg, true, true, 0);

        Self { widget: vbox }
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.widget
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
}

impl DirectoryPickerPage {
    pub fn new(heading: &str, label_text: &str, default: &str) -> Self {
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
        }
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.widget
    }

    pub fn get_directory(&self) -> String {
        self.entry.text().to_string()
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

// ── Finish Page ─────────────────────────────────────────────────────────────

pub struct FinishPage {
    widget: gtk::Box,
}

impl FinishPage {
    pub fn new(title: &str, message: &str) -> Self {
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, SPACING);
        set_page_margins(&vbox);

        vbox.pack_start(&bold_heading(title, "x-large"), false, false, 0);

        let msg = gtk::Label::new(Some(message));
        msg.set_xalign(0.0);
        msg.set_halign(gtk::Align::Start);
        msg.set_line_wrap(true);
        vbox.pack_start(&msg, true, true, 0);

        Self { widget: vbox }
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.widget
    }
}
