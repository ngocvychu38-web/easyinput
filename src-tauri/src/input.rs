#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::c_void;
    use objc2::{rc::Retained, runtime::ProtocolObject};
    use objc2_app_kit::{NSPasteboard, NSPasteboardItem, NSPasteboardTypeString, NSPasteboardWriting};
    use objc2_foundation::{NSArray, NSString};

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
            return Err("识别已完成，但 EasyInput 没有“辅助功能”权限，无法写入当前光标位置。".into());
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
            if !down.is_null() { unsafe { CFRelease(down); } }
            if !up.is_null() { unsafe { CFRelease(up); } }
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

    pub fn replace_selected_text(text: &str) -> Result<(), String> {
        if !unsafe { CGPreflightPostEventAccess() } {
            return Err("模型已返回，但 EasyInput 没有“辅助功能”权限，无法替换当前选区。".into());
        }
        if text.is_empty() { return Ok(()); }
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
        let Some(items) = pasteboard.pasteboardItems() else { return Vec::new(); };
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
        if snapshot.is_empty() { return; }
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
                if pasteboard.changeCount() == before { continue; }
                let value = pasteboard
                    .stringForType(unsafe { NSPasteboardTypeString })
                    .map(|value| value.to_string())
                    .filter(|value| value != "easyinput-selection-probe" && !value.trim().is_empty());
                if value.is_some() { return Ok(value); }
            }
            Ok(None)
        })();
        restore_pasteboard(&pasteboard, snapshot);
        capture
    }

    unsafe fn attribute(name: &str) -> Result<*const c_void, String> {
        let name = std::ffi::CString::new(name).map_err(|_| "辅助功能属性名称无效")?;
        let value = unsafe { CFStringCreateWithCString(std::ptr::null(), name.as_ptr(), UTF8) };
        if value.is_null() { Err("无法创建辅助功能属性".into()) } else { Ok(value) }
    }

    unsafe fn string_value(value: *const c_void) -> Result<String, String> {
        let length = unsafe { CFStringGetLength(value) };
        let capacity = unsafe { CFStringGetMaximumSizeForEncoding(length, UTF8) } + 1;
        if capacity <= 0 { return Ok(String::new()); }
        let mut buffer = vec![0_u8; capacity as usize];
        if !unsafe { CFStringGetCString(value, buffer.as_mut_ptr().cast(), capacity, UTF8) } {
            return Err("无法读取当前选中文本".into());
        }
        let end = buffer.iter().position(|byte| *byte == 0).unwrap_or(buffer.len());
        String::from_utf8(buffer[..end].to_vec()).map_err(|error| format!("选中文本不是有效 UTF-8：{error}"))
    }

    unsafe fn copy_attribute(element: *const c_void, name: &str) -> Result<Option<*const c_void>, String> {
        let attribute = unsafe { attribute(name)? };
        let mut value = std::ptr::null();
        let status = unsafe { AXUIElementCopyAttributeValue(element, attribute, &mut value) };
        unsafe { CFRelease(attribute); }
        if status == 0 && !value.is_null() { Ok(Some(value)) } else { Ok(None) }
    }

    unsafe fn selected_text_from_element(element: *const c_void) -> Result<Option<String>, String> {
        if let Some(selected) = unsafe { copy_attribute(element, "AXSelectedText")? } {
            let text = unsafe { string_value(selected) };
            unsafe { CFRelease(selected); }
            if let Ok(value) = text {
                if !value.trim().is_empty() { return Ok(Some(value)); }
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
        unsafe { CFRelease(parameterized); CFRelease(range); }
        if status != 0 || selected.is_null() { return Ok(None); }
        let text = unsafe { string_value(selected) };
        unsafe { CFRelease(selected); }
        text.map(|value| if value.trim().is_empty() { None } else { Some(value) })
    }

    pub fn selected_text() -> Result<Option<String>, String> {
        if !unsafe { CGPreflightPostEventAccess() } {
            return Err("EasyInput 没有“辅助功能”权限，无法读取当前选中文本。".into());
        }
        let system = unsafe { AXUIElementCreateSystemWide() };
        if system.is_null() { return selected_text_via_copy(); }
        let focused_attribute = match unsafe { attribute("AXFocusedUIElement") } {
            Ok(value) => value,
            Err(_) => { unsafe { CFRelease(system); } return selected_text_via_copy(); }
        };
        let mut focused = std::ptr::null();
        let focused_status = unsafe { AXUIElementCopyAttributeValue(system, focused_attribute, &mut focused) };
        unsafe { CFRelease(focused_attribute); CFRelease(system); }
        if focused_status != 0 || focused.is_null() { return selected_text_via_copy(); }
        let accessibility_text = unsafe { selected_text_from_element(focused) };
        unsafe { CFRelease(focused); }
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
