fn shortcut_key_code(key: &str) -> Option<u16> {
    Some(match key {
        "a" => 0,
        "s" => 1,
        "d" => 2,
        "f" => 3,
        "h" => 4,
        "g" => 5,
        "z" => 6,
        "x" => 7,
        "c" => 8,
        "v" => 9,
        "b" => 11,
        "q" => 12,
        "w" => 13,
        "e" => 14,
        "r" => 15,
        "y" => 16,
        "t" => 17,
        "1" => 18,
        "2" => 19,
        "3" => 20,
        "4" => 21,
        "6" => 22,
        "5" => 23,
        "=" => 24,
        "9" => 25,
        "7" => 26,
        "-" => 27,
        "8" => 28,
        "0" => 29,
        "]" => 30,
        "o" => 31,
        "u" => 32,
        "[" => 33,
        "i" => 34,
        "p" => 35,
        "enter" | "return" => 36,
        "l" => 37,
        "j" => 38,
        "'" => 39,
        "k" => 40,
        ";" => 41,
        "\\" => 42,
        "," => 43,
        "/" => 44,
        "n" => 45,
        "m" => 46,
        "." => 47,
        "tab" => 48,
        "space" => 49,
        "`" => 50,
        "backspace" | "delete" => 51,
        "escape" | "esc" => 53,
        "f1" => 122,
        "f2" => 120,
        "f3" => 99,
        "f4" => 118,
        "f5" => 96,
        "f6" => 97,
        "f7" => 98,
        "f8" => 100,
        "f9" => 101,
        "f10" => 109,
        "f11" => 103,
        "f12" => 111,
        "left" | "leftarrow" => 123,
        "right" | "rightarrow" => 124,
        "down" | "downarrow" => 125,
        "up" | "uparrow" => 126,
        _ => return None,
    })
}

fn parse_shortcut(shortcut: &str) -> Result<(u16, u64), String> {
    if shortcut.trim().is_empty() || shortcut.len() > 80 {
        return Err("快捷键为空或过长。".into());
    }
    const SHIFT_FLAG: u64 = 1 << 17;
    const CONTROL_FLAG: u64 = 1 << 18;
    const OPTION_FLAG: u64 = 1 << 19;
    const COMMAND_FLAG: u64 = 1 << 20;
    let mut flags = 0_u64;
    let mut key_code = None;
    for part in shortcut
        .split('+')
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
    {
        match part.as_str() {
            "command" | "cmd" | "meta" => flags |= COMMAND_FLAG,
            "shift" => flags |= SHIFT_FLAG,
            "option" | "alt" => flags |= OPTION_FLAG,
            "control" | "ctrl" => flags |= CONTROL_FLAG,
            key if key_code.is_none() => {
                key_code = Some(
                    shortcut_key_code(key).ok_or_else(|| format!("不支持快捷键按键“{key}”。"))?,
                )
            }
            _ => return Err("快捷键只能包含一个普通按键。".into()),
        }
    }
    Ok((
        key_code.ok_or_else(|| "快捷键缺少有效的普通按键。".to_string())?,
        flags,
    ))
}

pub fn validate_shortcut(shortcut: &str) -> Result<(), String> {
    parse_shortcut(shortcut).map(|_| ())
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2::{rc::Retained, runtime::ProtocolObject};
    use objc2_app_kit::{
        NSPasteboard, NSPasteboardItem, NSPasteboardTypeString, NSPasteboardWriting,
    };
    use objc2_foundation::{NSArray, NSString};
    use std::ffi::c_void;

    type CGEventRef = *mut c_void;
    const UTF8: u32 = 0x0800_0100;

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGPreflightPostEventAccess() -> bool;
        fn CGRequestPostEventAccess() -> bool;
        fn CGEventCreateKeyboardEvent(
            source: *const c_void,
            virtual_key: u16,
            key_down: bool,
        ) -> CGEventRef;
        fn CGEventKeyboardSetUnicodeString(
            event: CGEventRef,
            string_length: usize,
            unicode_string: *const u16,
        );
        fn CGEventPost(tap: u32, event: CGEventRef);
        fn CGEventSetFlags(event: CGEventRef, flags: u64);
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXUIElementCreateSystemWide() -> *const c_void;
        fn AXUIElementCopyAttributeValue(
            element: *const c_void,
            attribute: *const c_void,
            value: *mut *const c_void,
        ) -> i32;
        fn AXUIElementCopyParameterizedAttributeValue(
            element: *const c_void,
            attribute: *const c_void,
            parameter: *const c_void,
            value: *mut *const c_void,
        ) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(value: *const c_void);
        fn CFStringCreateWithCString(
            allocator: *const c_void,
            text: *const std::ffi::c_char,
            encoding: u32,
        ) -> *const c_void;
        fn CFStringGetLength(value: *const c_void) -> isize;
        fn CFStringGetMaximumSizeForEncoding(length: isize, encoding: u32) -> isize;
        fn CFStringGetCString(
            value: *const c_void,
            buffer: *mut std::ffi::c_char,
            buffer_size: isize,
            encoding: u32,
        ) -> bool;
    }

    pub fn request_access() -> bool {
        unsafe { CGPreflightPostEventAccess() || CGRequestPostEventAccess() }
    }

    pub fn type_text(text: &str) -> Result<(), String> {
        if !unsafe { CGPreflightPostEventAccess() } {
            return Err(
                "识别已完成，但 EasyInput 没有“辅助功能”权限，无法写入当前光标位置。".into(),
            );
        }
        let mut units = Vec::new();
        for character in text.chars() {
            let mut encoded = [0_u16; 2];
            let encoded = character.encode_utf16(&mut encoded);
            if units.len() + encoded.len() > 20 {
                post_chunk(&units)?;
                units.clear();
            }
            units.extend_from_slice(encoded);
        }
        if !units.is_empty() {
            post_chunk(&units)?;
        }
        Ok(())
    }

    fn post_chunk(units: &[u16]) -> Result<(), String> {
        let event = unsafe { CGEventCreateKeyboardEvent(std::ptr::null(), 0, true) };
        if event.is_null() {
            return Err("无法创建 macOS 文本输入事件。".into());
        }
        unsafe {
            CGEventKeyboardSetUnicodeString(event, units.len(), units.as_ptr());
            CGEventPost(0, event);
            CFRelease(event);
        }
        std::thread::sleep(std::time::Duration::from_millis(3));
        Ok(())
    }

    fn post_copy_shortcut() -> Result<(), String> {
        post_command_shortcut(8, "复制选区")
    }

    fn post_paste_shortcut() -> Result<(), String> {
        post_command_shortcut(9, "粘贴文本")
    }

    fn post_command_shortcut(key_code: u16, action: &str) -> Result<(), String> {
        const COMMAND_FLAG: u64 = 1 << 20;
        let down = unsafe { CGEventCreateKeyboardEvent(std::ptr::null(), key_code, true) };
        let up = unsafe { CGEventCreateKeyboardEvent(std::ptr::null(), key_code, false) };
        if down.is_null() || up.is_null() {
            if !down.is_null() {
                unsafe {
                    CFRelease(down);
                }
            }
            if !up.is_null() {
                unsafe {
                    CFRelease(up);
                }
            }
            return Err(format!("无法创建{action}的键盘事件。"));
        }
        unsafe {
            CGEventSetFlags(down, COMMAND_FLAG);
            CGEventSetFlags(up, COMMAND_FLAG);
            CGEventPost(0, down);
            CGEventPost(0, up);
            CFRelease(down);
            CFRelease(up);
        }
        Ok(())
    }

    pub fn press_shortcut(shortcut: &str) -> Result<(), String> {
        if !unsafe { CGPreflightPostEventAccess() } {
            return Err("EasyInput 没有“辅助功能”权限，无法执行快捷键。".into());
        }
        let (key_code, flags) = super::parse_shortcut(shortcut)?;
        let down = unsafe { CGEventCreateKeyboardEvent(std::ptr::null(), key_code, true) };
        let up = unsafe { CGEventCreateKeyboardEvent(std::ptr::null(), key_code, false) };
        if down.is_null() || up.is_null() {
            if !down.is_null() {
                unsafe {
                    CFRelease(down);
                }
            }
            if !up.is_null() {
                unsafe {
                    CFRelease(up);
                }
            }
            return Err("无法创建 macOS 快捷键事件。".into());
        }
        unsafe {
            CGEventSetFlags(down, flags);
            CGEventSetFlags(up, flags);
            CGEventPost(0, down);
            CGEventPost(0, up);
            CFRelease(down);
            CFRelease(up);
        }
        Ok(())
    }

    pub fn replace_selected_text(text: &str) -> Result<(), String> {
        if !unsafe { CGPreflightPostEventAccess() } {
            return Err("模型已返回，但 EasyInput 没有“辅助功能”权限，无法替换当前选区。".into());
        }
        if text.is_empty() {
            return Ok(());
        }
        let pasteboard = NSPasteboard::generalPasteboard();
        let snapshot = snapshot_pasteboard(&pasteboard);
        pasteboard.clearContents();
        let replacement = NSString::from_str(text);
        if !pasteboard.setString_forType(&replacement, unsafe { NSPasteboardTypeString }) {
            restore_pasteboard(&pasteboard, snapshot);
            return Err("无法把模型结果放入临时剪贴板。".into());
        }
        let pasted = post_paste_shortcut();
        // Rich text editors consume the pasteboard on their next run-loop
        // turn. Keep the replacement available briefly, then restore every
        // original pasteboard item and flavor.
        std::thread::sleep(std::time::Duration::from_millis(180));
        restore_pasteboard(&pasteboard, snapshot);
        pasted
    }

    fn snapshot_pasteboard(pasteboard: &NSPasteboard) -> Vec<Retained<NSPasteboardItem>> {
        let Some(items) = pasteboard.pasteboardItems() else {
            return Vec::new();
        };
        let mut snapshot = Vec::with_capacity(items.count());
        for item_index in 0..items.count() {
            let source = items.objectAtIndex(item_index);
            let copy = NSPasteboardItem::new();
            let types = source.types();
            for type_index in 0..types.count() {
                let data_type = types.objectAtIndex(type_index);
                if let Some(data) = source.dataForType(&data_type) {
                    copy.setData_forType(&data, &data_type);
                }
            }
            snapshot.push(copy);
        }
        snapshot
    }

    fn restore_pasteboard(pasteboard: &NSPasteboard, snapshot: Vec<Retained<NSPasteboardItem>>) {
        pasteboard.clearContents();
        if snapshot.is_empty() {
            return;
        }
        let objects: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> = snapshot
            .into_iter()
            .map(ProtocolObject::from_retained)
            .collect();
        let objects = NSArray::from_retained_slice(&objects);
        pasteboard.writeObjects(&objects);
    }

    fn selected_text_via_copy() -> Result<Option<String>, String> {
        let pasteboard = NSPasteboard::generalPasteboard();
        let snapshot = snapshot_pasteboard(&pasteboard);
        pasteboard.clearContents();
        let marker = NSString::from_str("easyinput-selection-probe");
        pasteboard.setString_forType(&marker, unsafe { NSPasteboardTypeString });
        let before = pasteboard.changeCount();

        let capture = (|| {
            post_copy_shortcut()?;
            for _ in 0..25 {
                std::thread::sleep(std::time::Duration::from_millis(20));
                if pasteboard.changeCount() == before {
                    continue;
                }
                let value = pasteboard
                    .stringForType(unsafe { NSPasteboardTypeString })
                    .map(|value| value.to_string())
                    .filter(|value| {
                        value != "easyinput-selection-probe" && !value.trim().is_empty()
                    });
                if value.is_some() {
                    return Ok(value);
                }
            }
            Ok(None)
        })();
        restore_pasteboard(&pasteboard, snapshot);
        capture
    }

    unsafe fn attribute(name: &str) -> Result<*const c_void, String> {
        let name = std::ffi::CString::new(name).map_err(|_| "辅助功能属性名称无效")?;
        let value = unsafe { CFStringCreateWithCString(std::ptr::null(), name.as_ptr(), UTF8) };
        if value.is_null() {
            Err("无法创建辅助功能属性".into())
        } else {
            Ok(value)
        }
    }

    unsafe fn string_value(value: *const c_void) -> Result<String, String> {
        let length = unsafe { CFStringGetLength(value) };
        let capacity = unsafe { CFStringGetMaximumSizeForEncoding(length, UTF8) } + 1;
        if capacity <= 0 {
            return Ok(String::new());
        }
        let mut buffer = vec![0_u8; capacity as usize];
        if !unsafe { CFStringGetCString(value, buffer.as_mut_ptr().cast(), capacity, UTF8) } {
            return Err("无法读取当前选中文本".into());
        }
        let end = buffer
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(buffer.len());
        String::from_utf8(buffer[..end].to_vec())
            .map_err(|error| format!("选中文本不是有效 UTF-8：{error}"))
    }

    unsafe fn copy_attribute(
        element: *const c_void,
        name: &str,
    ) -> Result<Option<*const c_void>, String> {
        let attribute = unsafe { attribute(name)? };
        let mut value = std::ptr::null();
        let status = unsafe { AXUIElementCopyAttributeValue(element, attribute, &mut value) };
        unsafe {
            CFRelease(attribute);
        }
        if status == 0 && !value.is_null() {
            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    unsafe fn selected_text_from_element(element: *const c_void) -> Result<Option<String>, String> {
        if let Some(selected) = unsafe { copy_attribute(element, "AXSelectedText")? } {
            let text = unsafe { string_value(selected) };
            unsafe {
                CFRelease(selected);
            }
            if let Ok(value) = text {
                if !value.trim().is_empty() {
                    return Ok(Some(value));
                }
            }
        }

        let Some(range) = (unsafe { copy_attribute(element, "AXSelectedTextRange")? }) else {
            return Ok(None);
        };
        let parameterized = unsafe { attribute("AXStringForRange")? };
        let mut selected = std::ptr::null();
        let status = unsafe {
            AXUIElementCopyParameterizedAttributeValue(element, parameterized, range, &mut selected)
        };
        unsafe {
            CFRelease(parameterized);
            CFRelease(range);
        }
        if status != 0 || selected.is_null() {
            return Ok(None);
        }
        let text = unsafe { string_value(selected) };
        unsafe {
            CFRelease(selected);
        }
        text.map(|value| {
            if value.trim().is_empty() {
                None
            } else {
                Some(value)
            }
        })
    }

    pub fn selected_text() -> Result<Option<String>, String> {
        if !unsafe { CGPreflightPostEventAccess() } {
            return Err("EasyInput 没有“辅助功能”权限，无法读取当前选中文本。".into());
        }
        let system = unsafe { AXUIElementCreateSystemWide() };
        if system.is_null() {
            return selected_text_via_copy();
        }
        let focused_attribute = match unsafe { attribute("AXFocusedUIElement") } {
            Ok(value) => value,
            Err(_) => {
                unsafe {
                    CFRelease(system);
                }
                return selected_text_via_copy();
            }
        };
        let mut focused = std::ptr::null();
        let focused_status =
            unsafe { AXUIElementCopyAttributeValue(system, focused_attribute, &mut focused) };
        unsafe {
            CFRelease(focused_attribute);
            CFRelease(system);
        }
        if focused_status != 0 || focused.is_null() {
            return selected_text_via_copy();
        }
        let accessibility_text = unsafe { selected_text_from_element(focused) };
        unsafe {
            CFRelease(focused);
        }
        match accessibility_text {
            Ok(Some(text)) => Ok(Some(text)),
            Ok(None) | Err(_) => selected_text_via_copy(),
        }
    }
}

pub fn request_text_input_access() -> bool {
    #[cfg(target_os = "macos")]
    return macos::request_access();
    #[cfg(not(target_os = "macos"))]
    false
}

pub fn type_text(text: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return macos::type_text(text);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = text;
        Err("当前平台暂不支持自动写入识别文字。".into())
    }
}

pub fn replace_selected_text(text: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return macos::replace_selected_text(text);
    #[cfg(not(target_os = "macos"))]
    {
        type_text(text)
    }
}

pub fn selected_text() -> Result<Option<String>, String> {
    #[cfg(target_os = "macos")]
    return macos::selected_text();
    #[cfg(not(target_os = "macos"))]
    Ok(None)
}

pub fn press_shortcut(shortcut: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return macos::press_shortcut(shortcut);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = shortcut;
        Err("当前平台暂不支持执行快捷键。".into())
    }
}

#[cfg(test)]
mod shortcut_tests {
    use super::*;

    #[test]
    fn validates_supported_shortcuts() {
        assert!(validate_shortcut("Command+Shift+P").is_ok());
        assert!(validate_shortcut("Control+Space").is_ok());
        assert!(validate_shortcut("Enter").is_ok());
    }

    #[test]
    fn rejects_unknown_or_multiple_normal_keys() {
        assert!(validate_shortcut("Command+Banana").is_err());
        assert!(validate_shortcut("Command+A+B").is_err());
    }
}
