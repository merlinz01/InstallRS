use anyhow::Result;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc, Mutex};

use winsafe::co;
use winsafe::gui;
use winsafe::prelude::*;

use super::pages::{
    ComponentsPage, CustomPage, DirectoryPickerPage, ErrorPage, FinishPage, InstallPage,
    LicensePage, PageKind, WelcomePage,
};
use crate::gui::types::{
    ConfiguredPage, GuiContext, GuiMessage, InstallCallback, OnBeforeLeaveCallback,
    OnEnterCallback, PageContext, WizardConfig, WizardPage,
};
use crate::Installer;

const WINDOW_WIDTH: i32 = 500;
const WINDOW_HEIGHT: i32 = 360;
const BUTTON_WIDTH: i32 = 80;
const BUTTON_HEIGHT: i32 = 26;
const MARGIN: i32 = 10;

/// Find the next visible page strictly after `from`. Evaluates each
/// candidate's `skip_if` predicate; returns None if every remaining page
/// is hidden or `from` is already at the end.
fn next_visible_page(pages: &[Page], from: usize, ctx: &PageContext) -> Option<usize> {
    let mut i = from.checked_add(1)?;
    while i < pages.len() {
        if pages[i].skip_if.as_ref().is_some_and(|p| p(ctx)) {
            i = i.checked_add(1)?;
        } else {
            return Some(i);
        }
    }
    None
}

/// Find the previous visible page strictly before `from`. Returns None
/// when every earlier page is hidden or `from` is already at the start.
fn prev_visible_page(pages: &[Page], from: usize, ctx: &PageContext) -> Option<usize> {
    let mut i = from.checked_sub(1)?;
    loop {
        if pages[i].skip_if.as_ref().is_some_and(|p| p(ctx)) {
            i = i.checked_sub(1)?;
        } else {
            return Some(i);
        }
    }
}

/// Page wrapper that holds the panel, its kind, and navigation callbacks.
struct Page {
    panel: gui::WindowControl,
    kind: PageKind,
    on_enter: Option<OnEnterCallback>,
    on_before_leave: Option<OnBeforeLeaveCallback>,
    skip_if: Option<crate::gui::types::SkipIfCallback>,
    /// Only meaningful for `PageKind::Install` — when true, the Next button
    /// leading into / shown on this page uses `buttons.uninstall`.
    is_uninstall: bool,
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
    // Try to load the application icon from embedded resources (resource ID 1,
    // set by winresource).  Fall back to no icon if the resource doesn't exist.
    let class_icon = {
        use winsafe::HINSTANCE;
        let hinst = HINSTANCE::GetModuleHandle(None).unwrap_or(HINSTANCE::NULL);
        match hinst.LoadIcon(winsafe::IdIdiStr::Id(1)) {
            Ok(mut hicon) => gui::Icon::Handle(hicon.leak()),
            Err(_) => gui::Icon::None,
        }
    };

    let wnd = gui::WindowMain::new(gui::WindowMainOpts {
        title: &config.title,
        size: gui::dpi(WINDOW_WIDTH, WINDOW_HEIGHT),
        class_icon,
        style: co::WS::CAPTION
            | co::WS::SYSMENU
            | co::WS::CLIPCHILDREN
            | co::WS::VISIBLE
            | co::WS::MINIMIZEBOX
            | co::WS::MAXIMIZEBOX
            | co::WS::THICKFRAME,
        ..Default::default()
    });

    // Content area dimensions (above the button bar). The panel itself is flush
    // with the top and sides of the window; pages add their own internal padding.
    let content_width = WINDOW_WIDTH;
    let button_bar_height = BUTTON_HEIGHT + 2 * MARGIN;
    let content_height = WINDOW_HEIGHT - button_bar_height;

    // Create page panels.
    let mut pages: Vec<Page> = Vec::new();
    for (idx, configured) in config.pages.into_iter().enumerate() {
        let ConfiguredPage {
            page: page_cfg,
            on_enter,
            on_before_leave,
            skip_if,
        } = configured;
        let visible = idx == 0;
        let panel = gui::WindowControl::new(
            &wnd,
            gui::WindowControlOpts {
                position: gui::dpi(0, 0),
                size: gui::dpi(content_width, content_height),
                style: co::WS::CHILD
                    | co::WS::CLIPCHILDREN
                    | co::WS::CLIPSIBLINGS
                    | if visible {
                        co::WS::VISIBLE
                    } else {
                        co::WS::NoValue
                    },
                ex_style: co::WS_EX::NoValue,
                resize_behavior: (gui::Horz::Resize, gui::Vert::Resize),
                ..Default::default()
            },
        );

        let page_is_uninstall = matches!(
            &page_cfg,
            WizardPage::Install {
                is_uninstall: true,
                ..
            }
        );

        let kind = match page_cfg {
            WizardPage::Welcome { title, message } => PageKind::Welcome(WelcomePage::new(
                &panel,
                &title,
                &message,
                content_width,
                content_height,
            )),
            WizardPage::License {
                heading,
                text,
                accept_label,
            } => PageKind::License(LicensePage::new(
                &panel,
                &heading,
                &text,
                &accept_label,
                content_width,
                content_height,
            )),
            WizardPage::Components { heading, label } => {
                let comps = installer.lock().unwrap().components().to_vec();
                PageKind::Components(ComponentsPage::new(
                    &panel,
                    &heading,
                    &label,
                    &comps,
                    content_width,
                    content_height,
                ))
            }
            WizardPage::DirectoryPicker {
                heading,
                label,
                default,
            } => PageKind::DirectoryPicker(DirectoryPickerPage::new(
                &panel,
                &heading,
                &label,
                &default,
                content_width,
                content_height,
            )),
            WizardPage::Install { .. } => {
                PageKind::Install(InstallPage::new(&panel, content_width, content_height))
            }
            WizardPage::Finish { title, message } => PageKind::Finish(FinishPage::new(
                &panel,
                &title,
                &message,
                content_width,
                content_height,
            )),
            WizardPage::Error { title, message } => PageKind::Error(ErrorPage::new(
                &panel,
                &title,
                &message,
                content_width,
                content_height,
            )),
            WizardPage::Custom {
                heading,
                label,
                widgets,
            } => {
                let initial = installer.lock().unwrap().option_values_snapshot();
                PageKind::Custom(CustomPage::new(
                    &panel,
                    &heading,
                    &label,
                    &widgets,
                    &initial,
                    content_width,
                    content_height,
                ))
            }
        };

        pages.push(Page {
            panel,
            kind,
            on_enter,
            on_before_leave,
            skip_if,
            is_uninstall: page_is_uninstall,
        });
    }

    // Navigation buttons.
    let btn_y = WINDOW_HEIGHT - button_bar_height + MARGIN / 2;
    let (bw, bh) = gui::dpi(BUTTON_WIDTH, BUTTON_HEIGHT);

    let btn_back = gui::Button::new(
        &wnd,
        gui::ButtonOpts {
            text: &config.buttons.back,
            position: gui::dpi(WINDOW_WIDTH - 3 * (BUTTON_WIDTH + MARGIN), btn_y),
            width: bw,
            height: bh,
            resize_behavior: (gui::Horz::Repos, gui::Vert::Repos),
            ..Default::default()
        },
    );

    let btn_next = gui::Button::new(
        &wnd,
        gui::ButtonOpts {
            text: &config.buttons.next,
            position: gui::dpi(WINDOW_WIDTH - 2 * (BUTTON_WIDTH + MARGIN), btn_y),
            width: bw,
            height: bh,
            resize_behavior: (gui::Horz::Repos, gui::Vert::Repos),
            control_style: co::BS::DEFPUSHBUTTON,
            ..Default::default()
        },
    );

    let btn_cancel = gui::Button::new(
        &wnd,
        gui::ButtonOpts {
            text: &config.buttons.cancel,
            position: gui::dpi(WINDOW_WIDTH - (BUTTON_WIDTH + MARGIN), btn_y),
            width: bw,
            height: bh,
            resize_behavior: (gui::Horz::Repos, gui::Vert::Repos),
            ..Default::default()
        },
    );

    let current_page = Arc::new(Mutex::new(0usize));
    let pages = Rc::new(Mutex::new(pages));
    let install_callback = Arc::new(Mutex::new(install_callback));
    let install_running = Arc::new(AtomicBool::new(false));
    let install_result: Arc<Mutex<Option<Result<()>>>> = Arc::new(Mutex::new(None));
    let install_handle: Arc<Mutex<Option<std::thread::JoinHandle<()>>>> =
        Arc::new(Mutex::new(None));

    // Helper: build a fresh PageContext for callbacks. Defined early so
    // `update_buttons` can use it when evaluating `skip_if` predicates.
    let make_page_ctx = {
        let installer_c = installer.clone();
        let install_dir_c = install_dir.clone();
        let cancelled_c = cancelled.clone();
        Arc::new(move || {
            PageContext::new(
                installer_c.clone(),
                install_dir_c.clone(),
                cancelled_c.clone(),
            )
        })
    };

    // Helper: update button states for the current page.
    {
        let pages_c = pages.clone();
        let current_c = current_page.clone();
        let btn_back_c = btn_back.clone();
        let btn_next_c = btn_next.clone();
        let btn_cancel_c = btn_cancel.clone();
        let install_running_c = install_running.clone();
        let label_next = config.buttons.next.clone();
        let label_install = config.buttons.install.clone();
        let label_uninstall = config.buttons.uninstall.clone();
        let label_finish = config.buttons.finish.clone();
        let make_ctx_btn = make_page_ctx.clone();

        let update_buttons = move || {
            let idx = *current_c.lock().unwrap();
            let pages_guard = pages_c.lock().unwrap();
            let is_first = idx == 0;
            let is_install = matches!(&pages_guard[idx].kind, PageKind::Install(_));
            let is_finish = matches!(&pages_guard[idx].kind, PageKind::Finish(_));
            let is_error = matches!(&pages_guard[idx].kind, PageKind::Error(_));
            let is_terminal = is_finish || is_error;
            let ctx = make_ctx_btn();
            let next_idx = next_visible_page(&pages_guard, idx, &ctx);
            let next_is_install = next_idx
                .map(|i| matches!(&pages_guard[i].kind, PageKind::Install(_)))
                .unwrap_or(false);
            // Pull the uninstall flag off whichever install page the button
            // currently refers to (the current one, or the one we're about
            // to advance into).
            let install_is_uninstall = if is_install {
                pages_guard[idx].is_uninstall
            } else if let Some(i) = next_idx.filter(|_| next_is_install) {
                pages_guard[i].is_uninstall
            } else {
                false
            };
            let running = install_running_c.load(std::sync::atomic::Ordering::Relaxed);

            btn_back_c
                .hwnd()
                .EnableWindow(!is_first && !is_install && !is_terminal);
            let install_label = if install_is_uninstall {
                &label_uninstall
            } else {
                &label_install
            };
            let _ = if is_terminal {
                btn_next_c.hwnd().SetWindowText(&label_finish)
            } else if next_is_install || is_install {
                btn_next_c.hwnd().SetWindowText(install_label)
            } else {
                btn_next_c.hwnd().SetWindowText(&label_next)
            };
            btn_next_c
                .hwnd()
                .EnableWindow(!running && !is_install && can_advance(&pages_guard[idx]));
            btn_cancel_c.hwnd().EnableWindow(!is_terminal);
        };

        // Store the closure in an Arc for reuse.
        let update_buttons = Rc::new(update_buttons);

        // Helper to start the install in a background thread. Idempotent — the
        // callback is consumed on first call, so subsequent calls are a no-op.
        let start_install: Rc<dyn Fn() + 'static> = {
            let installer_c = installer.clone();
            let install_dir_c = install_dir.clone();
            let cancelled_c = cancelled.clone();
            let tx_c = tx.clone();
            let install_cb = install_callback.clone();
            let install_running_c = install_running.clone();
            let install_handle_c = install_handle.clone();
            let update = update_buttons.clone();

            Rc::new(move || {
                let cb = install_cb.lock().unwrap().take();
                if let Some(callback) = cb {
                    install_running_c.store(true, std::sync::atomic::Ordering::Relaxed);

                    let installer_bg = installer_c.clone();
                    let install_dir_bg = install_dir_c.clone();
                    let cancelled_bg = cancelled_c.clone();
                    let tx_bg = tx_c.clone();

                    let handle = std::thread::spawn(move || {
                        let mut ctx = GuiContext::new(
                            tx_bg.clone(),
                            installer_bg,
                            install_dir_bg,
                            cancelled_bg,
                        );
                        // Auto-attach a progress sink so Installer ops emit
                        // status/progress/log events to the GUI.
                        {
                            let mut inst = ctx.installer();
                            inst.set_progress_sink(ctx.progress_sink());
                            inst.reset_progress();
                        }
                        let result = callback(&mut ctx);
                        // Detach the sink: once the install is done, further
                        // ops (if any) shouldn't push to a dead GUI channel.
                        ctx.installer().clear_progress_sink();
                        let _ = tx_bg.send(GuiMessage::Finished(result));
                    });
                    *install_handle_c.lock().unwrap() = Some(handle);

                    update();
                }
            })
        };

        // Wire up license checkbox state changes to refresh button enablement.
        {
            let pages_guard = pages.lock().unwrap();
            for page in pages_guard.iter() {
                if let PageKind::License(ref lp) = page.kind {
                    let update = update_buttons.clone();
                    lp.on_accept_changed(move || update());
                }
            }
        }

        // Wire up button clicks.
        {
            let pages_c = pages.clone();
            let current_c = current_page.clone();
            let update = update_buttons.clone();
            let make_ctx = make_page_ctx.clone();
            btn_back.on().bn_clicked(move || {
                let idx = *current_c.lock().unwrap();
                if idx == 0 {
                    return Ok(());
                }

                // on_before_leave / on_enter are intentionally skipped on
                // backward navigation — they fire only on forward moves.
                let pages_guard = pages_c.lock().unwrap();
                let ctx = make_ctx();
                let Some(new_idx) = prev_visible_page(&pages_guard, idx, &ctx) else {
                    return Ok(());
                };
                pages_guard[idx].panel.hwnd().ShowWindow(co::SW::HIDE);
                pages_guard[new_idx].panel.hwnd().ShowWindow(co::SW::SHOW);
                drop(pages_guard);
                *current_c.lock().unwrap() = new_idx;
                update();
                Ok(())
            });
        }

        {
            let pages_c = pages.clone();
            let current_c = current_page.clone();
            let update = update_buttons.clone();
            let install_dir_c = install_dir.clone();
            let start_install_c = start_install.clone();
            let wnd_c = wnd.clone();
            let make_ctx = make_page_ctx.clone();
            let installer_c = installer.clone();

            btn_next.on().bn_clicked(move || {
                let idx = *current_c.lock().unwrap();

                // Sync directory picker value before on_before_leave so the
                // callback sees the updated install_dir.
                {
                    let pages_guard = pages_c.lock().unwrap();
                    if let PageKind::DirectoryPicker(ref dp) = pages_guard[idx].kind {
                        let dir = dp.get_directory();
                        *install_dir_c.lock().unwrap() = dir;
                    }
                    if let PageKind::Components(ref cp) = pages_guard[idx].kind {
                        let sels = cp.selections();
                        let mut inst = installer_c.lock().unwrap();
                        for (id, on) in sels {
                            inst.set_component_selected(&id, on);
                        }
                    }
                    if let PageKind::Custom(ref cp) = pages_guard[idx].kind {
                        let values = cp.collect_values();
                        let mut inst = installer_c.lock().unwrap();
                        for (key, v) in values {
                            inst.set_option_value(&key, v);
                        }
                    }
                }

                // on_before_leave — cancel navigation on Ok(false) or Err.
                {
                    let pages_guard = pages_c.lock().unwrap();
                    if let Some(ref cb) = pages_guard[idx].on_before_leave {
                        let mut ctx = make_ctx();
                        match cb(&mut ctx) {
                            Ok(true) => {}
                            Ok(false) => return Ok(()),
                            Err(e) => {
                                eprintln!("on_before_leave error: {e}");
                                return Ok(());
                            }
                        }
                    }
                }

                let pages_guard = pages_c.lock().unwrap();

                // On finish or error page, close the window.
                if matches!(
                    &pages_guard[idx].kind,
                    PageKind::Finish(_) | PageKind::Error(_)
                ) {
                    drop(pages_guard);
                    wnd_c.close();
                    return Ok(());
                }

                // Advance to next visible page.
                let ctx_tmp = make_ctx();
                if let Some(new_idx) = next_visible_page(&pages_guard, idx, &ctx_tmp) {
                    let next_is_install =
                        matches!(&pages_guard[new_idx].kind, PageKind::Install(_));
                    pages_guard[idx].panel.hwnd().ShowWindow(co::SW::HIDE);
                    pages_guard[new_idx].panel.hwnd().ShowWindow(co::SW::SHOW);
                    drop(pages_guard);
                    *current_c.lock().unwrap() = new_idx;
                    update();

                    // on_enter of the new page.
                    {
                        let pages_guard = pages_c.lock().unwrap();
                        if let Some(ref cb) = pages_guard[new_idx].on_enter {
                            let mut ctx = make_ctx();
                            if let Err(e) = cb(&mut ctx) {
                                eprintln!("on_enter error: {e}");
                            }
                        }
                    }

                    if next_is_install {
                        start_install_c();
                    }
                }

                Ok(())
            });
        }

        {
            let cancelled_c = cancelled.clone();
            let wnd_c = wnd.clone();
            let install_running_c = install_running.clone();
            btn_cancel.on().bn_clicked(move || {
                cancelled_c.store(true, std::sync::atomic::Ordering::Relaxed);
                // If the install is still running, leave the window open so
                // the Finished handler can route to the error page once the
                // bg thread bails out. Otherwise close immediately.
                if !install_running_c.load(std::sync::atomic::Ordering::Relaxed) {
                    wnd_c.close();
                }
                Ok(())
            });
        }

        // Enforce minimum window size (the initial dimensions + non-client area).
        {
            let (client_w, client_h) = gui::dpi(WINDOW_WIDTH, WINDOW_HEIGHT);
            let style = co::WS::CAPTION
                | co::WS::SYSMENU
                | co::WS::CLIPCHILDREN
                | co::WS::VISIBLE
                | co::WS::MINIMIZEBOX
                | co::WS::MAXIMIZEBOX
                | co::WS::THICKFRAME;
            let rc = winsafe::AdjustWindowRectEx(
                winsafe::RECT {
                    left: 0,
                    top: 0,
                    right: client_w,
                    bottom: client_h,
                },
                style,
                false,
                co::WS_EX::NoValue,
            )
            .unwrap_or(winsafe::RECT {
                left: 0,
                top: 0,
                right: client_w,
                bottom: client_h,
            });
            let min_w = rc.right - rc.left;
            let min_h = rc.bottom - rc.top;

            wnd.on().wm_get_min_max_info(move |p| {
                p.info.ptMinTrackSize.x = min_w;
                p.info.ptMinTrackSize.y = min_h;
                Ok(())
            });
        }

        // Timer to poll the message channel from the background thread.
        const TIMER_ID: usize = 1;
        {
            let wnd_c = wnd.clone();
            wnd.on().wm_create(move |_| {
                wnd_c.hwnd().SetTimer(TIMER_ID, 50, None)?;
                Ok(0)
            });
        }

        {
            let pages_timer = pages.clone();
            let current_timer = current_page.clone();
            let install_running_timer = install_running.clone();
            let install_result_timer = install_result.clone();
            let update_timer = update_buttons.clone();
            let make_ctx = make_page_ctx.clone();
            let installer_log_err = installer.clone();

            wnd.on().wm_timer(TIMER_ID, move || {
                // Drain all pending messages.
                loop {
                    match rx.try_recv() {
                        Ok(GuiMessage::SetStatus(status)) => {
                            let pages_guard = pages_timer.lock().unwrap();
                            let idx = *current_timer.lock().unwrap();
                            if let PageKind::Install(ref ip) = pages_guard[idx].kind {
                                ip.set_status(&status);
                            }
                        }
                        Ok(GuiMessage::SetProgress(progress)) => {
                            let pages_guard = pages_timer.lock().unwrap();
                            let idx = *current_timer.lock().unwrap();
                            if let PageKind::Install(ref ip) = pages_guard[idx].kind {
                                ip.set_progress(progress);
                            }
                        }
                        Ok(GuiMessage::Log(msg)) => {
                            let pages_guard = pages_timer.lock().unwrap();
                            let idx = *current_timer.lock().unwrap();
                            if let PageKind::Install(ref ip) = pages_guard[idx].kind {
                                ip.append_log(&msg);
                            }
                        }
                        Ok(GuiMessage::Finished(result)) => {
                            install_running_timer
                                .store(false, std::sync::atomic::Ordering::Relaxed);
                            let is_ok = result.is_ok();

                            if is_ok {
                                *install_result_timer.lock().unwrap() = Some(result);
                                // Advance to the next visible page (finish page).
                                let pages_guard = pages_timer.lock().unwrap();
                                let idx = *current_timer.lock().unwrap();
                                let ctx_tmp = make_ctx();
                                if let Some(new_idx) =
                                    next_visible_page(&pages_guard, idx, &ctx_tmp)
                                {
                                    pages_guard[idx].panel.hwnd().ShowWindow(co::SW::HIDE);
                                    pages_guard[new_idx].panel.hwnd().ShowWindow(co::SW::SHOW);
                                    drop(pages_guard);
                                    *current_timer.lock().unwrap() = new_idx;

                                    // on_enter of the new page.
                                    let pages_guard = pages_timer.lock().unwrap();
                                    if let Some(ref cb) = pages_guard[new_idx].on_enter {
                                        let mut ctx = make_ctx();
                                        if let Err(e) = cb(&mut ctx) {
                                            eprintln!("on_enter error: {e}");
                                        }
                                    }
                                }
                            } else {
                                // Surface the error to the user: append it to
                                // the install page log, then navigate to the
                                // error page if one was registered. Fall back
                                // to a native error dialog otherwise.
                                let err_msg = match &result {
                                    Err(e) => format!("{e:#}"),
                                    Ok(_) => String::new(),
                                };
                                {
                                    let pages_guard = pages_timer.lock().unwrap();
                                    let idx = *current_timer.lock().unwrap();
                                    if let PageKind::Install(ref ip) = pages_guard[idx].kind {
                                        ip.append_log(&format!("Error: {err_msg}"));
                                    }
                                }
                                if let Err(ref e) = result {
                                    installer_log_err.lock().unwrap().log_error(e);
                                }
                                *install_result_timer.lock().unwrap() = Some(result);

                                // Find an error page anywhere in the wizard.
                                let error_idx = {
                                    let pages_guard = pages_timer.lock().unwrap();
                                    pages_guard
                                        .iter()
                                        .position(|p| matches!(&p.kind, PageKind::Error(_)))
                                };
                                if let Some(new_idx) = error_idx {
                                    let pages_guard = pages_timer.lock().unwrap();
                                    let idx = *current_timer.lock().unwrap();
                                    if let PageKind::Error(ref ep) = pages_guard[new_idx].kind {
                                        ep.set_error_text(&err_msg);
                                    }
                                    pages_guard[idx].panel.hwnd().ShowWindow(co::SW::HIDE);
                                    pages_guard[new_idx].panel.hwnd().ShowWindow(co::SW::SHOW);
                                    drop(pages_guard);
                                    *current_timer.lock().unwrap() = new_idx;

                                    let pages_guard = pages_timer.lock().unwrap();
                                    if let Some(ref cb) = pages_guard[new_idx].on_enter {
                                        let mut ctx = make_ctx();
                                        if let Err(e) = cb(&mut ctx) {
                                            eprintln!("on_enter error: {e}");
                                        }
                                    }
                                } else {
                                    let _ = crate::gui::error("Installation failed", &err_msg);
                                }
                            }

                            update_timer();
                        }
                        Err(mpsc::TryRecvError::Empty) => break,
                        Err(mpsc::TryRecvError::Disconnected) => break,
                    }
                }
                Ok(())
            });
        }

        // Initial button state update and focus after the window is first shown.
        {
            let update = update_buttons.clone();
            let btn_next_c = btn_next.clone();
            let pages_c = pages.clone();
            let current_c = current_page.clone();
            let start_install_c = start_install.clone();
            let make_ctx = make_page_ctx.clone();
            let focus_set = Arc::new(AtomicBool::new(false));
            wnd.on().wm_show_window(move |_| {
                update();
                if !focus_set.swap(true, std::sync::atomic::Ordering::Relaxed) {
                    let _ = btn_next_c.hwnd().SetFocus();

                    let pages_guard = pages_c.lock().unwrap();
                    let idx = *current_c.lock().unwrap();
                    let is_install = matches!(&pages_guard[idx].kind, PageKind::Install(_));

                    // on_enter of the initial page.
                    if let Some(ref cb) = pages_guard[idx].on_enter {
                        let mut ctx = make_ctx();
                        if let Err(e) = cb(&mut ctx) {
                            eprintln!("on_enter error: {e}");
                        }
                    }
                    drop(pages_guard);

                    // If the first page is the install page, auto-start.
                    if is_install {
                        start_install_c();
                    }
                }
                Ok(())
            });
        }
    }

    wnd.run_main(None).map_err(|e| anyhow::anyhow!("{e}"))?;

    // Join the install bg thread if still running — it holds a clone of the
    // installer Arc. Cancellation flag was already set by Cancel / Ctrl+C,
    // so the next op errors out quickly and the thread exits.
    if let Some(handle) = install_handle.lock().unwrap().take() {
        let _ = handle.join();
    }

    // Check if the install had an error.
    let result = install_result.lock().unwrap().take();
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
