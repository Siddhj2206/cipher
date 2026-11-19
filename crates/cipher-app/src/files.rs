use std::path::Path;
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use std::process::Command;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::process::Stdio;

#[derive(Clone, Copy)]
pub enum Show {
    Folder,
    Book,
}

pub fn show(path: &Path, target: Show) -> Result<(), String> {
    let action = match target {
        Show::Folder => "open the folder",
        Show::Book => "show the Book",
    };
    let run = || {
        let path = std::path::absolute(path).map_err(|e| e.to_string())?;
        let folder = match target {
            Show::Folder => {
                std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
                &path
            }
            Show::Book => {
                std::fs::metadata(&path).map_err(|e| e.to_string())?;
                path.parent().ok_or("the Book has no parent folder")?
            }
        };
        show_native(&path, folder, target)
    };
    run().map_err(|e| {
        format!(
            "Could not {action} in the file manager: {}. {e}",
            path.display()
        )
    })
}

#[cfg(target_os = "linux")]
fn show_native(path: &Path, folder: &Path, target: Show) -> Result<(), String> {
    if file_manager_call(path, target).is_ok() {
        return Ok(());
    }
    run(Command::new("xdg-open").arg(folder))
}

#[cfg(target_os = "linux")]
fn file_manager_call(path: &Path, target: Show) -> Result<(), String> {
    use zbus::blocking::connection::Builder;

    let url = file_url(path).ok_or("invalid file path")?;
    let bus = Builder::session()
        .and_then(|b| b.method_timeout(std::time::Duration::from_secs(5)).build())
        .map_err(|e| e.to_string())?;
    let service = "org.freedesktop.FileManager1";
    let method = match target {
        Show::Folder => "ShowFolders",
        Show::Book => "ShowItems",
    };
    bus.call_method(
        Some(service),
        "/org/freedesktop/FileManager1",
        Some(service),
        method,
        &(&[url.as_str()][..], ""),
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

#[cfg(target_os = "linux")]
fn file_url(path: &Path) -> Option<url::Url> {
    url::Url::from_file_path(path).ok()
}

#[cfg(target_os = "macos")]
fn show_native(path: &Path, folder: &Path, target: Show) -> Result<(), String> {
    if matches!(target, Show::Book) && run(Command::new("open").arg("-R").arg(path)).is_ok() {
        return Ok(());
    }
    run(Command::new("open").arg(folder))
}

#[cfg(target_os = "windows")]
fn show_native(path: &Path, folder: &Path, target: Show) -> Result<(), String> {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::os::windows::process::CommandExt;

    let path = match target {
        Show::Book => path,
        Show::Folder => folder,
    };
    let wide: Vec<_> = path.as_os_str().encode_wide().collect();
    let path = explorer_path(&wide).ok_or("Explorer cannot open this kind of path")?;
    let path = OsString::from_wide(&path);
    let mut command = Command::new("explorer.exe");
    match target {
        Show::Book => {
            let mut select = OsString::from("/select,\"");
            select.push(path);
            select.push("\"");
            command.raw_arg(select);
        }
        Show::Folder => {
            command.arg(path);
        }
    }
    let mut explorer = command
        .spawn()
        .map_err(|e| format!("Could not launch Explorer: {e}"))?;
    // Explorer may exit with a nonzero status after handing off to the desktop shell.
    let _ = explorer.wait();
    Ok(())
}

#[cfg(any(target_os = "windows", test))]
fn explorer_path(path: &[u16]) -> Option<Vec<u16>> {
    const SLASH: u16 = b'\\' as u16;
    const COLON: u16 = b':' as u16;
    const VERBATIM: [u16; 4] = [SLASH, SLASH, b'?' as u16, SLASH];
    const UNC: [u16; 4] = [b'U' as u16, b'N' as u16, b'C' as u16, SLASH];
    let Some(rest) = path.strip_prefix(&VERBATIM) else {
        return Some(path.to_vec());
    };
    if let Some(network) = rest.strip_prefix(&UNC) {
        Some([&[SLASH, SLASH], network].concat())
    } else if matches!(rest, [drive, COLON, SLASH, ..] if u8::try_from(*drive).is_ok_and(|b| b.is_ascii_alphabetic()))
    {
        Some(rest.to_vec())
    } else {
        None
    }
}

#[cfg(test)]
#[test]
fn explorer_paths_preserve_drive_and_network_targets() {
    for (input, expected) in [
        (
            r"\\?\C:\My Books\Тихая башня.cipher",
            r"C:\My Books\Тихая башня.cipher",
        ),
        (r"\\?\UNC\server\share\é.cipher", r"\\server\share\é.cipher"),
        (r"D:\Cipher", r"D:\Cipher"),
        (r"\\server\share", r"\\server\share"),
    ] {
        let path: Vec<_> = input.encode_utf16().collect();
        assert_eq!(
            explorer_path(&path),
            Some(expected.encode_utf16().collect())
        );
    }
    let device: Vec<_> = r"\\?\Volume{device}\Cipher".encode_utf16().collect();
    assert_eq!(explorer_path(&device), None);
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn show_native(_: &Path, _: &Path, _: Show) -> Result<(), String> {
    Err("No file manager integration for this platform".into())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn run(command: &mut Command) -> Result<(), String> {
    let name = command.get_program().to_string_lossy().into_owned();
    let output = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .output()
        .map_err(|e| format!("Could not launch {name}: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail = stderr.trim();
    if detail.is_empty() {
        Err(format!("{name} failed ({})", output.status))
    } else {
        Err(format!("{name}: {detail}"))
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn file_url_round_trips_a_book_path() {
        let path = Path::new("/tmp/My Books é #/Тихая башня 100%.cipher");
        let url = file_url(path).unwrap();
        assert_eq!(
            url.as_str(),
            "file:///tmp/My%20Books%20%C3%A9%20%23/%D0%A2%D0%B8%D1%85%D0%B0%D1%8F%20%D0%B1%D0%B0%D1%88%D0%BD%D1%8F%20100%25.cipher"
        );
        assert_eq!(url.to_file_path().unwrap(), path);
    }

    #[test]
    fn reports_file_manager_failure() {
        let error =
            run(Command::new("/bin/sh").args(["-c", "echo No handler >&2; exit 1"])).unwrap_err();
        assert_eq!(error, "/bin/sh: No handler");
    }
}
