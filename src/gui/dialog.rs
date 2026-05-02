//! Native message-box helpers for use inside wizard callbacks.
//!
//! These functions show a modal dialog parented to the current active window
//! (typically the wizard). All functions block until the user dismisses the
//! dialog.

use anyhow::Result;

#[cfg(feature = "gui-win32")]
use winsafe::{co, prelude::*, HWND};

#[cfg(feature = "gui-win32")]
fn show_win32(title: &str, message: &str, flags: co::MB) -> Result<co::DLGID> {
    let parent = HWND::GetActiveWindow().unwrap_or(HWND::NULL);
    parent
        .MessageBox(message, title, flags)
        .map_err(|e| anyhow::anyhow!("{e}"))
}

#[cfg(all(feature = "gui-gtk", not(feature = "gui-win32")))]
fn show_gtk(
    title: &str,
    message: &str,
    kind: gtk::MessageType,
    buttons: gtk::ButtonsType,
) -> Result<gtk::ResponseType> {
    use gtk::prelude::*;
    crate::gui::gtk::disable_setlocale_once();
    gtk::init().map_err(|e| anyhow::anyhow!("gtk init failed: {e}"))?;
    crate::gui::gtk::apply_default_window_icon();
    let parent = gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Window>().ok())
        .find(|w| w.is_active());
    let dialog = gtk::MessageDialog::new(
        parent.as_ref(),
        gtk::DialogFlags::MODAL | gtk::DialogFlags::DESTROY_WITH_PARENT,
        kind,
        buttons,
        message,
    );
    dialog.set_title(title);
    let response = dialog.run();
    unsafe {
        dialog.destroy();
    }
    Ok(response)
}

/// Show an informational dialog with an OK button.
pub fn info(title: impl AsRef<str>, message: impl AsRef<str>) -> Result<()> {
    let (title, message) = (title.as_ref(), message.as_ref());
    #[cfg(feature = "gui-win32")]
    {
        show_win32(title, message, co::MB::OK | co::MB::ICONINFORMATION).map(|_| ())
    }
    #[cfg(all(feature = "gui-gtk", not(feature = "gui-win32")))]
    {
        show_gtk(title, message, gtk::MessageType::Info, gtk::ButtonsType::Ok).map(|_| ())
    }
    #[cfg(not(any(feature = "gui-win32", feature = "gui-gtk")))]
    {
        let _ = (title, message);
        Err(anyhow::anyhow!("No dialog backend available"))
    }
}

/// Show a warning dialog with an OK button.
pub fn warn(title: impl AsRef<str>, message: impl AsRef<str>) -> Result<()> {
    let (title, message) = (title.as_ref(), message.as_ref());
    #[cfg(feature = "gui-win32")]
    {
        show_win32(title, message, co::MB::OK | co::MB::ICONWARNING).map(|_| ())
    }
    #[cfg(all(feature = "gui-gtk", not(feature = "gui-win32")))]
    {
        show_gtk(
            title,
            message,
            gtk::MessageType::Warning,
            gtk::ButtonsType::Ok,
        )
        .map(|_| ())
    }
    #[cfg(not(any(feature = "gui-win32", feature = "gui-gtk")))]
    {
        let _ = (title, message);
        Err(anyhow::anyhow!("No dialog backend available"))
    }
}

/// Show an error dialog with an OK button.
pub fn error(title: impl AsRef<str>, message: impl AsRef<str>) -> Result<()> {
    let (title, message) = (title.as_ref(), message.as_ref());
    #[cfg(feature = "gui-win32")]
    {
        show_win32(title, message, co::MB::OK | co::MB::ICONERROR).map(|_| ())
    }
    #[cfg(all(feature = "gui-gtk", not(feature = "gui-win32")))]
    {
        show_gtk(
            title,
            message,
            gtk::MessageType::Error,
            gtk::ButtonsType::Ok,
        )
        .map(|_| ())
    }
    #[cfg(not(any(feature = "gui-win32", feature = "gui-gtk")))]
    {
        let _ = (title, message);
        Err(anyhow::anyhow!("No dialog backend available"))
    }
}

/// Show a language-chooser dialog with a dropdown of `(code, display_name)`
/// entries. Returns the selected code, or `None` if the user cancelled.
///
/// Typical use before the wizard is built:
///
/// ```rust,ignore
/// if let Some(code) = installrs::gui::choose_language(
///     "Language",
///     "Please choose your language:",
///     &[("en", "English"), ("es", "Español"), ("de", "Deutsch")],
///     Some("en"),
/// )? {
///     rust_i18n::set_locale(&code);
/// }
/// ```
pub fn choose_language(
    title: impl AsRef<str>,
    prompt: impl AsRef<str>,
    choices: &[(&str, &str)],
    default_code: Option<&str>,
) -> Result<Option<String>> {
    let (title, prompt) = (title.as_ref(), prompt.as_ref());
    #[cfg(feature = "gui-win32")]
    {
        choose_language_win32(title, prompt, choices, default_code)
    }
    #[cfg(all(feature = "gui-gtk", not(feature = "gui-win32")))]
    {
        choose_language_gtk(title, prompt, choices, default_code)
    }
    #[cfg(not(any(feature = "gui-win32", feature = "gui-gtk")))]
    {
        let _ = (title, prompt, choices, default_code);
        Err(anyhow::anyhow!("No dialog backend available"))
    }
}

#[cfg(all(feature = "gui-gtk", not(feature = "gui-win32")))]
fn choose_language_gtk(
    title: &str,
    prompt: &str,
    choices: &[(&str, &str)],
    default_code: Option<&str>,
) -> Result<Option<String>> {
    use gtk::prelude::*;
    crate::gui::gtk::disable_setlocale_once();
    gtk::init().map_err(|e| anyhow::anyhow!("gtk init failed: {e}"))?;
    crate::gui::gtk::apply_default_window_icon();

    let dialog = gtk::Dialog::with_buttons(
        Some(title),
        None::<&gtk::Window>,
        gtk::DialogFlags::MODAL,
        &[("OK", gtk::ResponseType::Ok)],
    );
    dialog.set_default_response(gtk::ResponseType::Ok);
    dialog.set_default_size(360, -1);

    let content = dialog.content_area();
    content.set_spacing(8);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(16);
    content.set_margin_end(16);

    let label = gtk::Label::new(Some(prompt));
    label.set_xalign(0.0);
    label.set_halign(gtk::Align::Start);
    content.pack_start(&label, false, false, 0);

    let combo = gtk::ComboBoxText::new();
    for (code, name) in choices {
        combo.append(Some(code), name);
    }
    let default = default_code.and_then(|def| {
        if choices.iter().any(|(c, _)| *c == def) {
            Some(def)
        } else {
            None
        }
    });
    if let Some(def) = default {
        combo.set_active_id(Some(def));
    } else if !choices.is_empty() {
        combo.set_active(Some(0));
    }
    content.pack_start(&combo, false, false, 0);

    content.show_all();

    // Focus the OK button so Enter immediately accepts the default
    // selection. set_default_response alone makes Enter activate OK,
    // but only when focus is not on a widget that consumes Return
    // itself (the combo's popup, etc.) — explicit grab is safer.
    if let Some(ok_btn) = dialog.widget_for_response(gtk::ResponseType::Ok) {
        ok_btn.grab_focus();
    }

    let response = dialog.run();
    let selected = if response == gtk::ResponseType::Ok {
        combo.active_id().map(|s| s.to_string())
    } else {
        None
    };

    unsafe {
        dialog.destroy();
    }
    while gtk::events_pending() {
        gtk::main_iteration();
    }

    Ok(selected)
}

#[cfg(feature = "gui-win32")]
fn choose_language_win32(
    title: &str,
    prompt: &str,
    choices: &[(&str, &str)],
    default_code: Option<&str>,
) -> Result<Option<String>> {
    use std::cell::RefCell;
    use std::rc::Rc;
    use winsafe::{co as wco, gui as wgui, prelude::*};

    const W: i32 = 320;
    const H: i32 = 110;
    const PAD: i32 = 12;
    const BTN_W: i32 = 80;
    const BTN_H: i32 = 26;

    // Load the app icon from embedded resources (resource ID 1, set by
    // winresource) so the dialog's title bar and taskbar entry match the
    // main wizard. Falls back to no icon if the resource is missing.
    let class_icon = {
        use winsafe::HINSTANCE;
        let hinst = HINSTANCE::GetModuleHandle(None).unwrap_or(HINSTANCE::NULL);
        match hinst.LoadIcon(winsafe::IdIdiStr::Id(1)) {
            Ok(mut hicon) => wgui::Icon::Handle(hicon.leak()),
            Err(_) => wgui::Icon::None,
        }
    };

    // We use a standalone `WindowMain` rather than `WindowModal`: the language
    // picker runs before the wizard exists, so there's no parent for a modal.
    // `run_main` blocks until the window closes, which is what we want.
    let wnd = wgui::WindowMain::new(wgui::WindowMainOpts {
        title,
        size: wgui::dpi(W, H),
        class_icon,
        style: wco::WS::CAPTION | wco::WS::SYSMENU | wco::WS::VISIBLE | wco::WS::CLIPCHILDREN,
        ..Default::default()
    });

    let _prompt_label = wgui::Label::new(
        &wnd,
        wgui::LabelOpts {
            text: prompt,
            position: wgui::dpi(PAD, PAD),
            size: wgui::dpi(W - 2 * PAD, 20),
            ..Default::default()
        },
    );

    let items: Vec<&str> = choices.iter().map(|(_, n)| *n).collect();
    let combo = wgui::ComboBox::new(
        &wnd,
        wgui::ComboBoxOpts {
            position: wgui::dpi(PAD, PAD + 24),
            width: wgui::dpi(W - 2 * PAD, 0).0,
            items: &items,
            ..Default::default()
        },
    );

    let codes: Vec<String> = choices.iter().map(|(c, _)| c.to_string()).collect();
    let default_idx = default_code
        .and_then(|d| choices.iter().position(|(c, _)| *c == d))
        .unwrap_or(0);

    let btn_ok = wgui::Button::new(
        &wnd,
        wgui::ButtonOpts {
            text: "OK",
            position: wgui::dpi(W - BTN_W - PAD, H - BTN_H - PAD),
            width: wgui::dpi(BTN_W, 0).0,
            height: wgui::dpi(0, BTN_H).1,
            control_style: wco::BS::DEFPUSHBUTTON,
            ..Default::default()
        },
    );

    {
        let combo_c = combo.clone();
        let btn_ok_c = btn_ok.clone();
        wnd.on().wm_create(move |_| {
            combo_c.items().select(Some(default_idx as u32));
            // Focus the OK button so Enter immediately accepts the
            // default selection. WindowMain doesn't auto-route Enter to
            // BS_DEFPUSHBUTTON the way a true dialog would; this is the
            // explicit equivalent.
            let _ = btn_ok_c.hwnd().SetFocus();
            Ok(0)
        });
    }

    let result: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    {
        let combo_c = combo.clone();
        let wnd_c = wnd.clone();
        let result_c = result.clone();
        let codes_c = codes.clone();
        btn_ok.on().bn_clicked(move || {
            if let Some(idx) = combo_c.items().selected_index() {
                if let Some(code) = codes_c.get(idx as usize) {
                    *result_c.borrow_mut() = Some(code.clone());
                }
            }
            wnd_c.close();
            Ok(())
        });
    }

    wnd.run_main(None).map_err(|e| anyhow::anyhow!("{e}"))?;

    let selected = result.borrow_mut().take();
    Ok(selected)
}

/// Show a Yes/No confirmation dialog. Returns `true` if the user clicked Yes.
pub fn confirm(title: impl AsRef<str>, message: impl AsRef<str>) -> Result<bool> {
    let (title, message) = (title.as_ref(), message.as_ref());
    #[cfg(feature = "gui-win32")]
    {
        let r = show_win32(title, message, co::MB::YESNO | co::MB::ICONQUESTION)?;
        Ok(r == co::DLGID::YES)
    }
    #[cfg(all(feature = "gui-gtk", not(feature = "gui-win32")))]
    {
        let r = show_gtk(
            title,
            message,
            gtk::MessageType::Question,
            gtk::ButtonsType::YesNo,
        )?;
        Ok(r == gtk::ResponseType::Yes)
    }
    #[cfg(not(any(feature = "gui-win32", feature = "gui-gtk")))]
    {
        let _ = (title, message);
        Err(anyhow::anyhow!("No dialog backend available"))
    }
}
