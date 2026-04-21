use winsafe::co;
use winsafe::gui;
use winsafe::msg::{lvm, wm};
use winsafe::prelude::*;
use winsafe::{HBRUSH, HFONT, LVHITTESTINFO, LVITEM, SIZE, TRACKMOUSEEVENT};

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
    Error(ErrorPage),
    Custom(CustomPage),
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
    list: gui::ListView,
    _desc_label: gui::Label,
    ids: Vec<String>,
}

impl ComponentsPage {
    pub fn new(
        parent: &gui::WindowControl,
        heading: &str,
        label_text: &str,
        components: &[crate::Component],
        width: i32,
        height: i32,
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

        // ListView with checkboxes: native scrolling + standard Windows idiom.
        // Reserve space at the bottom for a hover-description label.
        const DESC_H: i32 = 48;
        let list_y = label_y + 28;
        let list_h = height - list_y - DESC_H - 10 - PAD;
        let list_w = width - 2 * PAD;
        let col_width: i32 = list_w - 24; // leave room for the scrollbar
        let cols: [(&str, i32); 1] = [("Component", col_width)];
        let list = gui::ListView::new(
            parent,
            gui::ListViewOpts {
                position: gui::dpi(PAD, list_y),
                size: gui::dpi(list_w, list_h),
                control_style: co::LVS::REPORT
                    | co::LVS::NOCOLUMNHEADER
                    | co::LVS::SHOWSELALWAYS
                    | co::LVS::SINGLESEL,
                control_ex_style: co::LVS_EX::CHECKBOXES,
                columns: &cols,
                resize_behavior: (gui::Horz::Resize, gui::Vert::Resize),
                ..Default::default()
            },
        );

        let ids: Vec<String> = components.iter().map(|c| c.id.clone()).collect();
        let required: Vec<bool> = components.iter().map(|c| c.required).collect();

        // Populate rows + initial check state after the control is created.
        {
            let list_c = list.clone();
            let initial: Vec<(String, bool)> = components
                .iter()
                .map(|c| {
                    let text = if c.required {
                        format!("{} (required)", c.label)
                    } else {
                        c.label.clone()
                    };
                    (text, c.selected)
                })
                .collect();
            parent.on().wm_create(move |_| {
                for (idx, (text, selected)) in initial.iter().enumerate() {
                    list_c.items().add(&[text.as_str()], None, ())?;
                    set_lv_check(&list_c, idx as u32, *selected);
                }
                Ok(0)
            });
        }

        // Block unchecking required rows via LVN_ITEMCHANGING — the canonical
        // Win32 way. Returning `true` tells Windows to reject the change.
        // Only block the checked→unchecked transition, so the *initial*
        // programmatic set (state image 0 → 2) still goes through; otherwise
        // required rows would render without any checkbox at all.
        {
            let required_c = required.clone();
            list.on().lvn_item_changing(move |p| {
                if p.uChanged.has(co::LVIF::STATE) {
                    let idx = p.iItem as usize;
                    if idx < required_c.len() && required_c[idx] {
                        let old_img = p.uOldState.raw() & 0xF000;
                        let new_img = p.uNewState.raw() & 0xF000;
                        if old_img == 0x2000 && new_img == 0x1000 {
                            return Ok(true);
                        }
                    }
                }
                Ok(false)
            });
        }

        // Custom-draw required rows with the system "grey text" color so
        // they read as disabled — the ListView doesn't have a per-row
        // disabled state, so this is the standard approach.
        {
            let required_c = required.clone();
            list.on().nm_custom_draw(move |p| match p.mcd.dwDrawStage {
                co::CDDS::PREPAINT => Ok(co::CDRF::NOTIFYITEMDRAW),
                co::CDDS::ITEMPREPAINT => {
                    let idx = p.mcd.dwItemSpec;
                    if idx < required_c.len() && required_c[idx] {
                        p.clrText = winsafe::GetSysColor(co::COLOR::GRAYTEXT);
                        return Ok(co::CDRF::NEWFONT);
                    }
                    Ok(co::CDRF::DODEFAULT)
                }
                _ => Ok(co::CDRF::DODEFAULT),
            });
        }

        // Description label below the list, updated on hover.
        let desc_y = list_y + list_h + 10;
        let desc_label = gui::Label::new(
            parent,
            gui::LabelOpts {
                text: "",
                position: gui::dpi(PAD, desc_y),
                size: gui::dpi(width - 2 * PAD, DESC_H),
                resize_behavior: (gui::Horz::Resize, gui::Vert::Repos),
                ..Default::default()
            },
        );

        // On hover, update the description label with the component's text.
        // We use WM_MOUSEMOVE + LVM_HITTEST instead of LVN_HOTTRACK because
        // LVN_HOTTRACK only fires for the label region — hovering over the
        // checkbox state icon yields iItem = -1. A manual hit test with
        // LVHITTESTINFO covers the whole row (icon + state + label).
        {
            let desc_c = desc_label.clone();
            let list_c = list.clone();
            let descriptions: Vec<String> = components
                .iter()
                .map(|c| {
                    if c.description.is_empty() {
                        c.label.clone()
                    } else {
                        c.description.clone()
                    }
                })
                .collect();
            let list_leave = list.clone();
            list.on_subclass().wm_mouse_move(move |p| {
                let mut hti = LVHITTESTINFO::default();
                hti.pt = p.coords;
                let idx = unsafe {
                    list_c
                        .hwnd()
                        .SendMessage(lvm::HitTest { info: &mut hti })
                };
                let text = match idx {
                    Some(i) if (i as usize) < descriptions.len() => {
                        descriptions[i as usize].as_str()
                    }
                    _ => "",
                };
                let _ = desc_c.hwnd().SetWindowText(text);

                // Re-arm TrackMouseEvent so WM_MOUSELEAVE fires when the
                // cursor exits the list. It's a one-shot notification —
                // must be re-requested on every mouse move.
                let mut tme = TRACKMOUSEEVENT::default();
                tme.dwFlags = co::TME::LEAVE;
                tme.hwndTrack = unsafe { list_leave.hwnd().raw_copy() };
                let _ = winsafe::TrackMouseEvent(&mut tme);
                Ok(())
            });

            // Clear the description label when the cursor leaves the list.
            let desc_leave = desc_label.clone();
            list.on_subclass().wm_mouse_leave(move || {
                let _ = desc_leave.hwnd().SetWindowText("");
                Ok(())
            });
        }

        setup_transparent_labels(parent);

        Self {
            _heading_label: heading_label,
            _label: label,
            list,
            _desc_label: desc_label,
            ids,
        }
    }

    /// Current selections, in order: `(component_id, is_checked)`.
    pub fn selections(&self) -> Vec<(String, bool)> {
        self.ids
            .iter()
            .enumerate()
            .map(|(i, id)| (id.clone(), get_lv_check(&self.list, i as u32)))
            .collect()
    }
}

// ListView check-state helpers. winsafe's `ListViewItem` has no
// `set_checked`/`is_checked`, so we go through LVM_{SET,GET}ITEMSTATE with
// `LVIS_STATEIMAGEMASK` directly — state image index 2 = checked, 1 = unchecked.

fn set_lv_check(list: &gui::ListView, index: u32, checked: bool) {
    let raw_state: u32 = if checked { 0x2000 } else { 0x1000 };
    let mut lvi = LVITEM::default();
    lvi.stateMask = co::LVIS::STATEIMAGEMASK;
    lvi.state = unsafe { co::LVIS::from_raw(raw_state) };
    let _ = unsafe {
        list.hwnd().SendMessage(lvm::SetItemState {
            index: Some(index),
            lvitem: &lvi,
        })
    };
}

fn get_lv_check(list: &gui::ListView, index: u32) -> bool {
    let state = unsafe {
        list.hwnd().SendMessage(lvm::GetItemState {
            index,
            mask: co::LVIS::STATEIMAGEMASK,
        })
    };
    (state.raw() & 0xF000) == 0x2000
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

// ── Custom Page ─────────────────────────────────────────────────────────────

enum CustomControl {
    Text { edit: gui::Edit },
    Checkbox { check: gui::CheckBox },
    Dropdown { combo: gui::ComboBox, values: Vec<String> },
}

pub struct CustomPage {
    _heading_label: gui::Label,
    _label: gui::Label,
    controls: Vec<(String, CustomControl)>,
    _extras: Vec<gui::Label>,
}

impl CustomPage {
    pub fn new(
        parent: &gui::WindowControl,
        heading: &str,
        label_text: &str,
        widgets: &[crate::gui::CustomWidget],
        initial: &std::collections::HashMap<String, crate::OptionValue>,
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

        let label = gui::Label::new(
            parent,
            gui::LabelOpts {
                text: label_text,
                position: gui::dpi(PAD, PAD + 28),
                size: gui::dpi(width - 2 * PAD, 20),
                resize_behavior: (gui::Horz::Resize, gui::Vert::None),
                ..Default::default()
            },
        );

        let mut y = PAD + 28 + 26;
        let row_w = width - 2 * PAD;
        let mut controls: Vec<(String, CustomControl)> = Vec::new();
        let mut extras: Vec<gui::Label> = Vec::new();
        // Collect per-widget initial states to apply in a single wm_create
        // handler — winsafe only keeps one wm_create registration per parent.
        let mut initial_checks: Vec<(gui::CheckBox, bool)> = Vec::new();
        let mut initial_dropdowns: Vec<(gui::ComboBox, usize)> = Vec::new();

        for w in widgets {
            use crate::gui::CustomWidget;
            match w {
                CustomWidget::Text {
                    key,
                    label: lbl,
                    default,
                    password,
                } => {
                    let lbl_ctl = gui::Label::new(
                        parent,
                        gui::LabelOpts {
                            text: lbl,
                            position: gui::dpi(PAD, y),
                            size: gui::dpi(row_w, 18),
                            resize_behavior: (gui::Horz::Resize, gui::Vert::None),
                            ..Default::default()
                        },
                    );
                    let (ew, eh) = gui::dpi(row_w, 24);
                    let initial_text = match initial.get(key) {
                        Some(crate::OptionValue::String(s)) => s.clone(),
                        _ => default.clone(),
                    };
                    let style = if *password {
                        co::ES::AUTOHSCROLL | co::ES::PASSWORD
                    } else {
                        co::ES::AUTOHSCROLL
                    };
                    let edit = gui::Edit::new(
                        parent,
                        gui::EditOpts {
                            text: &initial_text,
                            position: gui::dpi(PAD, y + 20),
                            width: ew,
                            height: eh,
                            control_style: style,
                            resize_behavior: (gui::Horz::Resize, gui::Vert::None),
                            ..Default::default()
                        },
                    );
                    controls.push((key.clone(), CustomControl::Text { edit }));
                    extras.push(lbl_ctl);
                    y += 52;
                }
                CustomWidget::Checkbox {
                    key,
                    label: lbl,
                    default,
                } => {
                    let initial_val = match initial.get(key) {
                        Some(crate::OptionValue::Flag(b)) | Some(crate::OptionValue::Bool(b)) => {
                            *b
                        }
                        _ => *default,
                    };
                    let check = gui::CheckBox::new(
                        parent,
                        gui::CheckBoxOpts {
                            text: lbl,
                            position: gui::dpi(PAD, y + 4),
                            size: gui::dpi(row_w, 20),
                            resize_behavior: (gui::Horz::Resize, gui::Vert::None),
                            ..Default::default()
                        },
                    );
                    initial_checks.push((check.clone(), initial_val));
                    controls.push((key.clone(), CustomControl::Checkbox { check }));
                    y += 32;
                }
                CustomWidget::Dropdown {
                    key,
                    label: lbl,
                    choices,
                    default,
                } => {
                    let lbl_ctl = gui::Label::new(
                        parent,
                        gui::LabelOpts {
                            text: lbl,
                            position: gui::dpi(PAD, y),
                            size: gui::dpi(row_w, 18),
                            resize_behavior: (gui::Horz::Resize, gui::Vert::None),
                            ..Default::default()
                        },
                    );
                    let current = match initial.get(key) {
                        Some(crate::OptionValue::String(s)) => s.clone(),
                        _ => default.clone(),
                    };
                    let idx = choices
                        .iter()
                        .position(|(v, _)| *v == current)
                        .unwrap_or(0);
                    let items: Vec<&str> = choices.iter().map(|(_, d)| d.as_str()).collect();
                    let values: Vec<String> =
                        choices.iter().map(|(v, _)| v.clone()).collect();
                    let (cw, _) = gui::dpi(row_w, 0);
                    let combo = gui::ComboBox::new(
                        parent,
                        gui::ComboBoxOpts {
                            position: gui::dpi(PAD, y + 20),
                            width: cw,
                            items: &items,
                            resize_behavior: (gui::Horz::Resize, gui::Vert::None),
                            ..Default::default()
                        },
                    );
                    initial_dropdowns.push((combo.clone(), idx));
                    controls.push((key.clone(), CustomControl::Dropdown { combo, values }));
                    extras.push(lbl_ctl);
                    y += 52;
                }
            }
        }

        // Single wm_create handler: apply the heading font, then seed every
        // checkbox / dropbox's initial state in one shot.
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
                for (check, val) in &initial_checks {
                    check.set_check(*val);
                }
                for (combo, idx) in &initial_dropdowns {
                    combo.items().select(Some(*idx as u32));
                }
                Ok(0)
            });
        }

        setup_transparent_labels(parent);

        Self {
            _heading_label: heading_label,
            _label: label,
            controls,
            _extras: extras,
        }
    }

    /// Read the current value of every widget, keyed by option name. The
    /// wizard calls this on forward navigation and stores each entry via
    /// [`crate::Installer::set_option_value`].
    pub fn collect_values(&self) -> Vec<(String, crate::OptionValue)> {
        let mut out = Vec::new();
        for (key, ctl) in &self.controls {
            let val = match ctl {
                CustomControl::Text { edit } => {
                    crate::OptionValue::String(edit.text().unwrap_or_default())
                }
                CustomControl::Checkbox { check } => crate::OptionValue::Bool(check.is_checked()),
                CustomControl::Dropdown { combo, values } => {
                    let idx = combo.items().selected_index().unwrap_or(0) as usize;
                    let v = values.get(idx).cloned().unwrap_or_default();
                    crate::OptionValue::String(v)
                }
            };
            out.push((key.clone(), val));
        }
        out
    }
}

// ── Error Page ──────────────────────────────────────────────────────────────

pub struct ErrorPage {
    _title_label: gui::Label,
    _message_label: gui::Label,
    error_edit: gui::Edit,
}

impl ErrorPage {
    pub fn new(
        parent: &gui::WindowControl,
        title: &str,
        message: &str,
        width: i32,
        height: i32,
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
                size: gui::dpi(width - 2 * PAD, 40),
                resize_behavior: (gui::Horz::Resize, gui::Vert::None),
                ..Default::default()
            },
        );

        let edit_y = PAD + 40 + 48;
        let (ew, eh) = gui::dpi(width - 2 * PAD, height - edit_y - PAD);
        let error_edit = gui::Edit::new(
            parent,
            gui::EditOpts {
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

        setup_transparent_labels(parent);

        Self {
            _title_label: title_label,
            _message_label: message_label,
            error_edit,
        }
    }

    pub fn set_error_text(&self, text: &str) {
        let text = text.replace("\r\n", "\n").replace('\n', "\r\n");
        let _ = self.error_edit.set_text(&text);
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
