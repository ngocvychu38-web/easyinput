#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::c_void;

    type CGEventRef = *mut c_void;

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
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXUIElementCreateSystemWide() -> *const c_void;
        fn AXUIElementCopyAttributeValue(
            element: *const c_void,
            attribute: *const c_void,
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

    pub fn selected_text() -> Result<Option<String>, String> {
        if !unsafe { CGPreflightPostEventAccess() } {
            return Err("EasyInput 没有“辅助功能”权限，无法读取当前选中文本。".into());
        }
        const UTF8: u32 = 0x0800_0100;
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

        let system = unsafe { AXUIElementCreateSystemWide() };
        if system.is_null() { return Err("无法访问当前系统焦点".into()); }
        let focused_attribute = unsafe { attribute("AXFocusedUIElement")? };
        let mut focused = std::ptr::null();
        let focused_status = unsafe { AXUIElementCopyAttributeValue(system, focused_attribute, &mut focused) };
        unsafe { CFRelease(focused_attribute); CFRelease(system); }
        if focused_status != 0 || focused.is_null() { return Err("无法读取当前应用的焦点输入区域".into()); }
        let selected_attribute = unsafe { attribute("AXSelectedText")? };
        let mut selected = std::ptr::null();
        let selected_status = unsafe { AXUIElementCopyAttributeValue(focused, selected_attribute, &mut selected) };
        unsafe { CFRelease(selected_attribute); CFRelease(focused); }
        if selected_status != 0 || selected.is_null() { return Ok(None); }
        let text = unsafe { string_value(selected) };
        unsafe { CFRelease(selected); }
        text.map(|value| if value.is_empty() { None } else { Some(value) })
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

pub fn selected_text() -> Result<Option<String>, String> {
    #[cfg(target_os = "macos")]
    return macos::selected_text();
    #[cfg(not(target_os = "macos"))]
    Ok(None)
}
