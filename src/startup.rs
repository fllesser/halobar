use std::{env, io, path::PathBuf};
use winreg::{
    RegKey,
    enums::{HKEY_CURRENT_USER, KEY_SET_VALUE},
};

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const VALUE_NAME: &str = "HaloBar";

pub fn install() -> Result<(), String> {
    let executable =
        env::current_exe().map_err(|error| format!("获取 HaloBar 程序路径失败: {error}"))?;
    let command = format!("\"{}\"", executable.to_string_lossy());
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(RUN_KEY, KEY_SET_VALUE)
        .or_else(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                RegKey::predef(HKEY_CURRENT_USER)
                    .create_subkey(RUN_KEY)
                    .map(|(key, _)| key)
            } else {
                Err(error)
            }
        })
        .map_err(|error| format!("打开 Windows 开机启动项失败: {error}"))?;
    key.set_value(VALUE_NAME, &command)
        .map_err(|error| format!("写入 Windows 开机启动项失败: {error}"))
}

pub fn remove() -> Result<(), String> {
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(RUN_KEY, KEY_SET_VALUE)
        .map_err(|error| format!("打开 Windows 开机启动项失败: {error}"))?;
    match key.delete_value(VALUE_NAME) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("删除 Windows 开机启动项失败: {error}")),
    }
}

pub fn log_path() -> Result<PathBuf, String> {
    let local_app_data =
        env::var_os("LOCALAPPDATA").ok_or_else(|| "Windows 未提供 LOCALAPPDATA 路径".to_owned())?;
    Ok(PathBuf::from(local_app_data)
        .join("HaloBar")
        .join("halobar.log"))
}
