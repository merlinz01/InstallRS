use anyhow::Result;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use gtk::prelude::*;

use super::pages::{
    ComponentsPage, CustomPage, DirectoryPickerPage, ErrorPage, FinishPage, InstallPage,
    LicensePage, PageKind, WelcomePage,
};
use crate::gui::types::{
    ChannelSink, ConfiguredPage, GuiMessage, InstallCallback, OnBeforeLeaveCallback,
    OnEnterCallback, WizardConfig, WizardPage,
};
use crate::Installer;

const WINDOW_WIDTH: i32 = 560;
const WINDOW_HEIGHT: i32 = 400;

struct Page {
    widget: gtk::Box,
    kind: PageKind,
    on_enter: Option<OnEnterCallback>,
    on_before_leave: Option<OnBeforeLeaveCallback>,
    skip_if: Option<crate::gui::types::SkipIfCallback>,
    /// Only meaningful for `PageKind::Install` — when true, the Next button
    /// leading into / shown on this page uses `buttons.uninstall`.
    is_uninstall: bool,
}

/// Find the next visible page strictly after `from`. Evaluates each
/// candidate's `skip_if` predicate; returns None if every remaining page
/// is hidden or `from` is already at the end.
fn next_visible_page(pages: &[Page], from: usize, installer: &Installer) -> Option<usize> {
    let mut i = from.checked_add(1)?;
    while i < pages.len() {
        if pages[i].skip_if.as_ref().is_some_and(|p| p(installer)) {
            i = i.checked_add(1)?;
        } else {
            return Some(i);
        }
    }
    None
}

/// Place the keyboard focus where the user most likely wants it on the
/// given page so Enter advances the wizard. License pages focus the
/// accept checkbox (Space toggles acceptance, Enter on a disabled Next
/// would do nothing). Every other page focuses Next, so users can step
/// through the wizard with repeated Enter presses.
fn set_page_default_focus(page: &Page, btn_next: &gtk::Button) {
    use gtk::prelude::WidgetExt;
    if let PageKind::License(ref lp) = page.kind {
        lp.focus_accept();
    } else {
        btn_next.grab_focus();
    }
}

/// Find the previous visible page strictly before `from`. Returns None
/// when every earlier page is hidden or `from` is already at the start.
fn prev_visible_page(pages: &[Page], from: usize, installer: &Installer) -> Option<usize> {
    let mut i = from.checked_sub(1)?;
    loop {
        if pages[i].skip_if.as_ref().is_some_and(|p| p(installer)) {
            i = i.checked_sub(1)?;
        } else {
            return Some(i);
        }
    }
}

pub fn run(
    config: WizardConfig,
    installer: Arc<Mutex<Installer>>,
    cancelled: Arc<AtomicBool>,
    tx: mpsc::Sender<GuiMessage>,
    rx: mpsc::Receiver<GuiMessage>,
    install_callback: Option<InstallCallback>,
) -> Result<()> {
    // Don't let GTK call setlocale() — it warns when LANG is a short form
    // like "es" (rather than "es_ES.UTF-8") because the C library refuses it.
    // Our app-level i18n is string-key based (rust_i18n), so we don't need
    // C locale configuration either way.
    super::disable_setlocale_once();
    gtk::init().map_err(|e| anyhow::anyhow!("gtk init failed: {e}"))?;
    super::apply_default_window_icon();

    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_title(&config.title);
    window.set_default_size(WINDOW_WIDTH, WINDOW_HEIGHT);
    window.set_size_request(WINDOW_WIDTH, WINDOW_HEIGHT);
    window.set_position(gtk::WindowPosition::Center);

    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
    window.add(&vbox);

    let stack = gtk::Stack::new();
    stack.set_vexpand(true);
    stack.set_hexpand(true);
    vbox.pack_start(&stack, true, true, 0);

    let separator = gtk::Separator::new(gtk::Orientation::Horizontal);
    vbox.pack_start(&separator, false, false, 0);

    // Build pages.
    let mut pages: Vec<Page> = Vec::new();
    for (idx, configured) in config.pages.into_iter().enumerate() {
        let ConfiguredPage {
            page: page_cfg,
            on_enter,
            on_before_leave,
            skip_if,
        } = configured;

        let page_is_uninstall = matches!(
            &page_cfg,
            WizardPage::Install {
                is_uninstall: true,
                ..
            }
        );

        // Snapshot once per page; used by any variant that needs to
        // pre-fill widgets from the options store.
        let initial = installer.lock().unwrap().option_values_snapshot();

        let (widget, kind) = match page_cfg {
            WizardPage::Welcome {
                title,
                message,
                widgets,
            } => {
                let p = WelcomePage::new(&title, &message, &widgets, &initial);
                let w = p.widget().clone();
                (w, PageKind::Welcome(p))
            }
            WizardPage::License {
                heading,
                text,
                accept_label,
            } => {
                let p = LicensePage::new(&heading, &text, &accept_label);
                let w = p.widget().clone();
                (w, PageKind::License(p))
            }
            WizardPage::Components { heading, label } => {
                let comps = installer.lock().unwrap().components().to_vec();
                let p = ComponentsPage::new(&heading, &label, &comps);
                let w = p.widget().clone();
                (w, PageKind::Components(p))
            }
            WizardPage::DirectoryPicker {
                heading,
                label,
                key,
            } => {
                let initial_dir = installer
                    .lock()
                    .unwrap()
                    .get_option::<String>(&key)
                    .unwrap_or_default();
                let p = DirectoryPickerPage::new(&heading, &label, &key, &initial_dir);
                let w = p.widget().clone();
                (w, PageKind::DirectoryPicker(p))
            }
            WizardPage::Install { show_log, .. } => {
                let p = InstallPage::new(show_log);
                let w = p.widget().clone();
                (w, PageKind::Install(p))
            }
            WizardPage::Finish {
                title,
                message,
                widgets,
            } => {
                let p = FinishPage::new(&title, &message, &widgets, &initial);
                let w = p.widget().clone();
                (w, PageKind::Finish(p))
            }
            WizardPage::Error { title, message } => {
                let p = ErrorPage::new(&title, &message);
                let w = p.widget().clone();
                (w, PageKind::Error(p))
            }
            WizardPage::Custom {
                heading,
                label,
                widgets,
            } => {
                let p = CustomPage::new(&heading, &label, &widgets, &initial);
                let w = p.widget().clone();
                (w, PageKind::Custom(p))
            }
        };

        stack.add_named(&widget, &format!("page-{idx}"));
        pages.push(Page {
            widget,
            kind,
            on_enter,
            on_before_leave,
            skip_if,
            is_uninstall: page_is_uninstall,
        });
    }

    // Button bar.
    let btn_bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    btn_bar.set_margin_top(10);
    btn_bar.set_margin_bottom(10);
    btn_bar.set_margin_start(10);
    btn_bar.set_margin_end(10);
    btn_bar.set_halign(gtk::Align::End);

    let btn_back = gtk::Button::with_label(&config.buttons.back);
    let btn_next = gtk::Button::with_label(&config.buttons.next);
    let btn_cancel = gtk::Button::with_label(&config.buttons.cancel);

    btn_back.set_size_request(90, -1);
    btn_next.set_size_request(90, -1);
    btn_cancel.set_size_request(90, -1);

    btn_bar.pack_start(&btn_back, false, false, 0);
    btn_bar.pack_start(&btn_next, false, false, 0);
    btn_bar.pack_start(&btn_cancel, false, false, 0);
    vbox.pack_start(&btn_bar, false, false, 0);

    // Shared state. Rc/RefCell is fine — GTK is single-threaded on the main thread.
    let pages = Rc::new(RefCell::new(pages));
    let current_page = Rc::new(RefCell::new(0usize));
    let install_callback: Rc<RefCell<Option<InstallCallback>>> =
        Rc::new(RefCell::new(install_callback));
    let install_running = Rc::new(AtomicBool::new(false));
    let install_result: Rc<RefCell<Option<Result<()>>>> = Rc::new(RefCell::new(None));
    let install_handle: Rc<RefCell<Option<std::thread::JoinHandle<()>>>> =
        Rc::new(RefCell::new(None));

    // Show the first page.
    if let Some(first) = pages.borrow().first() {
        stack.set_visible_child(&first.widget);
    }

    // update_buttons: refresh sensitivity + next-button label based on current page.
    let update_buttons: Rc<dyn Fn()> = {
        let pages = pages.clone();
        let current = current_page.clone();
        let btn_back_c = btn_back.clone();
        let btn_next_c = btn_next.clone();
        let btn_cancel_c = btn_cancel.clone();
        let install_running_c = install_running.clone();
        let label_next = config.buttons.next.clone();
        let label_install = config.buttons.install.clone();
        let label_uninstall = config.buttons.uninstall.clone();
        let label_finish = config.buttons.finish.clone();
        let installer_btn = installer.clone();

        Rc::new(move || {
            let idx = *current.borrow();
            let pages_b = pages.borrow();
            let is_first = idx == 0;
            let is_install = matches!(&pages_b[idx].kind, PageKind::Install(_));
            let is_finish = matches!(&pages_b[idx].kind, PageKind::Finish(_));
            let is_error = matches!(&pages_b[idx].kind, PageKind::Error(_));
            let is_terminal = is_finish || is_error;
            let next_idx = {
                let inst = installer_btn.lock().unwrap();
                next_visible_page(&pages_b, idx, &inst)
            };
            let next_is_install = next_idx
                .map(|i| matches!(&pages_b[i].kind, PageKind::Install(_)))
                .unwrap_or(false);
            let install_is_uninstall = if is_install {
                pages_b[idx].is_uninstall
            } else if let Some(i) = next_idx.filter(|_| next_is_install) {
                pages_b[i].is_uninstall
            } else {
                false
            };
            let running = install_running_c.load(Ordering::Relaxed);

            btn_back_c.set_sensitive(!is_first && !is_install && !is_terminal);
            let install_label = if install_is_uninstall {
                &label_uninstall
            } else {
                &label_install
            };
            let label = if is_terminal {
                &label_finish
            } else if next_is_install || is_install {
                install_label
            } else {
                &label_next
            };
            btn_next_c.set_label(label);
            btn_next_c.set_sensitive(!running && !is_install && can_advance(&pages_b[idx]));
            btn_cancel_c.set_sensitive(!is_terminal);
        })
    };

    // Wire up license checkboxes to refresh button state.
    {
        let pages_b = pages.borrow();
        for page in pages_b.iter() {
            if let PageKind::License(ref lp) = page.kind {
                let update = update_buttons.clone();
                lp.on_accept_changed(move || update());
            }
        }
    }

    // start_install: consume the install callback and spawn the bg thread.
    let start_install: Rc<dyn Fn()> = {
        let installer_c = installer.clone();
        let tx_c = tx.clone();
        let install_cb = install_callback.clone();
        let install_running_c = install_running.clone();
        let install_handle_c = install_handle.clone();
        let update = update_buttons.clone();

        Rc::new(move || {
            let cb = install_cb.borrow_mut().take();
            if let Some(callback) = cb {
                install_running_c.store(true, Ordering::Relaxed);

                let installer_bg = installer_c.clone();
                let tx_bg = tx_c.clone();

                let handle = std::thread::spawn(move || {
                    let result = {
                        let mut inst = installer_bg.lock().unwrap();
                        inst.set_progress_sink(Box::new(ChannelSink::new(tx_bg.clone())));
                        inst.reset_progress();
                        let r = callback(&mut inst);
                        inst.clear_progress_sink();
                        r
                    };
                    let _ = tx_bg.send(GuiMessage::Finished(result));
                });
                *install_handle_c.borrow_mut() = Some(handle);

                update();
            }
        })
    };

    // Back button.
    {
        let pages_c = pages.clone();
        let current_c = current_page.clone();
        let update = update_buttons.clone();
        let stack_c = stack.clone();
        let installer_back = installer.clone();

        let btn_next_focus = btn_next.clone();
        btn_back.connect_clicked(move |_| {
            let idx = *current_c.borrow();
            if idx == 0 {
                return;
            }

            // on_before_leave / on_enter are intentionally skipped on
            // backward navigation — they fire only on forward moves.
            let pages_b = pages_c.borrow();
            let new_idx_opt = {
                let inst = installer_back.lock().unwrap();
                prev_visible_page(&pages_b, idx, &inst)
            };
            let Some(new_idx) = new_idx_opt else {
                return;
            };
            stack_c.set_visible_child(&pages_b[new_idx].widget);
            drop(pages_b);
            *current_c.borrow_mut() = new_idx;
            update();
            set_page_default_focus(&pages_c.borrow()[new_idx], &btn_next_focus);
        });
    }

    // Next button.
    {
        let pages_c = pages.clone();
        let current_c = current_page.clone();
        let update = update_buttons.clone();
        let start_install_c = start_install.clone();
        let window_c = window.clone();
        let stack_c = stack.clone();
        let installer_c = installer.clone();
        let btn_next_focus = btn_next.clone();

        btn_next.connect_clicked(move |_| {
            let idx = *current_c.borrow();

            // Sync directory picker / components / custom widgets into the
            // installer state before callbacks.
            {
                let pages_b = pages_c.borrow();
                if let PageKind::DirectoryPicker(ref dp) = pages_b[idx].kind {
                    let dir = dp.get_directory();
                    installer_c.lock().unwrap().set_option(dp.key(), dir);
                }
                if let PageKind::Components(ref cp) = pages_b[idx].kind {
                    let sels = cp.selections();
                    let mut inst = installer_c.lock().unwrap();
                    for (id, on) in sels {
                        inst.set_component_selected(&id, on);
                    }
                }
                let values_opt = match &pages_b[idx].kind {
                    PageKind::Custom(cp) => Some(cp.collect_values()),
                    PageKind::Welcome(wp) => Some(wp.collect_values()),
                    PageKind::Finish(fp) => Some(fp.collect_values()),
                    _ => None,
                };
                if let Some(values) = values_opt {
                    let mut inst = installer_c.lock().unwrap();
                    for (key, v) in values {
                        inst.set_option_value(&key, v);
                    }
                }
            }

            // on_before_leave.
            if let Some(ref cb) = pages_c.borrow()[idx].on_before_leave {
                let mut inst = installer_c.lock().unwrap();
                match cb(&mut inst) {
                    Ok(true) => {}
                    Ok(false) => return,
                    Err(e) => {
                        eprintln!("on_before_leave error: {e}");
                        return;
                    }
                }
            }

            // Finish or error page closes the window.
            if matches!(
                &pages_c.borrow()[idx].kind,
                PageKind::Finish(_) | PageKind::Error(_)
            ) {
                window_c.close();
                return;
            }

            let next_idx = {
                let inst = installer_c.lock().unwrap();
                next_visible_page(&pages_c.borrow(), idx, &inst)
            };
            if let Some(new_idx) = next_idx {
                let next_is_install =
                    matches!(&pages_c.borrow()[new_idx].kind, PageKind::Install(_));

                stack_c.set_visible_child(&pages_c.borrow()[new_idx].widget);
                *current_c.borrow_mut() = new_idx;
                update();
                set_page_default_focus(&pages_c.borrow()[new_idx], &btn_next_focus);

                {
                    let pages_b = pages_c.borrow();
                    if let Some(ref cb) = pages_b[new_idx].on_enter {
                        let mut inst = installer_c.lock().unwrap();
                        if let Err(e) = cb(&mut inst) {
                            eprintln!("on_enter error: {e}");
                        }
                    }
                }

                if next_is_install {
                    start_install_c();
                }
            }
        });
    }

    // Cancel button.
    {
        let cancelled_c = cancelled.clone();
        let window_c = window.clone();
        let install_running_c = install_running.clone();
        btn_cancel.connect_clicked(move |_| {
            cancelled_c.store(true, Ordering::Relaxed);
            // If the install is still running, leave the window open so the
            // Finished handler can route to the error page once the bg
            // thread bails out. Otherwise close immediately.
            if !install_running_c.load(Ordering::Relaxed) {
                window_c.close();
            }
        });
    }

    // Window close → quit main loop.
    window.connect_delete_event(|_, _| {
        gtk::main_quit();
        glib::Propagation::Proceed
    });

    // Timer: drain background → GUI messages.
    let timer_src = {
        let pages_c = pages.clone();
        let current_c = current_page.clone();
        let install_running_c = install_running.clone();
        let install_result_c = install_result.clone();
        let update = update_buttons.clone();
        let stack_c = stack.clone();
        let installer_timer = installer.clone();
        let btn_next_focus = btn_next.clone();

        glib::timeout_add_local(Duration::from_millis(50), move || {
            loop {
                match rx.try_recv() {
                    Ok(GuiMessage::SetStatus(s)) => {
                        let pages_b = pages_c.borrow();
                        let idx = *current_c.borrow();
                        if let PageKind::Install(ref ip) = pages_b[idx].kind {
                            ip.set_status(&s);
                        }
                    }
                    Ok(GuiMessage::SetProgress(p)) => {
                        let pages_b = pages_c.borrow();
                        let idx = *current_c.borrow();
                        if let PageKind::Install(ref ip) = pages_b[idx].kind {
                            ip.set_progress(p);
                        }
                    }
                    Ok(GuiMessage::Log(msg)) => {
                        let pages_b = pages_c.borrow();
                        let idx = *current_c.borrow();
                        if let PageKind::Install(ref ip) = pages_b[idx].kind {
                            ip.append_log(&msg);
                        }
                    }
                    Ok(GuiMessage::Finished(result)) => {
                        install_running_c.store(false, Ordering::Relaxed);
                        let is_ok = result.is_ok();

                        if is_ok {
                            *install_result_c.borrow_mut() = Some(result);
                            let idx = *current_c.borrow();
                            let next_idx = {
                                let inst = installer_timer.lock().unwrap();
                                next_visible_page(&pages_c.borrow(), idx, &inst)
                            };
                            if let Some(new_idx) = next_idx {
                                stack_c.set_visible_child(&pages_c.borrow()[new_idx].widget);
                                *current_c.borrow_mut() = new_idx;

                                let pages_b = pages_c.borrow();
                                if let Some(ref cb) = pages_b[new_idx].on_enter {
                                    let mut inst = installer_timer.lock().unwrap();
                                    if let Err(e) = cb(&mut inst) {
                                        eprintln!("on_enter error: {e}");
                                    }
                                }
                            }
                        } else {
                            let err_msg = match &result {
                                Err(e) => format!("{e:#}"),
                                Ok(_) => String::new(),
                            };
                            {
                                let pages_b = pages_c.borrow();
                                let idx = *current_c.borrow();
                                if let PageKind::Install(ref ip) = pages_b[idx].kind {
                                    ip.append_log(&format!("Error: {err_msg}"));
                                }
                            }
                            if let Err(ref e) = result {
                                installer_timer.lock().unwrap().log_error(e);
                            }
                            *install_result_c.borrow_mut() = Some(result);

                            // Navigate to the error page if one was registered.
                            let error_idx = pages_c
                                .borrow()
                                .iter()
                                .position(|p| matches!(&p.kind, PageKind::Error(_)));
                            if let Some(new_idx) = error_idx {
                                {
                                    let pages_b = pages_c.borrow();
                                    if let PageKind::Error(ref ep) = pages_b[new_idx].kind {
                                        ep.set_error_text(&err_msg);
                                    }
                                    stack_c.set_visible_child(&pages_b[new_idx].widget);
                                }
                                *current_c.borrow_mut() = new_idx;

                                let pages_b = pages_c.borrow();
                                if let Some(ref cb) = pages_b[new_idx].on_enter {
                                    let mut inst = installer_timer.lock().unwrap();
                                    if let Err(e) = cb(&mut inst) {
                                        eprintln!("on_enter error: {e}");
                                    }
                                }
                            } else {
                                let _ = crate::gui::error("Installation failed", &err_msg);
                            }
                        }

                        update();

                        // Refocus *after* update() flips btn_next sensitivity
                        // for the new page; grabbing focus on an insensitive
                        // button silently fails.
                        let cur = *current_c.borrow();
                        set_page_default_focus(&pages_c.borrow()[cur], &btn_next_focus);
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => break,
                }
            }
            glib::ControlFlow::Continue
        })
    };

    // Initial state + on_enter for the first page + auto-start if install.
    update_buttons();
    {
        let idx = *current_page.borrow();
        set_page_default_focus(&pages.borrow()[idx], &btn_next);
    }
    {
        let idx = *current_page.borrow();
        let is_install = {
            let pages_b = pages.borrow();
            if let Some(ref cb) = pages_b[idx].on_enter {
                let mut inst = installer.lock().unwrap();
                if let Err(e) = cb(&mut inst) {
                    eprintln!("on_enter error: {e}");
                }
            }
            matches!(&pages_b[idx].kind, PageKind::Install(_))
        };
        if is_install {
            start_install();
        }
    }

    window.show_all();
    gtk::main();

    // Tear down GTK-owned state so all captured Arc<Mutex<Installer>> refs
    // in widget event handlers and the timeout source can drop. Without
    // this, Arc::try_unwrap in run_wizard fails.
    timer_src.remove();
    unsafe {
        window.destroy();
    }
    while gtk::events_pending() {
        gtk::main_iteration();
    }

    // Join the install bg thread if still running — it holds a clone of
    // the installer Arc. Cancellation flag was already set by the Cancel
    // button, so the next op inside the thread errors out quickly.
    if let Some(handle) = install_handle.borrow_mut().take() {
        let _ = handle.join();
    }

    let result = install_result.borrow_mut().take();
    if let Some(Err(e)) = result {
        return Err(e);
    }

    Ok(())
}

fn can_advance(page: &Page) -> bool {
    match &page.kind {
        PageKind::License(lp) => lp.is_accepted(),
        _ => true,
    }
}
