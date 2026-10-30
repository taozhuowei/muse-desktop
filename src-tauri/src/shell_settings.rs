use serde::Serialize;
use tauri::{AppHandle, Webview};

#[cfg(target_os = "windows")]
mod windows {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW,
        RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE,
        REG_OPTION_NON_VOLATILE, REG_SZ,
    };

    const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    const VALUE_NAME: &str = "muse-desktop";

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(Some(0)).collect()
    }

    fn error(code: u32) -> String {
        std::io::Error::from_raw_os_error(code as i32).to_string()
    }

    fn startup_command() -> Result<Vec<u16>, String> {
        let path = std::env::current_exe().map_err(|e| e.to_string())?;
        let mut command = vec!['"' as u16];
        command.extend(path.as_os_str().encode_wide());
        command.extend(['"' as u16, 0]);
        Ok(command)
    }

    struct Key(HKEY);
    impl Drop for Key {
        fn drop(&mut self) {
            unsafe {
                RegCloseKey(self.0);
            }
        }
    }

    pub fn get() -> Result<bool, String> {
        let mut handle = null_mut();
        let status = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                wide(RUN_KEY).as_ptr(),
                0,
                KEY_QUERY_VALUE,
                &mut handle,
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(false);
        }
        if status != ERROR_SUCCESS {
            return Err(error(status));
        }
        let key = Key(handle);
        let name = wide(VALUE_NAME);
        let mut value_type = 0;
        let mut size = 0;
        let status = unsafe {
            RegQueryValueExW(
                key.0,
                name.as_ptr(),
                null(),
                &mut value_type,
                null_mut(),
                &mut size,
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(false);
        }
        if status != ERROR_SUCCESS {
            return Err(error(status));
        }
        if value_type != REG_SZ || size % 2 != 0 {
            return Ok(false);
        }
        let mut value = vec![0u16; (size as usize) / 2];
        let status = unsafe {
            RegQueryValueExW(
                key.0,
                name.as_ptr(),
                null(),
                &mut value_type,
                value.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(false);
        }
        if status != ERROR_SUCCESS {
            return Err(error(status));
        }
        if value_type != REG_SZ || size % 2 != 0 {
            return Ok(false);
        }
        value.truncate((size as usize) / 2);
        Ok(value == startup_command()?)
    }

    pub fn set(enabled: bool) -> Result<(), String> {
        let mut handle = null_mut();
        let status = unsafe {
            if enabled {
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    wide(RUN_KEY).as_ptr(),
                    0,
                    null(),
                    REG_OPTION_NON_VOLATILE,
                    KEY_SET_VALUE,
                    null(),
                    &mut handle,
                    null_mut(),
                )
            } else {
                RegOpenKeyExW(
                    HKEY_CURRENT_USER,
                    wide(RUN_KEY).as_ptr(),
                    0,
                    KEY_SET_VALUE,
                    &mut handle,
                )
            }
        };
        if !enabled && status == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        if status != ERROR_SUCCESS {
            return Err(error(status));
        }
        let key = Key(handle);
        let name = wide(VALUE_NAME);
        let status = if enabled {
            let command = startup_command()?;
            let bytes = u32::try_from(command.len() * std::mem::size_of::<u16>())
                .map_err(|e| e.to_string())?;
            unsafe {
                RegSetValueExW(
                    key.0,
                    name.as_ptr(),
                    0,
                    REG_SZ,
                    command.as_ptr().cast(),
                    bytes,
                )
            }
        } else {
            unsafe { RegDeleteValueW(key.0, name.as_ptr()) }
        };
        if status == ERROR_SUCCESS || (!enabled && status == ERROR_FILE_NOT_FOUND) {
            Ok(())
        } else {
            Err(error(status))
        }
    }
}

fn authorize(webview: &Webview) -> Result<(), String> {
    if webview.label() == "shell" {
        Ok(())
    } else {
        Err("只有本地设置页面可以访问开机自启设置".into())
    }
}

#[derive(Serialize)]
pub struct ShellInfo {
    version: String,
    autostart: bool,
    update_configured: bool,
}

#[tauri::command]
pub fn get_shell_info(webview: Webview, app: AppHandle) -> Result<ShellInfo, String> {
    authorize(&webview)?;
    #[cfg(target_os = "windows")]
    let autostart = windows::get()?;
    #[cfg(not(target_os = "windows"))]
    let autostart = return Err("开机自启仅支持 Windows".into());
    Ok(ShellInfo {
        version: app.package_info().version.to_string(),
        autostart,
        update_configured: true,
    })
}

#[tauri::command]
pub fn set_autostart(webview: Webview, enabled: bool) -> Result<bool, String> {
    authorize(&webview)?;
    #[cfg(target_os = "windows")]
    {
        windows::set(enabled)?;
        windows::get()
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = enabled;
        Err("开机自启仅支持 Windows".into())
    }
}
