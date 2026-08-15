//! 托盘图标右键菜单（共享模块，供 main.rs tray_wndproc 调用）。

use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

/// 弹出托盘右键菜单，返回 true 表示用户选择了「退出」。
pub unsafe fn show_menu(hwnd: HWND) -> bool {
    let menu = CreatePopupMenu().unwrap();
    AppendMenuW(menu, MF_STRING, 1, w!("打开悬浮球")).unwrap();
    AppendMenuW(menu, MF_STRING, 2, w!("退出")).unwrap();

    // 经典 Win32 坑：托盘菜单前必须 SetForegroundWindow，否则点击菜单外区域不关闭
    SetForegroundWindow(hwnd);

    let mut pt = POINT::default();
    GetCursorPos(&mut pt).unwrap();

    let cmd = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_NONOTIFY,
        pt.x,
        pt.y,
        0,
        hwnd,
        None,
    );

    PostMessageW(hwnd, WM_NULL, WPARAM(0), LPARAM(0));
    DestroyMenu(menu).unwrap();

    let selected = cmd.0 as u32;
    match selected {
        1 => {
            // 显示悬浮球
            crate::ball::show_ball();
            false
        }
        2 => true,
        _ => false,
    }
}
