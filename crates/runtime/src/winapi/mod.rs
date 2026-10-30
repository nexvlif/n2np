pub mod advapi32;
pub mod gdi32;
pub mod kernel32;
pub mod msvcrt;
pub mod ntdll;
pub mod user32;

use crate::modules;

fn addr(function: *const ()) -> usize {
    function as usize
}

pub fn normalize_dll_name(name: &str) -> String {
    let dll = modules::normalize(name);
    match dll.as_str() {
        "kernelbase.dll" => "kernel32.dll".to_string(),
        "ucrtbase.dll" => "msvcrt.dll".to_string(),
        "advapi32.dll" | "sechost.dll" => "advapi32.dll".to_string(),
        _ if dll.starts_with("api-ms-win-crt") => "msvcrt.dll".to_string(),
        _ if dll.starts_with("api-ms-win-core-registry") => "advapi32.dll".to_string(),
        _ if dll.starts_with("api-ms-win-core") => "kernel32.dll".to_string(),
        _ if dll.starts_with("api-ms-win-security") => "advapi32.dll".to_string(),
        _ => dll,
    }
}

macro_rules! api_table {
    (
        $(
            $dll:literal {
                $( $name:literal => $path:path ),* $(,)?
            }
        ),* $(,)?
    ) => {
        pub fn resolve(dll: &str, name: &str) -> Option<usize> {
            let dll = normalize_dll_name(dll);
            match (dll.as_str(), name) {
                $(
                    $(
                        ($dll, $name) => Some(addr($path as *const ())),
                    )*
                )*
                _ => None,
            }
        }

        pub fn supported_apis() -> Vec<(&'static str, &'static str)> {
            let mut apis = vec![
                $(
                    $(
                        ($dll, $name),
                    )*
                )*
            ];
            apis.sort_unstable();
            apis.dedup();
            apis
        }
    };
}

api_table! {
    "kernel32.dll" {
        "GetStdHandle" => kernel32::get_std_handle,
        "SetStdHandle" => kernel32::set_std_handle,
        "WriteFile" => kernel32::write_file,
        "ReadFile" => kernel32::read_file,
        "CreateFileA" => kernel32::create_file_a,
        "CreateFileW" => kernel32::create_file_w,
        "CloseHandle" => kernel32::close_handle,
        "GetFileSize" => kernel32::get_file_size,
        "GetFileSizeEx" => kernel32::get_file_size_ex,
        "SetFilePointer" => kernel32::set_file_pointer,
        "SetFilePointerEx" => kernel32::set_file_pointer_ex,
        "SetEndOfFile" => kernel32::set_end_of_file,
        "FlushFileBuffers" => kernel32::flush_file_buffers,
        "GetFileType" => kernel32::get_file_type,
        "DeleteFileA" => kernel32::delete_file_a,
        "DeleteFileW" => kernel32::delete_file_w,
        "CopyFileA" => kernel32::copy_file_a,
        "CopyFileW" => kernel32::copy_file_w,
        "MoveFileA" => kernel32::move_file_a,
        "MoveFileW" => kernel32::move_file_w,
        "CreateDirectoryA" => kernel32::create_directory_a,
        "CreateDirectoryW" => kernel32::create_directory_w,
        "RemoveDirectoryA" => kernel32::remove_directory_a,
        "RemoveDirectoryW" => kernel32::remove_directory_w,
        "GetFileAttributesA" => kernel32::get_file_attributes_a,
        "GetFileAttributesW" => kernel32::get_file_attributes_w,
        "SetFileAttributesA" => kernel32::set_file_attributes_a,
        "SetFileAttributesW" => kernel32::set_file_attributes_w,
        "GetCurrentDirectoryA" => kernel32::get_current_directory_a,
        "GetCurrentDirectoryW" => kernel32::get_current_directory_w,
        "SetCurrentDirectoryA" => kernel32::set_current_directory_a,
        "SetCurrentDirectoryW" => kernel32::set_current_directory_w,
        "GetFullPathNameA" => kernel32::get_full_path_name_a,
        "GetFullPathNameW" => kernel32::get_full_path_name_w,
        "GetTempPathA" => kernel32::get_temp_path_a,
        "GetTempPathW" => kernel32::get_temp_path_w,
        "GetTempFileNameA" => kernel32::get_temp_file_name_a,
        "GetTempFileNameW" => kernel32::get_temp_file_name_w,
        "GetModuleFileNameA" => kernel32::get_module_file_name_a,
        "GetModuleFileNameW" => kernel32::get_module_file_name_w,
        "GetModuleHandleA" => kernel32::get_module_handle_a,
        "GetModuleHandleW" => kernel32::get_module_handle_w,
        "GetModuleHandleExA" => kernel32::get_module_handle_ex_a,
        "GetModuleHandleExW" => kernel32::get_module_handle_ex_w,
        "LoadLibraryA" => kernel32::load_library_a,
        "LoadLibraryW" => kernel32::load_library_w,
        "GetProcAddress" => kernel32::get_proc_address,
        "FreeLibrary" => kernel32::free_library,
        "GetCommandLineA" => kernel32::get_command_line_a,
        "GetCommandLineW" => kernel32::get_command_line_w,
        "GetEnvironmentVariableA" => kernel32::get_environment_variable_a,
        "GetEnvironmentVariableW" => kernel32::get_environment_variable_w,
        "GetEnvironmentStrings" => kernel32::get_environment_strings_a,
        "GetEnvironmentStringsA" => kernel32::get_environment_strings_a,
        "GetEnvironmentStringsW" => kernel32::get_environment_strings_w,
        "FreeEnvironmentStringsA" => kernel32::free_environment_strings_a,
        "FreeEnvironmentStringsW" => kernel32::free_environment_strings_w,
        "ExpandEnvironmentStringsA" => kernel32::expand_environment_strings_a,
        "ExpandEnvironmentStringsW" => kernel32::expand_environment_strings_w,
        "GetLastError" => kernel32::get_last_error,
        "SetLastError" => kernel32::set_last_error,
        "GetCurrentProcess" => kernel32::get_current_process,
        "GetCurrentProcessId" => kernel32::get_current_process_id,
        "GetCurrentThread" => kernel32::get_current_thread,
        "GetCurrentThreadId" => kernel32::get_current_thread_id,
        "ExitProcess" => kernel32::exit_process,
        "TerminateProcess" => kernel32::terminate_process,
        "Sleep" => kernel32::sleep,
        "SleepEx" => kernel32::sleep_ex,
        "GetTickCount" => kernel32::get_tick_count,
        "GetTickCount64" => kernel32::get_tick_count64,
        "QueryPerformanceCounter" => kernel32::query_performance_counter,
        "QueryPerformanceFrequency" => kernel32::query_performance_frequency,
        "GetSystemTimeAsFileTime" => kernel32::get_system_time_as_file_time,
        "GetSystemTimePreciseAsFileTime" => kernel32::get_system_time_precise_as_file_time,
        "GetSystemTime" => kernel32::get_system_time,
        "GetLocalTime" => kernel32::get_local_time,
        "GetVersion" => kernel32::get_version,
        "GetVersionExA" => kernel32::get_version_ex_a,
        "GetVersionExW" => kernel32::get_version_ex_w,
        "GetSystemDirectoryA" => kernel32::get_system_directory_a,
        "GetSystemDirectoryW" => kernel32::get_system_directory_w,
        "GetWindowsDirectoryA" => kernel32::get_windows_directory_a,
        "GetWindowsDirectoryW" => kernel32::get_windows_directory_w,
        "GetComputerNameA" => kernel32::get_computer_name_a,
        "GetComputerNameW" => kernel32::get_computer_name_w,
        "GetUserNameA" => kernel32::get_user_name_a,
        "GetUserNameW" => kernel32::get_user_name_w,
        "GetStartupInfoA" => kernel32::get_startup_info_a,
        "GetStartupInfoW" => kernel32::get_startup_info_w,
        "InitializeCriticalSection" => kernel32::initialize_critical_section,
        "InitializeCriticalSectionAndSpinCount" => kernel32::initialize_critical_section_and_spin_count,
        "InitializeCriticalSectionEx" => kernel32::initialize_critical_section_ex,
        "EnterCriticalSection" => kernel32::enter_critical_section,
        "LeaveCriticalSection" => kernel32::leave_critical_section,
        "TryEnterCriticalSection" => kernel32::try_enter_critical_section,
        "DeleteCriticalSection" => kernel32::delete_critical_section,
        "SetCriticalSectionSpinCount" => kernel32::set_critical_section_spin_count,
        "CreateThread" => kernel32::create_thread,
        "ResumeThread" => kernel32::resume_thread,
        "WaitForSingleObject" => kernel32::wait_for_single_object,
        "WaitForMultipleObjects" => kernel32::wait_for_multiple_objects,
        "GetExitCodeThread" => kernel32::get_exit_code_thread,
        "CreateMutexA" => kernel32::create_mutex_a,
        "CreateMutexW" => kernel32::create_mutex_w,
        "ReleaseMutex" => kernel32::release_mutex,
        "CreateSemaphoreA" => kernel32::create_semaphore_a,
        "CreateSemaphoreW" => kernel32::create_semaphore_w,
        "ReleaseSemaphore" => kernel32::release_semaphore,
        "CreateEventA" => kernel32::create_event_a,
        "CreateEventW" => kernel32::create_event_w,
        "SetEvent" => kernel32::set_event,
        "ResetEvent" => kernel32::reset_event,
        "TlsAlloc" => kernel32::tls_alloc,
        "TlsFree" => kernel32::tls_free,
        "TlsGetValue" => kernel32::tls_get_value,
        "TlsSetValue" => kernel32::tls_set_value,
        "VirtualAlloc" => kernel32::virtual_alloc,
        "VirtualFree" => kernel32::virtual_free,
        "VirtualProtect" => kernel32::virtual_protect,
        "VirtualQuery" => kernel32::virtual_query,
        "GetProcessHeap" => kernel32::get_process_heap,
        "HeapCreate" => kernel32::heap_create,
        "HeapDestroy" => kernel32::heap_destroy,
        "HeapAlloc" => kernel32::heap_alloc,
        "HeapReAlloc" => kernel32::heap_realloc,
        "HeapFree" => kernel32::heap_free,
        "HeapSize" => kernel32::heap_size,
        "HeapValidate" => kernel32::heap_validate,
        "GlobalAlloc" => kernel32::global_alloc,
        "GlobalFree" => kernel32::global_free,
        "GlobalReAlloc" => kernel32::global_realloc,
        "GlobalSize" => kernel32::global_size,
        "LocalAlloc" => kernel32::local_alloc,
        "LocalFree" => kernel32::local_free,
        "LocalReAlloc" => kernel32::local_realloc,
        "LocalSize" => kernel32::local_size,
        "IsDebuggerPresent" => kernel32::is_debugger_present,
        "OutputDebugStringA" => kernel32::output_debug_string_a,
        "OutputDebugStringW" => kernel32::output_debug_string_w,
        "GetACP" => kernel32::get_acp,
        "GetOEMCP" => kernel32::get_oemcp,
        "GetConsoleCP" => kernel32::get_console_cp,
        "GetConsoleOutputCP" => kernel32::get_console_output_cp,
        "SetConsoleCP" => kernel32::set_console_cp,
        "SetConsoleOutputCP" => kernel32::set_console_output_cp,
        "GetConsoleMode" => kernel32::get_console_mode,
        "SetConsoleMode" => kernel32::set_console_mode,
        "SetConsoleTitleA" => kernel32::set_console_title_a,
        "SetConsoleTitleW" => kernel32::set_console_title_w,
        "AllocConsole" => kernel32::alloc_console,
        "FreeConsole" => kernel32::free_console,
        "SetErrorMode" => kernel32::set_error_mode,
        "GetSystemInfo" => kernel32::get_system_info,
        "GetNativeSystemInfo" => kernel32::get_native_system_info,
        "MultiByteToWideChar" => kernel32::multi_byte_to_wide_char,
        "WideCharToMultiByte" => kernel32::wide_char_to_multi_byte,
        "lstrlenA" => kernel32::lstrlen_a,
        "lstrlenW" => kernel32::lstrlen_w,
        "lstrcpyA" => kernel32::lstrcpy_a,
        "lstrcpyW" => kernel32::lstrcpy_w,
        "lstrcatA" => kernel32::lstrcat_a,
        "lstrcatW" => kernel32::lstrcat_w,
        "lstrcmpA" => kernel32::lstrcmp_a,
        "lstrcmpW" => kernel32::lstrcmp_w,
        "lstrcmpiA" => kernel32::lstrcmpi_a,
        "lstrcmpiW" => kernel32::lstrcmpi_w,
        "lstrcpynA" => kernel32::lstrcpyn_a,
        "lstrcpynW" => kernel32::lstrcpyn_w,
    },
    "user32.dll" {
        "MessageBoxA" => user32::message_box_a,
        "MessageBoxW" => user32::message_box_w,
        "RegisterClassA" => user32::register_class_a,
        "RegisterClassW" => user32::register_class_w,
        "RegisterClassExA" => user32::register_class_ex_a,
        "RegisterClassExW" => user32::register_class_ex_w,
        "UnregisterClassA" => user32::unregister_class_a,
        "UnregisterClassW" => user32::unregister_class_w,
        "CreateWindowExA" => user32::create_window_ex_a,
        "CreateWindowExW" => user32::create_window_ex_w,
        "DestroyWindow" => user32::destroy_window,
        "ShowWindow" => user32::show_window,
        "ShowWindowAsync" => user32::show_window_async,
        "UpdateWindow" => user32::update_window,
        "InvalidateRect" => user32::invalidate_rect,
        "ValidateRect" => user32::validate_rect,
        "GetMessageA" => user32::get_message_a,
        "GetMessageW" => user32::get_message_w,
        "PeekMessageA" => user32::peek_message_a,
        "PeekMessageW" => user32::peek_message_w,
        "TranslateMessage" => user32::translate_message,
        "DispatchMessageA" => user32::dispatch_message_a,
        "DispatchMessageW" => user32::dispatch_message_w,
        "PostMessageA" => user32::post_message_a,
        "PostMessageW" => user32::post_message_w,
        "SendMessageA" => user32::send_message_a,
        "SendMessageW" => user32::send_message_w,
        "DefWindowProcA" => user32::def_window_proc_a,
        "DefWindowProcW" => user32::def_window_proc_w,
        "SetTimer" => user32::set_timer,
        "KillTimer" => user32::kill_timer,
        "PostQuitMessage" => user32::post_quit_message,
        "BeginPaint" => user32::begin_paint,
        "EndPaint" => user32::end_paint,
        "GetDC" => user32::get_dc,
        "ReleaseDC" => user32::release_dc,
        "GetClientRect" => user32::get_client_rect,
        "GetWindowRect" => user32::get_window_rect,
        "GetWindowTextA" => user32::get_window_text_a,
        "GetWindowTextW" => user32::get_window_text_w,
        "GetWindowTextLengthA" => user32::get_window_text_length_a,
        "GetWindowTextLengthW" => user32::get_window_text_length_w,
        "SetWindowTextA" => user32::set_window_text_a,
        "SetWindowTextW" => user32::set_window_text_w,
        "GetWindowLongPtrA" => user32::get_window_long_ptr_a,
        "GetWindowLongPtrW" => user32::get_window_long_ptr_w,
        "SetWindowLongPtrA" => user32::set_window_long_ptr_a,
        "SetWindowLongPtrW" => user32::set_window_long_ptr_w,
        "GetWindowLongA" => user32::get_window_long_a,
        "GetWindowLongW" => user32::get_window_long_w,
        "SetWindowLongA" => user32::set_window_long_a,
        "SetWindowLongW" => user32::set_window_long_w,
        "AdjustWindowRect" => user32::adjust_window_rect,
        "AdjustWindowRectEx" => user32::adjust_window_rect_ex,
        "MoveWindow" => user32::move_window,
        "SetWindowPos" => user32::set_window_pos,
        "IsWindow" => user32::is_window,
        "IsWindowVisible" => user32::is_window_visible,
        "LoadCursorA" => user32::load_cursor_a,
        "LoadCursorW" => user32::load_cursor_w,
        "LoadIconA" => user32::load_icon_a,
        "LoadIconW" => user32::load_icon_w,
        "GetFocus" => user32::get_focus,
        "SetFocus" => user32::set_focus,
        "SetCapture" => user32::set_capture,
        "ReleaseCapture" => user32::release_capture,
        "GetKeyState" => user32::get_key_state,
        "GetAsyncKeyState" => user32::get_async_key_state,
        "GetMessagePos" => user32::get_message_pos,
        "GetMessageTime" => user32::get_message_time,
        "GetParent" => user32::get_parent,
        "GetWindow" => user32::get_window,
        "GetDesktopWindow" => user32::get_desktop_window,
        "GetSystemMetrics" => user32::get_system_metrics,
        "GetDoubleClickTime" => user32::get_double_click_time,
        "MessageBeep" => user32::message_beep,
        "SetCursorPos" => user32::set_cursor_pos,
        "GetCursorPos" => user32::get_cursor_pos,
    },
    "gdi32.dll" {
        "TextOutA" => gdi32::text_out_a,
        "TextOutW" => gdi32::text_out_w,
        "SetTextColor" => gdi32::set_text_color,
        "GetTextColor" => gdi32::get_text_color,
        "SetBkColor" => gdi32::set_bk_color,
        "GetBkColor" => gdi32::get_bk_color,
        "SetBkMode" => gdi32::set_bk_mode,
        "GetBkMode" => gdi32::get_bk_mode,
        "CreateSolidBrush" => gdi32::create_solid_brush,
        "DeleteObject" => gdi32::delete_object,
        "SelectObject" => gdi32::select_object,
        "GetStockObject" => gdi32::get_stock_object,
        "FillRect" => gdi32::fill_rect,
        "Rectangle" => gdi32::rectangle,
    },
    "ntdll.dll" {
        "RtlZeroMemory" => ntdll::rtl_zero_memory,
        "RtlFillMemory" => ntdll::rtl_fill_memory,
        "RtlCopyMemory" => ntdll::rtl_copy_memory,
        "RtlMoveMemory" => ntdll::rtl_move_memory,
        "RtlCompareMemory" => ntdll::rtl_compare_memory,
        "RtlSetLastWin32Error" => ntdll::rtl_set_last_win32_error,
        "RtlGetLastWin32Error" => ntdll::rtl_get_last_win32_error,
    },
    "advapi32.dll" {
        "RegOpenKeyExA" => advapi32::reg_open_key_ex_a,
        "RegOpenKeyExW" => advapi32::reg_open_key_ex_w,
        "RegCreateKeyExA" => advapi32::reg_create_key_ex_a,
        "RegCreateKeyExW" => advapi32::reg_create_key_ex_w,
        "RegCloseKey" => advapi32::reg_close_key,
        "RegQueryValueExA" => advapi32::reg_query_value_ex_a,
        "RegQueryValueExW" => advapi32::reg_query_value_ex_w,
        "RegSetValueExA" => advapi32::reg_set_value_ex_a,
        "RegSetValueExW" => advapi32::reg_set_value_ex_w,
        "RegDeleteValueA" => advapi32::reg_delete_value_a,
        "RegDeleteValueW" => advapi32::reg_delete_value_w,
        "RegDeleteKeyA" => advapi32::reg_delete_key_a,
        "RegDeleteKeyW" => advapi32::reg_delete_key_w,
        "RegDeleteKeyExA" => advapi32::reg_delete_key_ex_a,
        "RegDeleteKeyExW" => advapi32::reg_delete_key_ex_w,
        "RegEnumKeyExA" => advapi32::reg_enum_key_ex_a,
        "RegEnumKeyExW" => advapi32::reg_enum_key_ex_w,
        "RegEnumValueA" => advapi32::reg_enum_value_a,
        "RegEnumValueW" => advapi32::reg_enum_value_w,
        "RegQueryInfoKeyA" => advapi32::reg_query_info_key_a,
        "RegQueryInfoKeyW" => advapi32::reg_query_info_key_w,
        "RegGetValueA" => advapi32::reg_get_value_a,
        "RegGetValueW" => advapi32::reg_get_value_w,
        "RegSetKeyValueA" => advapi32::reg_set_key_value_a,
        "RegSetKeyValueW" => advapi32::reg_set_key_value_w,
        "RegFlushKey" => advapi32::reg_flush_key,
        "RegNotifyChangeKeyValue" => advapi32::reg_notify_change_key_value,
    },
    "msvcrt.dll" {
        "malloc" => msvcrt::malloc,
        "calloc" => msvcrt::calloc,
        "realloc" => msvcrt::realloc,
        "free" => msvcrt::free,
        "_aligned_malloc" => msvcrt::aligned_malloc,
        "_aligned_free" => msvcrt::aligned_free,
        "memcpy" => msvcrt::memcpy,
        "memmove" => msvcrt::memmove,
        "memset" => msvcrt::memset,
        "memcmp" => msvcrt::memcmp,
        "strlen" => msvcrt::strlen,
        "strcpy" => msvcrt::strcpy,
        "strncpy" => msvcrt::strncpy,
        "strcat" => msvcrt::strcat,
        "strcmp" => msvcrt::strcmp,
        "strncmp" => msvcrt::strncmp,
        "strchr" => msvcrt::strchr,
        "strrchr" => msvcrt::strrchr,
        "strstr" => msvcrt::strstr,
        "atoi" => msvcrt::atoi,
        "atol" => msvcrt::atol,
        "abs" => msvcrt::abs,
        "labs" => msvcrt::labs,
        "puts" => msvcrt::puts,
        "putchar" => msvcrt::putchar,
        "exit" => msvcrt::exit,
        "_exit" => msvcrt::_exit,
        "abort" => msvcrt::abort,
        "_initterm" => msvcrt::_initterm,
        "_initterm_e" => msvcrt::_initterm_e,
    },
}

pub fn resolve_ordinal(dll: &str, ordinal: u16) -> Option<usize> {
    log::debug!("[n2np-thunk] ordinal import {dll}!#{ordinal} is not mapped");
    None
}

pub unsafe extern "win64" fn missing_thunk() -> usize {
    log::warn!("[n2np-thunk] called an unimplemented Windows API thunk");
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernelbase_maps_to_kernel32() {
        assert_eq!(normalize_dll_name("KERNELBASE.dll"), "kernel32.dll");
        assert_eq!(
            normalize_dll_name("api-ms-win-core-file-l1-1-0.dll"),
            "kernel32.dll"
        );
        assert_eq!(normalize_dll_name("ucrtbase.dll"), "msvcrt.dll");
    }

    #[test]
    fn resolves_implemented_apis() {
        assert!(resolve("kernel32.dll", "ExitProcess").is_some());
        assert!(resolve("KERNELBASE.dll", "GetLastError").is_some());
        assert!(resolve("user32.dll", "MessageBoxA").is_some());
        assert!(resolve("kernel32.dll", "ThisApiDoesNotExist").is_none());
    }

    #[test]
    fn supported_apis_are_sorted_and_unique() {
        let apis = supported_apis();
        assert!(apis.windows(2).all(|pair| pair[0] < pair[1]));
    }
}
