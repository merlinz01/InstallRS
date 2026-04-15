use winsafe::co;
use winsafe::gui;
use winsafe::msg::wm;
use winsafe::prelude::*;
use winsafe::{HBRUSH, HFONT, SIZE};

/// Handle WM_CTLCOLORSTATIC on a parent panel to make all static (label)
/// controls draw with a transparent background.
fn setup_transparent_labels(parent: &gui::WindowControl) {
    parent
        .on()
        .wm_ctl_color_static(move |p: wm::CtlColorStatic| {
            p.hdc.SetBkMode(co::BKMODE::TRANSPARENT)?;
            Ok(HBRUSH::GetStockObject(co::STOCK_BRUSH::NULL)?)
        });
}

/// Discriminant for the page type — stored alongside the panel.
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
                position: gui::dpi(10, 20),
                size: gui::dpi(width - 20, 30),
                ..Default::default()
            },
        );

        let message_label = gui::Label::new(
            parent,
            gui::LabelOpts {
                text: message,
                position: gui::dpi(10, 60),
                size: gui::dpi(width - 20, 200),
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
    _text_edit: gui::Edit,
    accept_check: gui::CheckBox,
}

impl LicensePage {
    pub fn new(parent: &gui::WindowControl, text: &str, width: i32, height: i32) -> Self {
        let edit_height = height - 60;
        let (ew, eh) = gui::dpi(width - 20, edit_height);
        let text_edit = gui::Edit::new(
            parent,
            gui::EditOpts {
                text,
                position: gui::dpi(10, 10),
                width: ew,
                height: eh,
                control_style: co::ES::MULTILINE
                    | co::ES::READONLY
                    | co::ES::AUTOVSCROLL
                    | co::ES::WANTRETURN,
                resize_behavior: (gui::Horz::Resize, gui::Vert::Resize),
                ..Default::default()
            },
        );

        let accept_check = gui::CheckBox::new(
            parent,
            gui::CheckBoxOpts {
                text: "I accept the license agreement",
                position: gui::dpi(10, edit_height + 20),
                size: gui::dpi(width - 20, 20),
                ..Default::default()
            },
        );

        Self {
            _text_edit: text_edit,
            accept_check,
        }
    }

    pub fn is_accepted(&self) -> bool {
        self.accept_check.is_checked()
    }
}

// ── Directory Picker Page ───────────────────────────────────────────────────

pub struct DirectoryPickerPage {
    _label: gui::Label,
    dir_edit: gui::Edit,
    _browse_btn: gui::Button,
}

impl DirectoryPickerPage {
    pub fn new(parent: &gui::WindowControl, default: &str, width: i32, _height: i32) -> Self {
        let label = gui::Label::new(
            parent,
            gui::LabelOpts {
                text: "Install to:",
                position: gui::dpi(10, 20),
                size: gui::dpi(width - 20, 20),
                ..Default::default()
            },
        );

        let browse_width = 80;
        let edit_width = width - 20 - browse_width - 10;
        let (ew, eh) = gui::dpi(edit_width, 24);
        let dir_edit = gui::Edit::new(
            parent,
            gui::EditOpts {
                text: default,
                position: gui::dpi(10, 50),
                width: ew,
                height: eh,
                ..Default::default()
            },
        );

        let (bw, bh) = gui::dpi(browse_width, 26);
        let browse_btn = gui::Button::new(
            parent,
            gui::ButtonOpts {
                text: "Browse...",
                position: gui::dpi(10 + edit_width + 10, 49),
                width: bw,
                height: bh,
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
            _label: label,
            dir_edit,
            _browse_btn: browse_btn,
        }
    }

    pub fn get_directory(&self) -> String {
        self.dir_edit.text().unwrap_or_default()
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
                position: gui::dpi(10, 20),
                size: gui::dpi(width - 20, 20),
                ..Default::default()
            },
        );

        let progress_bar = gui::ProgressBar::new(
            parent,
            gui::ProgressBarOpts {
                position: gui::dpi(10, 50),
                size: gui::dpi(width - 20, 22),
                ..Default::default()
            },
        );

        let (lw, lh) = gui::dpi(width - 20, height - 100);
        let log_edit = gui::Edit::new(
            parent,
            gui::EditOpts {
                position: gui::dpi(10, 82),
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
                position: gui::dpi(10, 20),
                size: gui::dpi(width - 20, 30),
                ..Default::default()
            },
        );

        let message_label = gui::Label::new(
            parent,
            gui::LabelOpts {
                text: message,
                position: gui::dpi(10, 60),
                size: gui::dpi(width - 20, 200),
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
