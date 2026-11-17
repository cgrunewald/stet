//! The few things that differ per OS: clipboard, config location, dialogs.

use std::path::PathBuf;
#[cfg(not(target_os = "ios"))]
use stet_core::{Clip, Clipboard};

/// The system clipboard. Opened lazily: some platforms block on first use.
#[cfg(not(target_os = "ios"))]
#[derive(Default)]
pub struct SystemClipboard(Option<arboard::Clipboard>);

#[cfg(not(target_os = "ios"))]
impl SystemClipboard {
    fn handle(&mut self) -> Option<&mut arboard::Clipboard> {
        if self.0.is_none() {
            self.0 = arboard::Clipboard::new().ok();
        }
        self.0.as_mut()
    }
}

#[cfg(not(target_os = "ios"))]
impl Clipboard for SystemClipboard {
    fn get(&mut self) -> Option<String> {
        self.handle()?.get_text().ok()
    }
    fn set(&mut self, text: &str) {
        if let Some(clipboard) = self.handle() {
            let _ = clipboard.set_text(text.to_string());
        }
    }
    fn contents(&mut self) -> Clip {
        let Some(clipboard) = self.handle() else {
            return Clip::default();
        };
        Clip {
            text: clipboard.get_text().ok(),
            html: clipboard.get().html().ok(),
            files: clipboard.get().file_list().unwrap_or_default(),
        }
    }
    fn image(&mut self) -> Option<Vec<u8>> {
        use image::ImageEncoder;
        use image::codecs::png::{CompressionType, FilterType, PngEncoder};
        let picture = self.handle()?.get_image().ok()?;
        let mut png = Vec::new();
        // Fast, not small: a full-screen capture should paste without a pause.
        PngEncoder::new_with_quality(&mut png, CompressionType::Fast, FilterType::Adaptive)
            .write_image(
                &picture.bytes,
                picture.width as u32,
                picture.height as u32,
                image::ExtendedColorType::Rgba8,
            )
            .ok()?;
        Some(png)
    }
    fn set_rich(&mut self, text: &str, html: &str) {
        if let Some(clipboard) = self.handle() {
            let _ = clipboard.set_html(html.to_string(), Some(text.to_string()));
        }
    }
}

#[cfg(target_os = "ios")]
pub use crate::ios::Pasteboard as SystemClipboard;

/// `~/.config/stet` on Linux and macOS, `%APPDATA%\stet` on Windows, and
/// the app's own Application Support folder on iOS.
pub fn config_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("STET_CONFIG_DIR") {
        return Some(PathBuf::from(dir));
    }
    #[cfg(target_os = "ios")]
    return crate::ios::support_dir();
    #[cfg(not(target_os = "ios"))]
    {
        let dirs = directories::BaseDirs::new()?;
        Some(if cfg!(windows) {
            dirs.config_dir().join("stet")
        } else {
            dirs.home_dir().join(".config").join("stet")
        })
    }
}

/// Opens a link, or a file stet does not show itself, in the app the system
/// keeps for it.
pub fn open_url(target: &str) -> Result<(), String> {
    #[cfg(target_os = "ios")]
    return crate::ios::open_url(target);
    #[cfg(not(target_os = "ios"))]
    open::that_detached(target).map_err(|err| err.to_string())
}

#[derive(PartialEq, Eq)]
#[cfg_attr(target_os = "ios", allow(dead_code))]
pub enum Unsaved {
    Save,
    Discard,
    Cancel,
}

#[cfg(not(target_os = "ios"))]
mod dialogs {
    use super::Unsaved;
    use std::path::PathBuf;

    pub fn pick_file() -> Option<PathBuf> {
        rfd::FileDialog::new()
            .add_filter("Markdown", &["md", "markdown", "mdown", "txt"])
            .add_filter("All files", &["*"])
            .pick_file()
    }

    pub fn pick_save(name: &str) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_file_name(if name == "Untitled" { "Untitled.md" } else { name })
            .save_file()
    }

    pub fn confirm_unsaved(name: &str) -> Unsaved {
        let answer = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Warning)
            .set_title("Unsaved changes")
            .set_description(format!("Save changes to {name} before closing?"))
            .set_buttons(rfd::MessageButtons::YesNoCancel)
            .show();
        match answer {
            rfd::MessageDialogResult::Yes => Unsaved::Save,
            rfd::MessageDialogResult::No => Unsaved::Discard,
            _ => Unsaved::Cancel,
        }
    }

    /// True if the user would write over a file that changed on disk.
    pub fn confirm_overwrite(name: &str) -> bool {
        let answer = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Warning)
            .set_title("File changed on disk")
            .set_description(format!(
                "{name} was changed by something else since you opened it. Save your version over it?"
            ))
            .set_buttons(rfd::MessageButtons::OkCancelCustom(
                "Save Over It".into(),
                "Cancel".into(),
            ))
            .show();
        answer == rfd::MessageDialogResult::Custom("Save Over It".into())
    }
}

/// iOS has no dialogs to stop and ask with. Documents live in the app's
/// folder, which the file browser shows; closing one keeps what was written.
#[cfg(target_os = "ios")]
mod dialogs {
    use super::Unsaved;
    use std::path::PathBuf;

    /// Nothing to pick with: the shell shows the file browser instead.
    pub fn pick_file() -> Option<PathBuf> {
        None
    }

    /// A free name in the app's folder: `Untitled.md`, `Untitled 2.md`, …
    pub fn pick_save(name: &str) -> Option<PathBuf> {
        let dir = crate::ios::documents_dir()?;
        let name = if name == "Untitled" { "Untitled.md" } else { name };
        let (stem, extension) = name.rsplit_once('.').unwrap_or((name, "md"));
        (1..1000)
            .map(|count| match count {
                1 => dir.join(format!("{stem}.{extension}")),
                _ => dir.join(format!("{stem} {count}.{extension}")),
            })
            .find(|path| !path.exists())
    }

    pub fn confirm_unsaved(_: &str) -> Unsaved {
        Unsaved::Save
    }

    /// Never over someone else's change without being told to: `:w!` says so.
    pub fn confirm_overwrite(_: &str) -> bool {
        false
    }
}

pub use dialogs::{confirm_overwrite, confirm_unsaved, pick_file, pick_save};

/// Downloaded images are kept here between runs.
pub fn cache_dir() -> Option<PathBuf> {
    if std::env::var_os("STET_CONFIG_DIR").is_some() {
        return config_dir().map(|dir| dir.join("cache"));
    }
    #[cfg(target_os = "ios")]
    return crate::ios::cache_dir();
    #[cfg(not(target_os = "ios"))]
    Some(directories::BaseDirs::new()?.cache_dir().join("stet"))
}

/// Choices made while running (as opposed to written in `config.toml`),
/// remembered for the next launch.
#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct State {
    pub theme: Option<String>,
    pub zoom: Option<f32>,
    pub prose_font: Option<String>,
    pub mono_font: Option<String>,
    /// Window size in points.
    pub window: Option<(f32, f32)>,
    /// The Substack publication last published to.
    pub substack: Option<String>,
    /// Who sees uploaded videos: `unlisted`, `public` or `private`.
    pub youtube_privacy: Option<String>,
    /// Whether publishing uploads local videos to YouTube (`youtube`, the
    /// default) or leaves them as links to the file (`link`).
    pub videos: Option<String>,
    /// The document that was open, where launching opens no file by name.
    pub last: Option<PathBuf>,
}

impl State {
    fn path() -> Option<PathBuf> {
        Some(config_dir()?.join("state.toml"))
    }

    pub fn load() -> State {
        Self::path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|source| toml::from_str(&source).ok())
            .unwrap_or_default()
    }

    /// True if `config.toml` was edited after the state was written: the
    /// file is then the newer word on anything both set.
    pub fn older_than_config() -> bool {
        let modified = |name: &str| {
            std::fs::metadata(config_dir()?.join(name))
                .and_then(|meta| meta.modified())
                .ok()
        };
        match (modified("state.toml"), modified("config.toml")) {
            (Some(state), Some(config)) => state < config,
            _ => false,
        }
    }

    pub fn save(&self) {
        let Some(path) = Self::path() else { return };
        if let (Some(dir), Ok(source)) = (path.parent(), toml::to_string(self)) {
            let _ = std::fs::create_dir_all(dir);
            let _ = std::fs::write(path, source);
        }
    }
}

/// How the open document is written down to come back to: within the app's
/// own folder by its path from there, since the folder itself is somewhere
/// else after every update. Only iOS, where no launch names a file, keeps one.
pub fn remembered(path: Option<&std::path::Path>) -> Option<PathBuf> {
    #[cfg(target_os = "ios")]
    {
        let home = crate::ios::documents_dir()?;
        let path = path?;
        Some(path.strip_prefix(&home).unwrap_or(path).to_path_buf())
    }
    #[cfg(not(target_os = "ios"))]
    {
        let _ = path;
        None
    }
}

/// The document `remembered` wrote down, if it is still there.
pub fn recalled(last: Option<&std::path::Path>) -> Option<PathBuf> {
    #[cfg(target_os = "ios")]
    {
        let path = crate::ios::documents_dir()?.join(last?);
        path.exists().then_some(path)
    }
    #[cfg(not(target_os = "ios"))]
    {
        let _ = last;
        None
    }
}

/// Where a running stet listens for files to open.
#[cfg_attr(target_os = "ios", allow(dead_code))]
pub fn socket_path() -> Option<PathBuf> {
    let socket = config_dir()?.join("stet.sock");
    // Socket addresses are short (about 100 bytes); a deep config folder
    // gets a stand-in under the temp folder instead.
    if socket.as_os_str().len() < 96 {
        return Some(socket);
    }
    let hash = socket
        .to_string_lossy()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
            (hash ^ byte as u64).wrapping_mul(0x0100_0000_01b3)
        });
    Some(std::env::temp_dir().join(format!("stet-{hash:016x}.sock")))
}

/// Sends one request line to the stet that is already running and returns
/// its reply. `None` if none is running.
#[cfg(all(unix, not(target_os = "ios")))]
pub fn request(line: &str, patient: bool) -> Option<String> {
    use std::io::{BufRead, BufReader, Write};
    let mut stream = std::os::unix::net::UnixStream::connect(socket_path()?).ok()?;
    // `patient` waits as long as it takes (an assistant waiting for a message).
    let _ = stream.set_read_timeout((!patient).then(|| std::time::Duration::from_secs(10)));
    stream.write_all(line.replace('\n', " ").as_bytes()).ok()?;
    stream.write_all(b"\n").ok()?;
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply).ok()?;
    (!reply.is_empty()).then_some(reply)
}

#[cfg(any(not(unix), target_os = "ios"))]
pub fn request(_: &str, _: bool) -> Option<String> {
    None
}

/// Answers requests from later `stet` launches and from `stet ctl`, one line in
/// and one line out, calling `answer` for each with a way to ask whether
/// the caller is still there. False if another stet is already listening.
#[cfg(all(unix, not(target_os = "ios")))]
pub fn listen(answer: impl Fn(String, &dyn Fn() -> bool) -> String + Send + Sync + 'static) -> bool {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    let Some(socket) = socket_path() else { return false };
    if UnixStream::connect(&socket).is_ok() {
        return false;
    }
    // Whatever is there was left by an stet that is gone.
    let _ = std::fs::remove_file(&socket);
    if let Some(dir) = socket.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let Ok(listener) = UnixListener::bind(&socket) else {
        return false;
    };
    // Only this user may connect.
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600));
    }
    let answer = std::sync::Arc::new(answer);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let answer = answer.clone();
            // One thread per caller: a slow one cannot hold up the rest.
            std::thread::spawn(move || {
                use std::io::Read;
                let mut line = String::new();
                let (Ok(mut reply_to), Ok(probe)) = (stream.try_clone(), stream.try_clone()) else {
                    return;
                };
                if BufReader::new(stream).read_line(&mut line).is_ok() && !line.trim().is_empty() {
                    // The caller sends nothing more, so a read that ends means it hung up.
                    let _ = probe.set_nonblocking(true);
                    let alive = move || !matches!((&probe).read(&mut [0u8; 1]), Ok(0));
                    let reply = answer(line, &alive);
                    let _ = reply_to.set_nonblocking(false);
                    let _ = reply_to.write_all(reply.replace('\n', " ").as_bytes());
                    let _ = reply_to.write_all(b"\n");
                }
            });
        }
    });
    true
}

#[cfg(any(not(unix), target_os = "ios"))]
pub fn listen(_: impl Fn(String, &dyn Fn() -> bool) -> String + Send + Sync + 'static) -> bool {
    false
}

/// Removes the listening socket if this process owns it.
#[cfg_attr(target_os = "ios", allow(dead_code))]
pub fn stop_listening(owned: bool) {
    if let (true, Some(socket)) = (owned, socket_path()) {
        let _ = std::fs::remove_file(socket);
    }
}
