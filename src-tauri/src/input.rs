//! Native cursor output and physical-input monitoring. Guide mode never calls execute().
#[cfg(windows)]
mod platform {
    use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
    use windows_sys::Win32::{
        Foundation::{LPARAM, LRESULT, POINT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    };
    static READY: AtomicBool = AtomicBool::new(false);
    static AUTO: AtomicBool = AtomicBool::new(false);
    static PHYSICAL: AtomicBool = AtomicBool::new(false);
    static CLICKS: AtomicU64 = AtomicU64::new(0);
    static CLICK_X: AtomicI32 = AtomicI32::new(0);
    static CLICK_Y: AtomicI32 = AtomicI32::new(0);
    unsafe extern "system" fn mouse(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
        if code >= 0 {
            let data = &*(l as *const MSLLHOOKSTRUCT);
            if data.flags & 1 == 0 {
                if AUTO.load(Ordering::SeqCst) {
                    PHYSICAL.store(true, Ordering::SeqCst);
                }
                if w as u32 == WM_LBUTTONDOWN {
                    CLICK_X.store(data.pt.x, Ordering::SeqCst);
                    CLICK_Y.store(data.pt.y, Ordering::SeqCst);
                    CLICKS.fetch_add(1, Ordering::SeqCst);
                }
            }
        }
        CallNextHookEx(std::ptr::null_mut(), code, w, l)
    }
    unsafe extern "system" fn keyboard(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
        if code >= 0 && AUTO.load(Ordering::SeqCst) {
            let data = &*(l as *const KBDLLHOOKSTRUCT);
            if data.flags & 0x10 == 0 {
                PHYSICAL.store(true, Ordering::SeqCst);
            }
        }
        CallNextHookEx(std::ptr::null_mut(), code, w, l)
    }
    pub fn start_monitor() {
        std::thread::spawn(|| unsafe {
            let module = GetModuleHandleW(std::ptr::null());
            let mouse_hook = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse), module, 0);
            let key_hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard), module, 0);
            if mouse_hook.is_null() || key_hook.is_null() {
                if !mouse_hook.is_null() {
                    UnhookWindowsHookEx(mouse_hook);
                }
                if !key_hook.is_null() {
                    UnhookWindowsHookEx(key_hook);
                }
                return;
            }
            READY.store(true, Ordering::SeqCst);
            let mut message: MSG = std::mem::zeroed();
            while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            READY.store(false, Ordering::SeqCst);
            UnhookWindowsHookEx(mouse_hook);
            UnhookWindowsHookEx(key_hook);
        });
    }
    pub fn ready() -> bool {
        READY.load(Ordering::SeqCst)
    }
    pub fn set_auto(active: bool) {
        if !active {
            AUTO.store(false, Ordering::SeqCst);
            PHYSICAL.store(false, Ordering::SeqCst);
        } else if !AUTO.load(Ordering::SeqCst) {
            PHYSICAL.store(false, Ordering::SeqCst);
            AUTO.store(true, Ordering::SeqCst);
        }
    }
    pub fn physical_seen() -> bool {
        PHYSICAL.load(Ordering::SeqCst)
    }
    pub fn clicks() -> u64 {
        CLICKS.load(Ordering::SeqCst)
    }
    fn target(x: f64, y: f64) -> (i32, i32) {
        unsafe {
            (
                (x * (GetSystemMetrics(SM_CXSCREEN) - 1) as f64).round() as i32,
                (y * (GetSystemMetrics(SM_CYSCREEN) - 1) as f64).round() as i32,
            )
        }
    }
    pub fn guided_done(x: f64, y: f64, click: bool, since: u64) -> bool {
        let (tx, ty) = target(x, y);
        let (px, py) = if click {
            if clicks() <= since {
                return false;
            }
            (
                CLICK_X.load(Ordering::SeqCst),
                CLICK_Y.load(Ordering::SeqCst),
            )
        } else {
            unsafe {
                let mut p: POINT = std::mem::zeroed();
                if GetCursorPos(&mut p) == 0 {
                    return false;
                }
                (p.x, p.y)
            }
        };
        (px - tx).abs() <= 24 && (py - ty).abs() <= 24
    }
    pub fn execute(x: f64, y: f64, click: bool, allowed: impl Fn() -> bool) -> Result<(), String> {
        if !ready() || physical_seen() || !allowed() {
            return Err("Auto action cancelled before cursor movement.".into());
        }
        let (px, py) = target(x, y);
        unsafe {
            let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
            let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN);
            let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN);
            if vw <= 1 || vh <= 1 {
                return Err("Desktop geometry is unavailable.".into());
            }
            let mut movement: INPUT = std::mem::zeroed();
            movement.r#type = INPUT_MOUSE;
            movement.Anonymous.mi = MOUSEINPUT {
                dx: (((px - vx) as i64 * 65535) / (vw - 1) as i64) as i32,
                dy: (((py - vy) as i64 * 65535) / (vh - 1) as i64) as i32,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
                time: 0,
                dwExtraInfo: 0x424f42,
            };
            if !allowed() || physical_seen() {
                return Err("Auto action cancelled.".into());
            }
            if SendInput(1, &movement, std::mem::size_of::<INPUT>() as i32) != 1 {
                return Err("Windows blocked cursor movement. Session stopped.".into());
            }
            if click {
                if !allowed() || physical_seen() {
                    return Err("Auto click cancelled.".into());
                }
                let mut pair: [INPUT; 2] = std::mem::zeroed();
                for (input, flag) in pair
                    .iter_mut()
                    .zip([MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP])
                {
                    input.r#type = INPUT_MOUSE;
                    input.Anonymous.mi.dwFlags = flag;
                    input.Anonymous.mi.dwExtraInfo = 0x424f42;
                }
                let sent = SendInput(2, pair.as_ptr(), std::mem::size_of::<INPUT>() as i32);
                if sent != 2 {
                    // If only the down event was accepted, release it even after Stop.
                    if sent == 1 {
                        SendInput(1, &pair[1], std::mem::size_of::<INPUT>() as i32);
                    }
                    return Err("Windows blocked the click. Session stopped.".into());
                }
            }
        }
        Ok(())
    }
}
#[cfg(windows)]
pub use platform::*;
#[cfg(not(windows))]
pub fn start_monitor() {}
#[cfg(not(windows))]
pub fn ready() -> bool {
    false
}
#[cfg(not(windows))]
pub fn set_auto(_: bool) {}
#[cfg(not(windows))]
pub fn physical_seen() -> bool {
    false
}
#[cfg(not(windows))]
pub fn clicks() -> u64 {
    0
}
#[cfg(not(windows))]
pub fn guided_done(_: f64, _: f64, _: bool, _: u64) -> bool {
    false
}
#[cfg(not(windows))]
pub fn execute(_: f64, _: f64, _: bool, _: impl Fn() -> bool) -> Result<(), String> {
    Err("Auto cursor control currently requires Windows.".into())
}
