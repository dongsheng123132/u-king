//! 右下角常驻托盘 —— 像 360 一样守在系统托盘。
//!
//! 借鉴复杂版 tray.rs，但只留简化版需要的菜单项：
//! - 打开管理界面 / 隐藏到托盘
//! - 一键装到本地（emit 给前端去走带进度的安装流）
//! - 退出
//!
//! 左键单击托盘 = 打开主窗口（360 习惯）。关闭窗口 = 缩回托盘而非退出。

use tauri::{
    image::Image,
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, Wry,
};

use crate::{device, providers};

/// 在 setup 阶段安装托盘。
pub fn install(app: &AppHandle) -> tauri::Result<TrayIcon<Wry>> {
    let menu = build_menu(app)?;

    let icon: Image<'_> = app
        .default_window_icon()
        .cloned()
        .unwrap_or_else(|| Image::new_owned(vec![0; 4], 1, 1));

    let tray = TrayIconBuilder::with_id("uking-tray")
        .icon(icon)
        .tooltip("U-King · 个人 AI 操作系统")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(handle_menu_event)
        .on_tray_icon_event(handle_tray_event)
        .build(app)?;

    Ok(tray)
}

/// 托盘可一键切换的驱动清单：虾盘云（内置指纹 Key，永远可切）+ 官方还原 + 存了 Key 的
/// 自定义中转站。内置 DeepSeek/GLM/Kimi 预设不存 Key，托盘切不了 → 引导去「AI 设置」。
fn switchable_providers() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vec![];
    for p in providers::list_providers() {
        if p.id == "xiapan" {
            out.push((p.id, format!("{}（内置 Key）", p.name)));
        } else if !p.builtin && !p.api_key.is_empty() {
            out.push((p.id.clone(), p.name.clone()));
        }
    }
    out.push(("official".into(), "官方还原（用自己的登录）".into()));
    out
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let show = MenuItem::with_id(app, "show", "打开管理界面", true, None::<&str>)?;
    let hide = MenuItem::with_id(app, "hide", "隐藏到托盘", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;

    // —— AI 驱动快捷切换（cc-switch 式：托盘两下点完，不用开大窗口）——
    // 勾选态只读 active-drivers.json 显式记录（driver_status().active），以 claude 为代表。
    let active = providers::driver_status()
        .active
        .get("claude")
        .cloned()
        .unwrap_or_default();
    let drv_title = MenuItem::with_id(app, "drv-title", "AI 驱动（Claude/Codex/Hermes）", false, None::<&str>)?;
    let mut drv_items: Vec<CheckMenuItem<Wry>> = vec![];
    for (id, label) in switchable_providers() {
        drv_items.push(CheckMenuItem::with_id(
            app,
            format!("drv:{id}"),
            &label,
            true,
            active == id,
            None::<&str>,
        )?);
    }
    let manage = MenuItem::with_id(app, "manage", "换驱动 / 用自己的 Key…", true, None::<&str>)?;
    let sep_drv = PredefinedMenuItem::separator(app)?;

    let install = MenuItem::with_id(app, "install", "一键装到本地", true, None::<&str>)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let about = MenuItem::with_id(app, "about", "关于 U-King", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;

    let mut items: Vec<&dyn tauri::menu::IsMenuItem<Wry>> = vec![&show, &hide, &sep1, &drv_title];
    for it in &drv_items {
        items.push(it);
    }
    items.push(&manage);
    items.push(&sep_drv);
    items.push(&install);
    items.push(&sep2);
    items.push(&about);
    items.push(&quit);
    Menu::with_items(app, &items)
}

/// 托盘点了某个驱动：后台线程真切（凡碰网络/文件的不卡菜单线程），完事重建菜单刷新勾选 + 通知前端。
/// 只切 CLI 三件套（claude/codex/hermes，写完即生效）；ClawX 要关进程重启才生效，托盘不做
/// 惊吓动作 —— 有它时前端 toast 里提示去「AI 设置」里切（那边有托管式重启流程）。
fn switch_driver(app: &AppHandle, provider_id: String) {
    let app = app.clone();
    std::thread::spawn(move || {
        let key = if provider_id == "xiapan" {
            device::device_key_offline().unwrap_or_default()
        } else if provider_id == "official" {
            "-".into()
        } else {
            providers::list_providers()
                .into_iter()
                .find(|p| p.id == provider_id)
                .map(|p| p.api_key)
                .unwrap_or_default()
        };
        let targets: Vec<String> = ["claude", "codex", "hermes"].iter().map(|s| s.to_string()).collect();
        let msg = match providers::apply_provider(&provider_id, &key, None, &targets) {
            Ok(r) => {
                let name = providers::list_providers()
                    .into_iter()
                    .find(|p| p.id == provider_id)
                    .map(|p| p.name)
                    .unwrap_or_else(|| provider_id.clone());
                // Codex 默认省钱路由：托盘切虾盘云也要接管（否则托盘一切就退回贵的直连）
                let route_hint = if r.codex.is_some()
                    && crate::ensure_codex_cheap_route(&provider_id, None).is_some()
                {
                    " · Codex 走省钱路由"
                } else {
                    ""
                };
                let clawx_hint = if r.clawx.is_none() && providers::driver_status().clawx_installed {
                    // AI 设置页已有 ClawX Tab（apply_clawx_managed 自动关/写/重启），指路要指到点上
                    "（ClawX 在「AI 设置」→ ClawX 页一键切，自动重启生效）"
                } else {
                    ""
                };
                format!("已切到 {name} · Claude/Codex/Hermes 生效{route_hint}{clawx_hint}")
            }
            Err(e) => format!("切换失败：{e}"),
        };
        let _ = app.emit("uking:tray_driver_result", &msg);
        // 重建菜单刷新 ✓（切换后 active-drivers.json 已翻面）
        if let Some(tray) = app.tray_by_id("uking-tray") {
            if let Ok(m) = build_menu(&app) {
                let _ = tray.set_menu(Some(m));
            }
        }
    });
}

fn handle_menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    let id = event.id.as_ref();
    // 驱动快捷切换（"drv:<provider_id>"）
    if let Some(pid) = id.strip_prefix("drv:") {
        switch_driver(app, pid.to_string());
        return;
    }
    match id {
        "show" => show_main(app),
        "hide" => {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.hide();
            }
        }
        // 让前端去触发带进度的安装流（前端有 invoke + 事件订阅）
        "install" => {
            show_main(app);
            let _ = app.emit("uking:tray_action", "install");
        }
        // 打开「AI 设置」页（换驱动 / 填自己的 Key）
        "manage" => {
            show_main(app);
            let _ = app.emit("uking:tray_action", "manage");
        }
        "about" => {
            show_main(app);
            let _ = app.emit("uking:tray_action", "about");
        }
        "quit" => app.exit(0),
        _ => {}
    }
}

fn handle_tray_event(tray: &TrayIcon<Wry>, event: TrayIconEvent) {
    if let TrayIconEvent::Click {
        button: MouseButton::Left,
        button_state: MouseButtonState::Up,
        ..
    } = event
    {
        show_main(tray.app_handle());
    }
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}
