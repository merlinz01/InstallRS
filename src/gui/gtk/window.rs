use anyhow::Result;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use gtk::prelude::*;

use super::pages::{
    ComponentsPage, DirectoryPickerPage, ErrorPage, FinishPage, InstallPage, LicensePage, PageKind,
    WelcomePage,
};
use crate::gui::types::{
    ConfiguredPage, GuiContext, GuiMessage, InstallCallback, OnBeforeLeaveCallback,
    OnEnterCallback, PageContext, WizardConfig, WizardPage,
};
use crate::Installer;

const WINDOW_WIDTH: i32 = 560;
const WINDOW_HEIGHT: i32 = 400;

struct Page {
    widget: gtk::Box,
    kind: PageKind,
    on_enter: Option<OnEnterCallback>,
    on_before_leave: Option<OnBeforeLeaveCallback>,
}

pub fn run(
    config: WizardConfig,
    installer: Arc<Mutex<Installer>>,
    install_dir: Arc<Mutex<String>>,
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
        } = configured;

        let (widget, kind) = match page_cfg {
            WizardPage::Welcome { title, message } => {
                let p = WelcomePage::new(&title, &message);
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
                default,
            } => {
                let p = DirectoryPickerPage::new(&heading, &label, &default);
                let w = p.widget().clone();
                (w, PageKind::DirectoryPicker(p))
            }
            WizardPage::Install { .. } => {
                let p = InstallPage::new();
                let w = p.widget().clone();
                (w, PageKind::Install(p))
            }
            WizardPage::Finish { title, message } => {
                let p = FinishPage::new(&title, &message);
                let w = p.widget().clone();
                (w, PageKind::Finish(p))
            }
            WizardPage::Error { title, message } => {
                let p = ErrorPage::new(&title, &message);
                let w = p.widget().clone();
                (w, PageKind::Error(p))
            }
        };

        stack.add_named(&widget, &format!("page-{idx}"));
        pages.push(Page {
            widget,
            kind,
            on_enter,
            on_before_leave,
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
    let page_count = pages.len();
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
        let label_finish = config.buttons.finish.clone();

        Rc::new(move || {
            let idx = *current.borrow();
            let pages_b = pages.borrow();
            let is_first = idx == 0;
            let is_install = matches!(&pages_b[idx].kind, PageKind::Install(_));
            let is_finish = matches!(&pages_b[idx].kind, PageKind::Finish(_));
            let is_error = matches!(&pages_b[idx].kind, PageKind::Error(_));
            let is_terminal = is_finish || is_error;
            let next_is_install =
                idx + 1 < pages_b.len() && matches!(&pages_b[idx + 1].kind, PageKind::Install(_));
            let running = install_running_c.load(Ordering::Relaxed);

            btn_back_c.set_sensitive(!is_first && !is_install && !is_terminal);
            let label = if is_terminal {
                &label_finish
            } else if next_is_install || is_install {
                &label_install
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

    // Factory for fresh PageContext instances.
    let make_ctx: Rc<dyn Fn() -> PageContext> = {
        let installer_c = installer.clone();
        let install_dir_c = install_dir.clone();
        let cancelled_c = cancelled.clone();
        Rc::new(move || {
            PageContext::new(
                installer_c.clone(),
                install_dir_c.clone(),
                cancelled_c.clone(),
            )
        })
    };

    // start_install: consume the install callback and spawn the bg thread.
    let start_install: Rc<dyn Fn()> = {
        let installer_c = installer.clone();
        let install_dir_c = install_dir.clone();
        let cancelled_c = cancelled.clone();
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
                let install_dir_bg = install_dir_c.clone();
                let cancelled_bg = cancelled_c.clone();
                let tx_bg = tx_c.clone();

                let handle = std::thread::spawn(move || {
                    let mut ctx =
                        GuiContext::new(tx_bg.clone(), installer_bg, install_dir_bg, cancelled_bg);
                    {
                        let mut inst = ctx.installer();
                        inst.set_progress_sink(ctx.progress_sink());
                        inst.reset_progress();
                    }
                    let result = callback(&mut ctx);
                    ctx.installer().clear_progress_sink();
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
        let make_ctx_c = make_ctx.clone();
        let stack_c = stack.clone();

        btn_back.connect_clicked(move |_| {
            let idx = *current_c.borrow();
            if idx == 0 {
                return;
            }

            if let Some(ref cb) = pages_c.borrow()[idx].on_before_leave {
                let mut ctx = make_ctx_c();
                match cb(&mut ctx) {
                    Ok(true) => {}
                    Ok(false) => return,
                    Err(e) => {
                        eprintln!("on_before_leave error: {e}");
                        return;
                    }
                }
            }

            let new_idx = idx - 1;
            stack_c.set_visible_child(&pages_c.borrow()[new_idx].widget);
            *current_c.borrow_mut() = new_idx;
            update();

            let pages_b = pages_c.borrow();
            if let Some(ref cb) = pages_b[new_idx].on_enter {
                let mut ctx = make_ctx_c();
                if let Err(e) = cb(&mut ctx) {
                    eprintln!("on_enter error: {e}");
                }
            }
        });
    }

    // Next button.
    {
        let pages_c = pages.clone();
        let current_c = current_page.clone();
        let update = update_buttons.clone();
        let install_dir_c = install_dir.clone();
        let start_install_c = start_install.clone();
        let window_c = window.clone();
        let stack_c = stack.clone();
        let make_ctx_c = make_ctx.clone();
        let installer_c = installer.clone();

        btn_next.connect_clicked(move |_| {
            let idx = *current_c.borrow();

            // Sync directory picker / components into installer state before callbacks.
            {
                let pages_b = pages_c.borrow();
                if let PageKind::DirectoryPicker(ref dp) = pages_b[idx].kind {
                    *install_dir_c.lock().unwrap() = dp.get_directory();
                }
                if let PageKind::Components(ref cp) = pages_b[idx].kind {
                    let sels = cp.selections();
                    let mut inst = installer_c.lock().unwrap();
                    for (id, on) in sels {
                        inst.set_component_selected(&id, on);
                    }
                }
            }

            // on_before_leave.
            if let Some(ref cb) = pages_c.borrow()[idx].on_before_leave {
                let mut ctx = make_ctx_c();
                match cb(&mut ctx) {
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

            if idx + 1 < page_count {
                let new_idx = idx + 1;
                let next_is_install =
                    matches!(&pages_c.borrow()[new_idx].kind, PageKind::Install(_));

                stack_c.set_visible_child(&pages_c.borrow()[new_idx].widget);
                *current_c.borrow_mut() = new_idx;
                update();

                {
                    let pages_b = pages_c.borrow();
                    if let Some(ref cb) = pages_b[new_idx].on_enter {
                        let mut ctx = make_ctx_c();
                        if let Err(e) = cb(&mut ctx) {
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
        let make_ctx_c = make_ctx.clone();
        let stack_c = stack.clone();

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
                            if idx + 1 < page_count {
                                let new_idx = idx + 1;
                                stack_c.set_visible_child(&pages_c.borrow()[new_idx].widget);
                                *current_c.borrow_mut() = new_idx;

                                let pages_b = pages_c.borrow();
                                if let Some(ref cb) = pages_b[new_idx].on_enter {
                                    let mut ctx = make_ctx_c();
                                    if let Err(e) = cb(&mut ctx) {
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
                                    let mut ctx = make_ctx_c();
                                    if let Err(e) = cb(&mut ctx) {
                                        eprintln!("on_enter error: {e}");
                                    }
                                }
                            } else {
                                let _ = crate::gui::error("Installation failed", &err_msg);
                            }
                        }

                        update();
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
    btn_next.grab_focus();
    {
        let idx = *current_page.borrow();
        let is_install = {
            let pages_b = pages.borrow();
            if let Some(ref cb) = pages_b[idx].on_enter {
                let mut ctx = make_ctx();
                if let Err(e) = cb(&mut ctx) {
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
