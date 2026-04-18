use winsafe::co;
use winsafe::gui;
use winsafe::msg::wm;
use winsafe::prelude::*;
use winsafe::{HBRUSH, HFONT, SIZE};

/// Internal padding between the panel edge and its controls.
const PAD: i32 = 20;

/// Handle WM_CTLCOLORSTATIC on a parent panel to make all static (label)
/// controls draw with a transparent background.
fn setup_transparent_labels(parent: &gui::WindowControl) {
    parent
        .on()
        .wm_ctl_color_static(move |p: wm::CtlColorStatic| {
            p.hdc.SetBkMode(co::BKMODE::TRANSPARENT)?;
            Ok(HBRUSH::GetSysColorBrush(co::COLOR::WINDOW)?)
        });
}

/// Discriminant for the page type — stored alongside the panel.
#[allow(dead_code)]
pub enum PageKind {
    Welcome(WelcomePage),
    License(LicensePage),
    Components(ComponentsPage),
    DirectoryPicker(DirectoryPickerPage),
    Install(InstallPage),
    Finish(FinishPage),
}

// ── Welcome Page ────────────────────────────────────────────────────────────

pub struct WelcomePage {
    _title_label: gui::Label,
    _message_label: gui::Label,
}

impl WelcomePage {
    pub fn new(
        parent: &gui::WindowControl,
        title: &str,
        message: &str,
        width: i32,
        _height: i32,
    ) -> Self {
        let title_label = gui::Label::new(
            parent,
            gui::LabelOpts {
                text: title,
                position: gui::dpi(PAD, PAD),
                size: gui::dpi(width - 2 * PAD, 30),
                resize_behavior: (gui::Horz::Resize, gui::Vert::None),
                ..Default::default()
            },
        );

        let message_label = gui::Label::new(
            parent,
            gui::LabelOpts {
                text: message,
                position: gui::dpi(PAD, PAD + 40),
                size: gui::dpi(width - 2 * PAD, 200),
                resize_behavior: (gui::Horz::Resize, gui::Vert::Resize),
                ..Default::default()
            },
        );

        // Set bold + larger font on title after the panel is created.
        {
            let title_c = title_label.clone();
            parent.on().wm_create(move |_| {
                let mut bold_font = HFONT::CreateFont(
                    SIZE { cx: 0, cy: -18 },
                    0,
                    0,
                    co::FW::BOLD,
                    false,
                    false,
                    false,
                    co::CHARSET::DEFAULT,
                    co::OUT_PRECIS::DEFAULT,
                    co::CLIP::DEFAULT_PRECIS,
                    co::QUALITY::DEFAULT,
                    co::PITCH::DEFAULT,
                    "Segoe UI",
                )?;
                unsafe {
                    title_c.hwnd().SendMessage(wm::SetFont {
                        hfont: bold_font.leak(),
                        redraw: true,
                    });
                }
                Ok(0)
            });
        }

        // Transparent label backgrounds.
        setup_transparent_labels(parent);

        Self {
            _title_label: title_label,
            _message_label: message_label,
        }
    }
}

// ── License Page ────────────────────────────────────────────────────────────

pub struct LicensePage {
    _heading_label: gui::Label,
    _text_edit: gui::Edit,
    accept_check: gui::CheckBox,
}

impl LicensePage {
    pub fn new(
        parent: &gui::WindowControl,
        heading: &str,
        text: &str,
        accept_label: &str,
        width: i32,
        height: i32,
    ) -> Self {
        // Win32 Edit controls require \r\n line endings.
        let text = &text.replace("\r\n", "\n").replace('\n', "\r\n");

        let heading_label = gui::Label::new(
            parent,
            gui::LabelOpts {
                text: heading,
                position: gui::dpi(PAD, PAD),
                size: gui::dpi(width - 2 * PAD, 24),
                resize_behavior: (gui::Horz::Resize, gui::Vert::None),
                ..Default::default()
            },
        );

        // Bold + larger font for the heading.
        {
            let heading_c = heading_label.clone();
            parent.on().wm_create(move |_| {
                let mut bold_font = HFONT::CreateFont(
                    SIZE { cx: 0, cy: -18 },
                    0,
                    0,
                    co::FW::BOLD,
                    false,
                    false,
                    false,
                    co::CHARSET::DEFAULT,
                    co::OUT_PRECIS::DEFAULT,
                    co::CLIP::DEFAULT_PRECIS,
                    co::QUALITY::DEFAULT,
                    co::PITCH::DEFAULT,
                    "Segoe UI",
                )?;
                unsafe {
                    heading_c.hwnd().SendMessage(wm::SetFont {
                        hfont: bold_font.leak(),
                        redraw: true,
                    });
                }
                Ok(0)
            });
        }

        let edit_y = PAD + 24 + 10;
        let edit_height = height - edit_y - 10 - 20 - PAD;
        let (ew, eh) = gui::dpi(width - 2 * PAD, edit_height);
        let text_edit = gui::Edit::new(
            parent,
            gui::EditOpts {
                text,
                position: gui::dpi(PAD, edit_y),
                width: ew,
                height: eh,
                control_style: co::ES::MULTILINE
                    | co::ES::READONLY
                    | co::ES::AUTOVSCROLL
                    | co::ES::WANTRETURN,
                window_style: co::WS::CHILD
                    | co::WS::GROUP
                    | co::WS::TABSTOP
                    | co::WS::VISIBLE
                    | co::WS::VSCROLL,
                resize_behavior: (gui::Horz::Resize, gui::Vert::Resize),
                ..Default::default()
            },
        );

        let accept_check = gui::CheckBox::new(
            parent,
            gui::CheckBoxOpts {
                text: accept_label,
                position: gui::dpi(PAD, edit_y + edit_height + 10),
                size: gui::dpi(width - 2 * PAD, 20),
                resize_behavior: (gui::Horz::Resize, gui::Vert::Repos),
                ..Default::default()
            },
        );

        setup_transparent_labels(parent);

        Self {
            _heading_label: heading_label,
            _text_edit: text_edit,
            accept_check,
        }
    }

    pub fn is_accepted(&self) -> bool {
        self.accept_check.is_checked()
    }

    /// Register a callback to run whenever the acceptance checkbox is clicked.
    pub fn on_accept_changed<F>(&self, f: F)
    where
        F: Fn() + 'static,
    {
        self.accept_check.on().bn_clicked(move || {
            f();
            Ok(())
        });
    }
}

// ── Directory Picker Page ───────────────────────────────────────────────────

pub struct DirectoryPickerPage {
    _heading_label: gui::Label,
    _label: gui::Label,
    dir_edit: gui::Edit,
    _browse_btn: gui::Button,
}

impl DirectoryPickerPage {
    pub fn new(
        parent: &gui::WindowControl,
        heading: &str,
        label_text: &str,
        default: &str,
        width: i32,
        _height: i32,
    ) -> Self {
        let heading_label = gui::Label::new(
            parent,
            gui::LabelOpts {
                text: heading,
                position: gui::dpi(PAD, PAD),
                size: gui::dpi(width - 2 * PAD, 24),
                resize_behavior: (gui::Horz::Resize, gui::Vert::None),
                ..Default::default()
            },
        );

        // Bold + larger font for the heading.
        {
            let heading_c = heading_label.clone();
            parent.on().wm_create(move |_| {
                let mut bold_font = HFONT::CreateFont(
                    SIZE { cx: 0, cy: -18 },
                    0,
                    0,
                    co::FW::BOLD,
                    false,
                    false,
                    false,
                    co::CHARSET::DEFAULT,
                    co::OUT_PRECIS::DEFAULT,
                    co::CLIP::DEFAULT_PRECIS,
                    co::QUALITY::DEFAULT,
                    co::PITCH::DEFAULT,
                    "Segoe UI",
                )?;
                unsafe {
                    heading_c.hwnd().SendMessage(wm::SetFont {
                        hfont: bold_font.leak(),
                        redraw: true,
                    });
                }
                Ok(0)
            });
        }

        let label_y = PAD + 24 + 20;
        let label = gui::Label::new(
            parent,
            gui::LabelOpts {
                text: label_text,
                position: gui::dpi(PAD, label_y),
                size: gui::dpi(width - 2 * PAD, 20),
                resize_behavior: (gui::Horz::Resize, gui::Vert::None),
                ..Default::default()
            },
        );

        let edit_y = label_y + 25;
        let browse_width = 80;
        let edit_width = width - 2 * PAD - browse_width - 10;
        let (ew, eh) = gui::dpi(edit_width, 24);
        let dir_edit = gui::Edit::new(
            parent,
            gui::EditOpts {
                text: default,
                position: gui::dpi(PAD, edit_y),
                width: ew,
                height: eh,
                resize_behavior: (gui::Horz::Resize, gui::Vert::None),
                ..Default::default()
            },
        );

        let (bw, bh) = gui::dpi(browse_width, 26);
        let browse_btn = gui::Button::new(
            parent,
            gui::ButtonOpts {
                text: "Browse...",
                position: gui::dpi(PAD + edit_width + 10, edit_y - 1),
                width: bw,
                height: bh,
                resize_behavior: (gui::Horz::Repos, gui::Vert::None),
                ..Default::default()
            },
        );

        // Wire up browse button to open a folder dialog.
        {
            let dir_edit_c = dir_edit.clone();
            let parent_c = parent.clone();
            browse_btn.on().bn_clicked(move || {
                let _guard = winsafe::CoInitializeEx(co::COINIT::APARTMENTTHREADED)?;

                let dlg = winsafe::CoCreateInstance::<winsafe::IFileOpenDialog>(
                    &co::CLSID::FileOpenDialog,
                    None::<&winsafe::IUnknown>,
                    co::CLSCTX::INPROC_SERVER,
                )?;

                let opts = dlg.GetOptions()?;
                dlg.SetOptions(opts | co::FOS::PICKFOLDERS)?;

                let user_clicked_ok = dlg.Show(parent_c.hwnd())?;
                if user_clicked_ok {
                    let item = dlg.GetResult()?;
                    let path = item.GetDisplayName(co::SIGDN::FILESYSPATH)?;
                    dir_edit_c.set_text(&path)?;
                }

                Ok(())
            });
        }

        setup_transparent_labels(parent);

        Self {
            _heading_label: heading_label,
            _label: label,
            dir_edit,
            _browse_btn: browse_btn,
        }
    }

    pub fn get_directory(&self) -> String {
        self.dir_edit.text().unwrap_or_default()
    }
}

// ── Components Page ─────────────────────────────────────────────────────────

pub struct ComponentsPage {
    _heading_label: gui::Label,
    _label: gui::Label,
    /// (component id, checkbox)
    checks: Vec<(String, gui::CheckBox)>,
}

impl ComponentsPage {
    pub fn new(
        parent: &gui::WindowControl,
        heading: &str,
        label_text: &str,
        components: &[crate::Component],
        width: i32,
        _height: i32,
    ) -> Self {
        let heading_label = gui::Label::new(
            parent,
            gui::LabelOpts {
                text: heading,
                position: gui::dpi(PAD, PAD),
                size: gui::dpi(width - 2 * PAD, 24),
                resize_behavior: (gui::Horz::Resize, gui::Vert::None),
                ..Default::default()
            },
        );

        {
            let heading_c = heading_label.clone();
            parent.on().wm_create(move |_| {
                let mut bold_font = HFONT::CreateFont(
                    SIZE { cx: 0, cy: -18 },
                    0,
                    0,
                    co::FW::BOLD,
                    false,
                    false,
                    false,
                    co::CHARSET::DEFAULT,
                    co::OUT_PRECIS::DEFAULT,
                    co::CLIP::DEFAULT_PRECIS,
                    co::QUALITY::DEFAULT,
                    co::PITCH::DEFAULT,
                    "Segoe UI",
                )?;
                unsafe {
                    heading_c.hwnd().SendMessage(wm::SetFont {
                        hfont: bold_font.leak(),
                        redraw: true,
                    });
                }
                Ok(0)
            });
        }

        let label_y = PAD + 24 + 20;
        let label = gui::Label::new(
            parent,
            gui::LabelOpts {
                text: label_text,
                position: gui::dpi(PAD, label_y),
                size: gui::dpi(width - 2 * PAD, 20),
                resize_behavior: (gui::Horz::Resize, gui::Vert::None),
                ..Default::default()
            },
        );

        let mut checks: Vec<(String, gui::CheckBox, bool, bool)> = Vec::new();
        let mut y = label_y + 28;
        for c in components {
            let display = if c.required {
                format!("{} (required)", c.label)
            } else {
                c.label.clone()
            };
            let cb = gui::CheckBox::new(
                parent,
                gui::CheckBoxOpts {
                    text: &display,
                    position: gui::dpi(PAD, y),
                    size: gui::dpi(width - 2 * PAD, 22),
                    resize_behavior: (gui::Horz::Resize, gui::Vert::None),
                    ..Default::default()
                },
            );
            checks.push((c.id.clone(), cb, c.selected, c.required));
            y += 24;
        }

        // Apply initial state after the controls are created.
        {
            let checks_c: Vec<(gui::CheckBox, bool, bool)> = checks
                .iter()
                .map(|(_, cb, sel, req)| (cb.clone(), *sel, *req))
                .collect();
            parent.on().wm_create(move |_| {
                for (cb, selected, required) in &checks_c {
                    cb.set_check(*selected);
                    if *required {
                        cb.hwnd().EnableWindow(false);
                    }
                }
                Ok(0)
            });
        }

        setup_transparent_labels(parent);

        let checks = checks.into_iter().map(|(id, cb, _, _)| (id, cb)).collect();

        Self {
            _heading_label: heading_label,
            _label: label,
            checks,
        }
    }

    /// Current selections, in order: `(component_id, is_checked)`.
    pub fn selections(&self) -> Vec<(String, bool)> {
        self.checks
            .iter()
            .map(|(id, cb)| (id.clone(), cb.is_checked()))
            .collect()
    }
}

// ── Install Page ────────────────────────────────────────────────────────────

pub struct InstallPage {
    status_label: gui::Label,
    progress_bar: gui::ProgressBar,
    log_edit: gui::Edit,
}

impl InstallPage {
    pub fn new(parent: &gui::WindowControl, width: i32, height: i32) -> Self {
        let status_label = gui::Label::new(
            parent,
            gui::LabelOpts {
                text: "Waiting to start...",
                position: gui::dpi(PAD, PAD),
                size: gui::dpi(width - 2 * PAD, 20),
                resize_behavior: (gui::Horz::Resize, gui::Vert::None),
                ..Default::default()
            },
        );

        let progress_bar = gui::ProgressBar::new(
            parent,
            gui::ProgressBarOpts {
                position: gui::dpi(PAD, PAD + 30),
                size: gui::dpi(width - 2 * PAD, 22),
                resize_behavior: (gui::Horz::Resize, gui::Vert::None),
                ..Default::default()
            },
        );

        let (lw, lh) = gui::dpi(width - 2 * PAD, height - 100 - PAD);
        let log_edit = gui::Edit::new(
            parent,
            gui::EditOpts {
                position: gui::dpi(PAD, PAD + 62),
                width: lw,
                height: lh,
                control_style: co::ES::MULTILINE
                    | co::ES::READONLY
                    | co::ES::AUTOVSCROLL
                    | co::ES::WANTRETURN,
                resize_behavior: (gui::Horz::Resize, gui::Vert::Resize),
                ..Default::default()
            },
        );

        setup_transparent_labels(parent);

        Self {
            status_label,
            progress_bar,
            log_edit,
        }
    }

    pub fn set_status(&self, status: &str) {
        let _ = self.status_label.hwnd().SetWindowText(status);
    }

    pub fn set_progress(&self, progress: f64) {
        let pos = (progress.clamp(0.0, 1.0) * 100.0) as u32;
        self.progress_bar.set_position(pos);
    }

    pub fn append_log(&self, message: &str) {
        let current = self.log_edit.text().unwrap_or_default();
        let new_text = if current.is_empty() {
            message.to_string()
        } else {
            format!("{current}\r\n{message}")
        };
        let _ = self.log_edit.set_text(&new_text);
    }
}

// ── Finish Page ─────────────────────────────────────────────────────────────

pub struct FinishPage {
    _title_label: gui::Label,
    _message_label: gui::Label,
}

impl FinishPage {
    pub fn new(
        parent: &gui::WindowControl,
        title: &str,
        message: &str,
        width: i32,
        _height: i32,
    ) -> Self {
        let title_label = gui::Label::new(
            parent,
            gui::LabelOpts {
                text: title,
                position: gui::dpi(PAD, PAD),
                size: gui::dpi(width - 2 * PAD, 30),
                resize_behavior: (gui::Horz::Resize, gui::Vert::None),
                ..Default::default()
            },
        );

        let message_label = gui::Label::new(
            parent,
            gui::LabelOpts {
                text: message,
                position: gui::dpi(PAD, PAD + 40),
                size: gui::dpi(width - 2 * PAD, 200),
                resize_behavior: (gui::Horz::Resize, gui::Vert::Resize),
                ..Default::default()
            },
        );

        // Set bold + larger font on title after the panel is created.
        {
            let title_c = title_label.clone();
            parent.on().wm_create(move |_| {
                let mut bold_font = HFONT::CreateFont(
                    SIZE { cx: 0, cy: -18 },
                    0,
                    0,
                    co::FW::BOLD,
                    false,
                    false,
                    false,
                    co::CHARSET::DEFAULT,
                    co::OUT_PRECIS::DEFAULT,
                    co::CLIP::DEFAULT_PRECIS,
                    co::QUALITY::DEFAULT,
                    co::PITCH::DEFAULT,
                    "Segoe UI",
                )?;
                unsafe {
                    title_c.hwnd().SendMessage(wm::SetFont {
                        hfont: bold_font.leak(),
                        redraw: true,
                    });
                }
                Ok(0)
            });
        }

        // Transparent label backgrounds.
        setup_transparent_labels(parent);

        Self {
            _title_label: title_label,
            _message_label: message_label,
        }
    }
}
