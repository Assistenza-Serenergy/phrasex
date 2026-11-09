//! Windows keyboard listener (low-level hooks on a dedicated thread) + text injector.
//!
//! The hook callbacks only translate the key and push it into a channel: no locks, no I/O,
//! because Windows silently drops a low-level hook that exceeds `LowLevelHooksTimeout`.
#![cfg(windows)]

use phrasex_engine::Key;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicIsize, Ordering::Relaxed};
use std::sync::{mpsc, OnceLock};
use std::thread::sleep;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
use windows_sys::Win32::System::DataExchange::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Memory::*;
use windows_sys::Win32::System::Ole::CF_UNICODETEXT;
use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};
use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

static TX: OnceLock<mpsc::Sender<Key>> = OnceLock::new();
/// Last focused window that is not ours (except the main editor) and not the taskbar: where insertions go.
static LAST_EXTERNAL: AtomicIsize = AtomicIsize::new(0);
/// Our main editor window: phrases can be typed into it like into any other app.
static MAIN_WINDOW: AtomicIsize = AtomicIsize::new(0);
/// Clipboard sequence number left by our own paste/restore: not a user copy.
static OWN_CLIP_SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
/// Executables recently focused by the user, most recent first (for the exclusion picker).
static RECENT_APPS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Installs the hooks on their own message-loop thread and returns the stream of keys.
pub fn start() -> Result<mpsc::Receiver<Key>, String> {
    let (tx, rx) = mpsc::channel();
    TX.set(tx).map_err(|_| "listener already started")?;
    let (ready_tx, ready_rx) = mpsc::channel();
    std::thread::spawn(move || unsafe {
        remember(GetForegroundWindow());
        let hmod = GetModuleHandleW(std::ptr::null());
        let kb = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), hmod, 0);
        let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), hmod, 0);
        let focus = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND,
            null_mut(), Some(focus_proc), 0, 0, WINEVENT_OUTOFCONTEXT,
        );
        if kb.is_null() || mouse.is_null() || focus.is_null() {
            let _ = ready_tx.send(Err(format!("hook installation failed (error {})", GetLastError())));
            return;
        }
        let _ = ready_tx.send(Ok(()));
        let mut msg = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {}
    });
    ready_rx.recv().map_err(|e| e.to_string())??;
    Ok(rx)
}

fn send(key: Key) {
    if let Some(tx) = TX.get() {
        let _ = tx.send(key);
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && (wp == WM_KEYDOWN as WPARAM || wp == WM_SYSKEYDOWN as WPARAM) {
        let k = &*(lp as *const KBDLLHOOKSTRUCT);
        // Our own output is tagged: never feed it back. Other tools' input (on-screen keyboard,
        // dictation, remote desktop) counts as typing.
        if k.dwExtraInfo != OWN_INPUT_TAG {
            if k.vkCode == VK_PACKET as u32 {
                // Unicode injected by another tool: the character is in the scan code.
                if let Some(Ok(c)) = char::decode_utf16([k.scanCode as u16]).next() {
                    send(if c.is_control() { Key::Reset } else { Key::Char(c) });
                }
            } else {
                translate(k.vkCode, k.scanCode);
            }
        }
    }
    CallNextHookEx(null_mut(), code, wp, lp)
}

unsafe extern "system" fn mouse_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && matches!(wp as u32, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN) {
        send(Key::Reset); // a click usually moves the caret
    }
    CallNextHookEx(null_mut(), code, wp, lp)
}

unsafe extern "system" fn focus_proc(_: HWINEVENTHOOK, _: u32, hwnd: HWND, _: i32, _: i32, _: u32, _: u32) {
    send(Key::Reset);
    remember(hwnd);
}

unsafe fn remember(hwnd: HWND) {
    if hwnd.is_null() || (window_pid(hwnd) == std::process::id() && hwnd as isize != MAIN_WINDOW.load(Relaxed)) {
        return; // our popup / form / tray menu: insertions go back to the window before it
    }
    if !is_shell(hwnd) {
        LAST_EXTERNAL.store(hwnd as isize, Relaxed);
        let exe = exe_of(hwnd);
        if !exe.is_empty() {
            let mut recent = RECENT_APPS.lock().unwrap_or_else(|p| p.into_inner());
            recent.retain(|e| *e != exe);
            recent.insert(0, exe);
            recent.truncate(15);
        }
    }
}

/// Executables the user recently worked in, most recent first.
pub fn recent_apps() -> Vec<String> {
    RECENT_APPS.lock().unwrap_or_else(|p| p.into_inner()).clone()
}

/// Taskbar / tray overflow: they get the focus while the user opens our tray menu.
unsafe fn is_shell(hwnd: HWND) -> bool {
    let mut class = [0u16; 64];
    let n = GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32).max(0) as usize;
    matches!(
        String::from_utf16_lossy(&class[..n]).as_str(),
        "Shell_TrayWnd" | "Shell_SecondaryTrayWnd" | "NotifyIconOverflowWindow" | "TopLevelWindowForOverflowXamlIsland"
    )
}

unsafe fn window_pid(hwnd: HWND) -> u32 {
    let mut pid = 0;
    GetWindowThreadProcessId(hwnd, &mut pid);
    pid
}

/// VK -> characters using the keyboard layout of the foreground window (Italian, AltGr, Shift...).
unsafe fn translate(vk: u32, scan: u32) {
    match vk as VIRTUAL_KEY {
        VK_BACK => return send(Key::Backspace),
        VK_SHIFT | VK_LSHIFT | VK_RSHIFT | VK_CONTROL | VK_LCONTROL | VK_RCONTROL | VK_MENU
        | VK_LMENU | VK_RMENU | VK_CAPITAL | VK_LWIN | VK_RWIN => return, // modifiers alone
        _ => {}
    }
    let mut state = [0u8; 256];
    for v in [VK_SHIFT, VK_CONTROL, VK_MENU] {
        if GetAsyncKeyState(v as i32) < 0 {
            state[v as usize] = 0x80;
        }
    }
    state[VK_CAPITAL as usize] = (GetKeyState(VK_CAPITAL as i32) & 1) as u8;
    let layout = GetKeyboardLayout(GetWindowThreadProcessId(GetForegroundWindow(), null_mut()));
    let mut buf = [0u16; 8];
    // Flag 0x4: do not alter the kernel keyboard state (keeps the user's dead keys intact).
    let n = ToUnicodeEx(vk, scan, state.as_ptr(), buf.as_mut_ptr(), buf.len() as i32, 0x4, layout);
    if n < 0 {
        return; // ponytail: dead key ignored (none on the IT layout); compose it if other layouts need it
    }
    // n == 0 (arrows, Home, F-keys...) or a control char (Enter, Tab, Esc, Ctrl+X) => caret context lost.
    let chars: Vec<char> = char::decode_utf16(buf[..n as usize].iter().copied()).filter_map(Result::ok).collect();
    if chars.is_empty() || chars.iter().any(|c| c.is_control()) {
        return send(Key::Reset);
    }
    chars.into_iter().for_each(|c| send(Key::Char(c)));
}

/// True when the focused window belongs to this process (don't expand inside our own editor).
pub fn foreground_is_own_process() -> bool {
    unsafe { window_pid(GetForegroundWindow()) == std::process::id() }
}

/// Registers our main editor window as a valid insertion target.
pub fn set_main_window(hwnd: isize) {
    MAIN_WINDOW.store(hwnd, Relaxed);
}

/// Text caret of the focused window in screen pixels (bottom-left).
/// Win32 caret first (classic apps, Office), then the accessibility caret (Chrome, Edge, Electron,
/// Firefox, WebView apps). None when the app exposes neither: callers fall back to the mouse.
pub fn caret_pos() -> Option<(i32, i32)> {
    unsafe {
        let thread = GetWindowThreadProcessId(GetForegroundWindow(), null_mut());
        let mut info: GUITHREADINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
        if GetGUIThreadInfo(thread, &mut info) == 0 {
            return None;
        }
        if !info.hwndCaret.is_null() {
            let mut p = POINT { x: info.rcCaret.left, y: info.rcCaret.bottom };
            if ClientToScreen(info.hwndCaret, &mut p) != 0 && (p.x, p.y) != (0, 0) {
                return Some((p.x, p.y));
            }
        }
        let target = if info.hwndFocus.is_null() { GetForegroundWindow() } else { info.hwndFocus };
        accessible_caret(target as isize)
    }
}

/// MSAA caret object (OBJID_CARET): what screen magnifiers use, exposed by Chromium-based apps too.
fn accessible_caret(hwnd: isize) -> Option<(i32, i32)> {
    use windows::core::Interface;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    use windows::Win32::System::Variant::VARIANT;
    use windows::Win32::UI::Accessibility::{AccessibleObjectFromWindow, IAccessible};
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED); // no-op when the thread already has COM
        let mut raw = null_mut();
        let objid_caret = -8i32 as u32;
        AccessibleObjectFromWindow(windows::Win32::Foundation::HWND(hwnd as _), objid_caret, &IAccessible::IID, &mut raw).ok()?;
        if raw.is_null() {
            return None;
        }
        let acc = IAccessible::from_raw(raw);
        let (mut x, mut y, mut w, mut h) = (0, 0, 0, 0);
        acc.accLocation(&mut x, &mut y, &mut w, &mut h, &VARIANT::from(0i32)).ok()?;
        (x != 0 || y != 0 || h != 0).then_some((x, y + h))
    }
}

/// Brings one of our windows to the front even when another app has the focus (the chooser opens
/// while the user types elsewhere, and Windows refuses plain focus changes from background apps).
pub fn force_foreground(hwnd: isize) {
    unsafe {
        let target = hwnd as HWND;
        let fg_thread = GetWindowThreadProcessId(GetForegroundWindow(), null_mut());
        let me = GetCurrentThreadId();
        let attached = fg_thread != 0 && fg_thread != me && AttachThreadInput(me, fg_thread, 1) != 0;
        BringWindowToTop(target);
        SetForegroundWindow(target);
        if attached {
            AttachThreadInput(me, fg_thread, 0);
        }
    }
}

/// Handle of the focused window (cheap; use it to cache [`foreground_exe`]).
pub fn foreground_window() -> isize {
    unsafe { GetForegroundWindow() as isize }
}

/// Lower-case executable name of the focused window, e.g. "outlook.exe" ("" if unknown).
pub fn foreground_exe() -> String {
    unsafe { exe_of(GetForegroundWindow()) }
}

unsafe fn exe_of(hwnd: HWND) -> String {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, window_pid(hwnd));
        if process.is_null() {
            return String::new();
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(process, 0, buf.as_mut_ptr(), &mut len) != 0;
        CloseHandle(process);
        if !ok {
            return String::new();
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        path.rsplit('\\').next().unwrap_or("").to_lowercase()
    }
}

/// Gives the focus back to the window the user was working in (after our popup, form or tray menu).
/// Only acts when *we* hold the focus (popup, form, tray menu, taskbar): after a typed abbreviation
/// the user's window already has it and must never be switched.
pub fn restore_focus() {
    let target = LAST_EXTERNAL.load(Relaxed) as HWND;
    unsafe {
        let fg = GetForegroundWindow();
        if target.is_null() || fg == target || !(fg.is_null() || window_pid(fg) == std::process::id() || is_shell(fg)) {
            return;
        }
        SetForegroundWindow(target);
    }
    sleep(Duration::from_millis(120)); // let the target process the activation before typing
}

/// Mouse position in physical screen pixels.
pub fn cursor_pos() -> (i32, i32) {
    let mut p = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut p) };
    (p.x, p.y)
}

/// Waits (max 1 s) until Ctrl/Alt/Shift/Win are released, so typed text is not read as shortcuts.
fn wait_modifiers_released() {
    let start = Instant::now();
    let held = || {
        [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN].iter().any(|&v| unsafe { GetAsyncKeyState(v as i32) } < 0)
    };
    while held() && start.elapsed() < Duration::from_secs(1) {
        sleep(Duration::from_millis(10));
    }
}

/// Real OS values for the macro engine (forms unsupported: used by the headless demo).
pub struct System;

impl phrasex_engine::macros::Context for System {
    fn now(&self) -> phrasex_engine::macros::DateTime {
        let mut t = unsafe { std::mem::zeroed() };
        unsafe { windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut t) };
        phrasex_engine::macros::DateTime {
            year: t.wYear,
            month: t.wMonth as u8,
            day: t.wDay as u8,
            hour: t.wHour as u8,
            minute: t.wMinute as u8,
            second: t.wSecond as u8,
        }
    }

    fn clipboard(&self) -> String {
        with_clipboard(|| unsafe {
            let h = GetClipboardData(CF_UNICODETEXT as u32);
            let p = if h.is_null() { std::ptr::null() } else { GlobalLock(h) as *const u16 };
            if p.is_null() {
                return String::new();
            }
            let len = (0..).take_while(|&i| *p.add(i) != 0).count();
            let text = String::from_utf16_lossy(std::slice::from_raw_parts(p, len));
            GlobalUnlock(h);
            text
        })
        .unwrap_or_default()
    }

    fn form(&self, _: &[phrasex_engine::macros::Field]) -> Option<std::collections::HashMap<String, String>> {
        None
    }
}

/// Opens the clipboard (retrying while another app holds it), runs `f`, closes it.
fn with_clipboard<T>(f: impl FnOnce() -> T) -> Option<T> {
    for _ in 0..20 {
        if unsafe { OpenClipboard(null_mut()) } != 0 {
            let out = f();
            unsafe { CloseClipboard() };
            return Some(out);
        }
        sleep(Duration::from_millis(10));
    }
    None
}

/// Text longer than this (or multi-line) is pasted: atomic, so busy apps (Win11 Notepad) cannot drop or
/// reorder keys, and Enter cannot send chats. ponytail: fixed; make it a setting if an app blocks paste.
const PASTE_MIN_CHARS: usize = 2;
const KEY_DELAY_MS: u64 = 5; // ponytail: fixed pacing (~200 chars/s); make it a setting if an app needs slower
const PASTE_SETTLE_MS: u64 = 300;

/// Erases `backspaces` characters, then inserts `text` (typed, or pasted when long / multi-line).
pub fn inject(backspaces: usize, text: &str) {
    wait_modifiers_released();
    // Each stroke is one SendInput (modifiers + key), followed by a short pause.
    let mut strokes: Vec<Vec<INPUT>> = (0..backspaces).map(|_| press(VK_BACK, 0)).collect();
    let paste_it = text.contains('\n') || text.chars().count() > PASTE_MIN_CHARS;
    if !paste_it {
        let layout = unsafe { GetKeyboardLayout(GetWindowThreadProcessId(GetForegroundWindow(), null_mut())) };
        for c in text.chars().filter(|&c| c != '\r') {
            strokes.push(char_stroke(c, layout));
        }
    }
    for s in &strokes {
        send_input(s);
        sleep(Duration::from_millis(KEY_DELAY_MS));
    }
    if paste_it && !text.is_empty() {
        paste(&text.replace("\r\n", "\n").replace('\n', "\r\n"));
    }
}

fn press(vk: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS) -> Vec<INPUT> {
    vec![key(vk, 0, flags), key(vk, 0, flags | KEYEVENTF_KEYUP)]
}

/// A character as real keys of the target's layout (Shift / AltGr included), so the app sees
/// ordinary typing. Characters missing from the layout fall back to Unicode packets.
/// (Unicode packets alone are unreliable: a busy app reads every queued packet as the last one.)
fn char_stroke(c: char, layout: HKL) -> Vec<INPUT> {
    if c == '\t' {
        return press(VK_TAB, 0);
    }
    let mut units = [0u16; 2];
    let encoded = c.encode_utf16(&mut units);
    let scan = if encoded.len() == 1 { unsafe { VkKeyScanExW(encoded[0], layout) } } else { -1 };
    if scan == -1 {
        return encoded.iter().flat_map(|&u| [key(0, u, KEYEVENTF_UNICODE), key(0, u, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP)]).collect();
    }
    let vk = (scan & 0xff) as VIRTUAL_KEY;
    let state = (scan >> 8) & 0xff;
    let mods: Vec<VIRTUAL_KEY> = [(1, VK_SHIFT), (2, VK_CONTROL), (4, VK_MENU)]
        .into_iter()
        .filter(|(bit, _)| state & bit != 0)
        .map(|(_, m)| m)
        .collect();
    let mut out: Vec<INPUT> = mods.iter().map(|&m| key(m, 0, 0)).collect();
    out.extend(press(vk, 0));
    out.extend(mods.iter().rev().map(|&m| key(m, 0, KEYEVENTF_KEYUP)));
    out
}

fn key(vk: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    // Modern (WinUI/XAML) apps ignore virtual keys without the hardware scan code, so modifiers
    // like Shift/Ctrl would be lost: always send both.
    let scan = if vk != 0 && flags & KEYEVENTF_UNICODE == 0 { unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) as u16 } } else { scan };
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: OWN_INPUT_TAG } },
    }
}

/// Marks keystrokes sent by Phrase X ("PHRX") so the hook can skip exactly those.
const OWN_INPUT_TAG: usize = 0x5048_5258;

fn send_input(inputs: &[INPUT]) {
    unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32) };
}

/// Clipboard formats holding GDI handles instead of memory: can't be copied byte-wise.
const GDI_FORMATS: [u32; 8] = [2, 3, 9, 14, 0x80, 0x82, 0x83, 0x8E];

/// Pastes `text` with Ctrl+V and then puts the user's clipboard back as it was.
fn paste(text: &str) {
    let saved: Vec<(u32, Vec<u8>)> = with_clipboard(|| unsafe {
        let mut out = Vec::new();
        let mut fmt = EnumClipboardFormats(0);
        while fmt != 0 {
            if !GDI_FORMATS.contains(&fmt) {
                let h = GetClipboardData(fmt);
                let size = if h.is_null() { 0 } else { GlobalSize(h) };
                let p = if size == 0 { null_mut() } else { GlobalLock(h) };
                if !p.is_null() {
                    out.push((fmt, std::slice::from_raw_parts(p as *const u8, size).to_vec()));
                    GlobalUnlock(h);
                }
            }
            fmt = EnumClipboardFormats(fmt);
        }
        out
    })
    .unwrap_or_default();

    let utf16: Vec<u8> = text.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
    let ok = with_clipboard(|| unsafe {
        EmptyClipboard();
        set_bytes(CF_UNICODETEXT as u32, &utf16);
        exclude_from_history();
    });
    if ok.is_none() {
        return; // clipboard locked by another app: nothing was changed
    }
    send_input(&[key(VK_CONTROL, 0, 0), key(0x56, 0, 0), key(0x56, 0, KEYEVENTF_KEYUP), key(VK_CONTROL, 0, KEYEVENTF_KEYUP)]);
    sleep(Duration::from_millis(PASTE_SETTLE_MS)); // the target reads the clipboard asynchronously

    with_clipboard(|| unsafe {
        EmptyClipboard();
        for (fmt, bytes) in &saved {
            set_bytes(*fmt, bytes);
        }
    });
    OWN_CLIP_SEQ.store(clipboard_sequence(), Relaxed);
}

/// Changes every time anything is copied.
pub fn clipboard_sequence() -> u32 {
    unsafe { GetClipboardSequenceNumber() }
}

/// Text the user just copied, for the clipboard history. `None` for our own pastes and for content
/// marked private (password managers set the exclusion formats).
pub fn copied_text() -> Option<String> {
    if clipboard_sequence() == OWN_CLIP_SEQ.load(Relaxed) {
        return None;
    }
    let private = ["ExcludeClipboardContentFromMonitorProcessing\0", "Clipboard Viewer Ignore\0"].iter().any(|name| unsafe {
        let wide: Vec<u16> = name.encode_utf16().collect();
        let fmt = RegisterClipboardFormatW(wide.as_ptr());
        fmt != 0 && IsClipboardFormatAvailable(fmt) != 0
    });
    if private {
        return None;
    }
    use phrasex_engine::macros::Context;
    let text = System.clipboard();
    (!text.trim().is_empty()).then_some(text)
}

unsafe fn set_bytes(fmt: u32, bytes: &[u8]) {
    let h = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1));
    if h.is_null() {
        return;
    }
    let p = GlobalLock(h) as *mut u8;
    if p.is_null() {
        return;
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
    GlobalUnlock(h);
    SetClipboardData(fmt, h); // the clipboard owns `h` from here
}

/// Keeps our temporary paste out of Windows clipboard history (Win+V) and cloud clipboard.
unsafe fn exclude_from_history() {
    let name: Vec<u16> = "ExcludeClipboardContentFromMonitorProcessing\0".encode_utf16().collect();
    let fmt = RegisterClipboardFormatW(name.as_ptr());
    if fmt != 0 {
        set_bytes(fmt, &[0]);
    }
}

/// Presses a named key: Enter, Tab, Esc, Up, Down, Left, Right, Home, End, Delete, Backspace,
/// PageUp, PageDown, Space, F1–F12. Unknown names are ignored.
pub fn send_key(name: &str) {
    let lower = name.to_lowercase();
    let (vk, extended) = match lower.as_str() {
        "enter" | "return" | "invio" => (VK_RETURN, false),
        "tab" => (VK_TAB, false),
        "esc" | "escape" => (VK_ESCAPE, false),
        "space" | "spazio" => (VK_SPACE, false),
        "backspace" => (VK_BACK, false),
        "up" | "su" => (VK_UP, true),
        "down" | "giu" | "giù" => (VK_DOWN, true),
        "left" | "sinistra" => (VK_LEFT, true),
        "right" | "destra" => (VK_RIGHT, true),
        "home" => (VK_HOME, true),
        "end" | "fine" => (VK_END, true),
        "delete" | "del" | "canc" => (VK_DELETE, true),
        "pageup" => (VK_PRIOR, true),
        "pagedown" => (VK_NEXT, true),
        f => match f.strip_prefix('f').and_then(|n| n.parse::<u16>().ok()) {
            Some(n @ 1..=12) => (VK_F1 + n - 1, false),
            _ => return,
        },
    };
    let flags = if extended { KEYEVENTF_EXTENDEDKEY } else { 0 };
    wait_modifiers_released();
    send_input(&[key(vk, 0, flags), key(vk, 0, flags | KEYEVENTF_KEYUP)]);
    sleep(Duration::from_millis(KEY_DELAY_MS));
}

/// Moves the caret left `n` characters (used for `{#cursor}`).
pub fn press_left(n: usize) {
    for _ in 0..n {
        send_key("left");
    }
}

/// Opens a URL, file, folder or program with its default handler.
pub fn open(target: &str) {
    let _ = std::process::Command::new("explorer.exe").arg(target).spawn();
}

/// Opens a web page in the default browser (ShellExecute: works from an elevated Phrase X too,
/// and keeps long URLs with `&` intact).
pub fn open_url(url: &str) {
    use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (verb, target) = (wide("open"), wide(url));
    unsafe { ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), target.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL) };
}

/// True when Phrase X runs as administrator (UAC elevated): only then can it type into elevated windows.
pub fn is_elevated() -> bool {
    unsafe { windows_sys::Win32::UI::Shell::IsUserAnAdmin() != 0 }
}
