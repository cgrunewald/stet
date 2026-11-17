//! What makes stet an iPhone and iPad app rather than a desktop program on a
//! small screen: the pasteboard, links, files handed over by other apps, the
//! on-screen keyboard's place, and the keys of a real keyboard that the
//! windowing library does not pass on.

use objc2::declare::ClassBuilder;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, NSObject, Sel};
use objc2::{ClassType, class, msg_send, msg_send_id, sel};
use objc2_foundation::{CGRect, NSArray, NSData, NSString, NSURL};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use stet_core::{Clip, Clipboard, Key, KeyEvent, Mods};

#[link(name = "UIKit", kind = "framework")]
unsafe extern "C" {
    fn UIAccessibilityIsReduceMotionEnabled() -> bool;
    fn UIImagePNGRepresentation(image: *mut AnyObject) -> *mut NSData;
}

/// What the system reports to the event loop.
pub enum AppEvent {
    /// Another app, or Files, handed these documents over.
    Open(Vec<PathBuf>),
    /// The on-screen keyboard came, went or changed size.
    Keyboard,
    /// A key on an attached keyboard that types no text went down, or
    /// (`None`) every key came back up.
    Key(Option<KeyEvent>),
}

static SINK: OnceLock<Box<dyn Fn(AppEvent) + Send + Sync>> = OnceLock::new();
/// Where the on-screen keyboard will rest, in screen coordinates.
static KEYBOARD: Mutex<Option<CGRect>> = Mutex::new(None);

/// The window scene stet is shown in, once the system has connected it, and
/// the windows made before it was, waiting for it. Pointers, as addresses:
/// both live for the life of the app and are only touched on the main thread.
struct Scenes {
    scene: Option<usize>,
    waiting: Vec<usize>,
}

static SCENES: Mutex<Scenes> = Mutex::new(Scenes {
    scene: None,
    waiting: Vec::new(),
});

/// Puts `window` in `scene`, sized to it, and shows it.
///
/// # Safety
/// Both are live: a UIWindow and a UIWindowScene, on the main thread.
unsafe fn place(window: *mut AnyObject, scene: *mut AnyObject) {
    // SAFETY: plain setters and accessors on the live objects the caller
    // vouches for.
    unsafe {
        let _: () = msg_send![window, setWindowScene: scene];
        let space: *mut AnyObject = msg_send![scene, coordinateSpace];
        let bounds: CGRect = msg_send![space, bounds];
        let _: () = msg_send![window, setFrame: bounds];
        let _: () = msg_send![window, makeKeyAndVisible];
    }
}

/// Shows the window `view` belongs to in stet's scene: now, if the system has
/// connected it, or as soon as it does. Since iOS 27 an app's windows live in
/// a scene, and the windowing library makes its window before there is one.
pub fn adopt(view: *mut std::ffi::c_void) {
    let view = view.cast::<AnyObject>();
    if view.is_null() {
        return;
    }
    // SAFETY: `view` is the live UIView of the window just made.
    let window: *mut AnyObject = unsafe { msg_send![view, window] };
    if window.is_null() {
        return;
    }
    let mut scenes = SCENES.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    match scenes.scene {
        // SAFETY: the scene is kept for the life of the app; both are live.
        Some(scene) => unsafe { place(window, scene as *mut AnyObject) },
        None => scenes.waiting.push(window as usize),
    }
}

/// Sends on the documents in a set of UIOpenURLContext.
///
/// # Safety
/// `contexts` is nil or a live NSSet of UIOpenURLContext.
unsafe fn open_contexts(contexts: *mut AnyObject) {
    if contexts.is_null() {
        return;
    }
    let mut files = Vec::new();
    // SAFETY: each context's `URL` is a live NSURL; see `handed_over`.
    unsafe {
        let all: Retained<NSArray<AnyObject>> = msg_send_id![contexts, allObjects];
        for context in all.iter() {
            let url: *mut NSURL = msg_send![context, URL];
            let Some(url) = url.as_ref() else { continue };
            let _: bool = msg_send![url, startAccessingSecurityScopedResource];
            if let Some(path) = url.path() {
                files.push(PathBuf::from(path.to_string()));
            }
        }
    }
    if !files.is_empty() {
        send(AppEvent::Open(files));
    }
}

extern "C" fn scene_connected(
    _this: &AnyObject,
    _cmd: Sel,
    scene: *mut AnyObject,
    _session: *mut AnyObject,
    options: *mut AnyObject,
) {
    let mut scenes = SCENES.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    // SAFETY: the system keeps the scene for as long as it is connected; it
    // is retained here so the address stays good for the life of the app.
    std::mem::forget(unsafe { Retained::retain(scene) });
    scenes.scene = Some(scene as usize);
    for window in std::mem::take(&mut scenes.waiting) {
        // SAFETY: windows the windowing library made and keeps; the scene
        // was just handed over.
        unsafe { place(window as *mut AnyObject, scene) };
    }
    drop(scenes);
    // SAFETY: connection options are live; their URL contexts are a set.
    unsafe {
        if !options.is_null() {
            let contexts: *mut AnyObject = msg_send![options, URLContexts];
            open_contexts(contexts);
        }
    }
}

extern "C" fn scene_opened(_this: &AnyObject, _cmd: Sel, _scene: *mut AnyObject, contexts: *mut AnyObject) {
    // SAFETY: the system passes a live set of UIOpenURLContext.
    unsafe { open_contexts(contexts) };
}

/// The class Info.plist names as the scene delegate. Registered before
/// `UIApplicationMain`, which asks for it while connecting the first scene.
pub fn register_scene_delegate() {
    let Some(mut builder) = ClassBuilder::new("StetSceneDelegate", class!(UIResponder)) else {
        return;
    };
    if let Some(protocol) = objc2::runtime::AnyProtocol::get("UIWindowSceneDelegate") {
        builder.add_protocol(protocol);
    }
    // SAFETY: each function has the signature its selector is sent with.
    unsafe {
        builder.add_method(
            sel!(scene:willConnectToSession:options:),
            scene_connected as extern "C" fn(_, _, _, _, _),
        );
        builder.add_method(sel!(scene:openURLContexts:), scene_opened as extern "C" fn(_, _, _, _));
    }
    builder.register();
}

fn send(event: AppEvent) {
    if let Some(sink) = SINK.get() {
        sink(event);
    }
}

fn string(text: &str) -> Retained<NSString> {
    NSString::from_str(text)
}

extern "C" fn handed_over(
    _this: &AnyObject,
    _cmd: Sel,
    _app: &AnyObject,
    url: &NSURL,
    _options: *mut AnyObject,
) -> Bool {
    // A document opened where it lives (Files, another app's folder) is
    // only readable while this is held; it is held for as long as stet runs.
    // SAFETY: `url` is a live NSURL; the method takes no arguments and returns a BOOL.
    let _: bool = unsafe { msg_send![url, startAccessingSecurityScopedResource] };
    // SAFETY: `path` is a plain accessor on a live NSURL.
    match unsafe { url.path() } {
        Some(path) => {
            send(AppEvent::Open(vec![PathBuf::from(path.to_string())]));
            Bool::YES
        }
        None => Bool::NO,
    }
}

extern "C" fn keyboard_changed(_this: &AnyObject, _cmd: Sel, note: &AnyObject) {
    // SAFETY: a keyboard notification's `userInfo` is a dictionary whose
    // frame entry is an NSValue holding a CGRect; a missing one is nil,
    // which is checked before use.
    unsafe {
        let info: *mut AnyObject = msg_send![note, userInfo];
        if info.is_null() {
            return;
        }
        let value: *mut AnyObject = msg_send![info, objectForKey: &*string("UIKeyboardFrameEndUserInfoKey")];
        if value.is_null() {
            return;
        }
        let frame: CGRect = msg_send![value, CGRectValue];
        *KEYBOARD.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(frame);
    }
    send(AppEvent::Keyboard);
}

/// The key a press stands for, if it is one stet takes itself: keys that
/// type nothing, and anything held with ⌘ or Control. Everything else goes
/// on to the text system and arrives as text.
fn translate(key: &AnyObject) -> Option<KeyEvent> {
    // SAFETY: `key` is a UIKey; these are its plain accessors, returning an
    // integer, an option set and a string.
    let (code, flags, characters) = unsafe {
        let code: isize = msg_send![key, keyCode];
        let flags: isize = msg_send![key, modifierFlags];
        let characters: Retained<NSString> = msg_send_id![key, charactersIgnoringModifiers];
        (code, flags, characters.to_string())
    };
    let mods = Mods {
        shift: flags & (1 << 17) != 0,
        ctrl: flags & (1 << 18) != 0,
        alt: flags & (1 << 19) != 0,
        sup: flags & (1 << 20) != 0,
    };
    let chord = mods.ctrl || mods.sup;
    // Keyboard usage codes, as on the key caps of any USB keyboard.
    let key = match code {
        0x29 => Key::Esc,
        0x2B => Key::Tab,
        0x4F => Key::Right,
        0x50 => Key::Left,
        0x51 => Key::Down,
        0x52 => Key::Up,
        0x4A => Key::Home,
        0x4D => Key::End,
        0x4B => Key::PageUp,
        0x4E => Key::PageDown,
        0x4C => Key::Delete,
        // Plain, these two reach the text system like any typing.
        0x28 | 0x58 if chord || mods.alt => Key::Enter,
        0x2A if chord || mods.alt => Key::Backspace,
        _ if chord => Key::Char(characters.chars().next().filter(|c| !c.is_control())?),
        _ => return None,
    };
    Some(KeyEvent { key, mods })
}

/// The key a press stands for when nothing is taking text: every key is
/// one, as on a desktop.
fn translate_any(key: &AnyObject) -> Option<KeyEvent> {
    if let Some(event) = translate(key) {
        return Some(event);
    }
    // SAFETY: `key` is a UIKey; plain accessors, as in `translate`.
    let (code, flags, characters) = unsafe {
        let code: isize = msg_send![key, keyCode];
        let flags: isize = msg_send![key, modifierFlags];
        let characters: Retained<NSString> = msg_send_id![key, characters];
        (code, flags, characters.to_string())
    };
    let key = match code {
        0x28 | 0x58 => Key::Enter,
        0x2A => Key::Backspace,
        _ => Key::Char(characters.chars().next().filter(|c| !c.is_control())?),
    };
    let mods = Mods {
        shift: flags & (1 << 17) != 0,
        alt: flags & (1 << 19) != 0,
        ..Mods::default()
    };
    Some(KeyEvent { key, mods })
}

thread_local! {
    /// The view is handing a press on for the text system to type; the
    /// window, next to hear of it, leaves it alone.
    static TYPING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Sends on the keys among `presses` that `translate` makes something of.
/// True if any were.
fn take(presses: &AnyObject, translate: fn(&AnyObject) -> Option<KeyEvent>) -> bool {
    let mut taken = false;
    // SAFETY: `presses` is the NSSet of UIPress the system passes; a press's
    // `key` is nil for anything that is not a keyboard, which is skipped.
    unsafe {
        let all: Retained<NSArray<AnyObject>> = msg_send_id![presses, allObjects];
        for press in all.iter() {
            let key: *mut AnyObject = msg_send![press, key];
            if let Some(event) = key.as_ref().and_then(translate) {
                send(AppEvent::Key(Some(event)));
                taken = true;
            }
        }
    }
    taken
}

/// The view has the keyboard while text is being typed: it takes the keys
/// that type nothing and lets the rest become text.
extern "C" fn view_presses_began(this: &AnyObject, _cmd: Sel, presses: &AnyObject, event: *mut AnyObject) {
    if !take(presses, translate) {
        TYPING.set(true);
        // SAFETY: the same message, with the arguments it came with, to the
        // class the view inherits it from.
        let _: () = unsafe { msg_send![super(this, class!(UIView)), pressesBegan: presses, withEvent: event] };
        TYPING.set(false);
    }
}

/// The window hears of keys when nothing is being typed (a document being
/// read, vim's normal mode): then each is a key in its own right.
extern "C" fn window_presses_began(this: &AnyObject, _cmd: Sel, presses: &AnyObject, event: *mut AnyObject) {
    if TYPING.get() || !take(presses, translate_any) {
        // SAFETY: as above, to the class the window inherits it from.
        let _: () = unsafe { msg_send![super(this, class!(UIWindow)), pressesBegan: presses, withEvent: event] };
    }
}

extern "C" fn view_presses_ended(this: &AnyObject, cmd: Sel, presses: &AnyObject, event: *mut AnyObject) {
    send(AppEvent::Key(None));
    // SAFETY: the message as it came, to the inherited method. Ended and
    // cancelled share a signature.
    unsafe {
        if cmd == sel!(pressesCancelled:withEvent:) {
            let _: () = msg_send![super(this, class!(UIView)), pressesCancelled: presses, withEvent: event];
        } else {
            let _: () = msg_send![super(this, class!(UIView)), pressesEnded: presses, withEvent: event];
        }
    }
}

extern "C" fn window_presses_ended(this: &AnyObject, cmd: Sel, presses: &AnyObject, event: *mut AnyObject) {
    send(AppEvent::Key(None));
    // SAFETY: as above.
    unsafe {
        if cmd == sel!(pressesCancelled:withEvent:) {
            let _: () = msg_send![super(this, class!(UIWindow)), pressesCancelled: presses, withEvent: event];
        } else {
            let _: () = msg_send![super(this, class!(UIWindow)), pressesEnded: presses, withEvent: event];
        }
    }
}

/// Starts reporting to `sink`. Call once the application object exists,
/// which is as soon as the event loop first runs.
pub fn install(sink: impl Fn(AppEvent) + Send + Sync + 'static) {
    if SINK.set(Box::new(sink)).is_err() {
        return;
    }
    // The windowing library leaves the application without a delegate; this
    // one takes the documents other apps hand over and watches the keyboard.
    let Some(mut builder) = ClassBuilder::new("StetAppDelegate", NSObject::class()) else {
        return;
    };
    // SAFETY: each function has the signature its selector is sent with.
    unsafe {
        builder.add_method(
            sel!(application:openURL:options:),
            handed_over as extern "C" fn(_, _, _, _, _) -> _,
        );
        builder.add_method(sel!(stetKeyboardChanged:), keyboard_changed as extern "C" fn(_, _, _));
    }
    let delegate_class = builder.register();
    // SAFETY: a plain NSObject subclass, made and never released: the
    // application and the notification center both hold it unretained.
    // The shared application exists once the event loop is running.
    unsafe {
        let delegate: *mut AnyObject = msg_send![delegate_class, new];
        let app: *mut AnyObject = msg_send![class!(UIApplication), sharedApplication];
        if !app.is_null() {
            let _: () = msg_send![app, setDelegate: delegate];
        }
        let center: *mut AnyObject = msg_send![class!(NSNotificationCenter), defaultCenter];
        for name in [
            "UIKeyboardWillChangeFrameNotification",
            "UIKeyboardWillHideNotification",
        ] {
            let _: () = msg_send![
                center,
                addObserver: delegate,
                selector: sel!(stetKeyboardChanged:),
                name: &*string(name),
                object: std::ptr::null::<AnyObject>(),
            ];
        }
    }
    // The windowing library's view hears about typed text only. These give
    // it, and its window, the rest of a real keyboard: Escape, the arrows,
    // ⌘ and Control, and every key while nothing is being typed.
    type Presses = extern "C" fn(&AnyObject, Sel, &AnyObject, *mut AnyObject);
    let methods: [(&str, Presses, Presses); 2] = [
        ("WinitUIView", view_presses_began, view_presses_ended),
        ("WinitUIWindow", window_presses_began, window_presses_ended),
    ];
    for (name, began, ended) in methods {
        let Some(class) = AnyClass::get(name) else { continue };
        let class = class as *const AnyClass as *mut objc2::ffi::objc_class;
        // SAFETY: each function matches the encoding given (`v@:@@`), and
        // the class exists for the life of the process.
        unsafe {
            for (selector, method) in [
                (sel!(pressesBegan:withEvent:), began),
                (sel!(pressesEnded:withEvent:), ended),
                (sel!(pressesCancelled:withEvent:), ended),
            ] {
                let method: unsafe extern "C" fn() = std::mem::transmute(method);
                objc2::ffi::class_addMethod(class, selector.as_ptr(), Some(method), c"v@:@@".as_ptr());
            }
        }
    }
}

/// How much of the bottom of the window the on-screen keyboard covers, in
/// points. `view` is the window's `UIView`.
pub fn keyboard_overlap(view: *mut std::ffi::c_void) -> f32 {
    let Some(frame) = *KEYBOARD.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) else {
        return 0.0;
    };
    let view = view.cast::<AnyObject>();
    if view.is_null() || frame.size.height <= 0.0 {
        return 0.0;
    }
    // SAFETY: `view` is the live UIView the window handle names; its window
    // and that window's screen are asked for with plain accessors, and nil
    // answers are checked before use.
    unsafe {
        let window: *mut AnyObject = msg_send![view, window];
        if window.is_null() {
            return 0.0;
        }
        let screen: *mut AnyObject = msg_send![window, screen];
        let space: *mut AnyObject = msg_send![screen, coordinateSpace];
        let local: CGRect = msg_send![window, convertRect: frame, fromCoordinateSpace: space];
        let bounds: CGRect = msg_send![window, bounds];
        (bounds.size.height - local.origin.y).clamp(0.0, bounds.size.height) as f32
    }
}

/// Settings → Accessibility → Motion → Reduce Motion.
pub fn reduce_motion() -> bool {
    // SAFETY: takes nothing, returns a BOOL, callable from any thread.
    unsafe { UIAccessibilityIsReduceMotionEnabled() }
}

/// Opens a link in whatever app takes it. Files stay in stet: there is no
/// default app to hand them to.
pub fn open_url(target: &str) -> Result<(), String> {
    if !target.contains("://") && !target.starts_with("mailto:") {
        return Err("nothing here opens that kind of file".to_string());
    }
    // SAFETY: the shared application exists while the event loop runs; the
    // URL and the options are checked, live objects; no completion handler
    // is given.
    unsafe {
        let url: Option<Retained<NSURL>> = NSURL::URLWithString(&string(target));
        let url = url.ok_or("not a link")?;
        let app: *mut AnyObject = msg_send![class!(UIApplication), sharedApplication];
        let options: Retained<AnyObject> = msg_send_id![class!(NSDictionary), dictionary];
        let _: () = msg_send![
            app,
            openURL: &*url,
            options: &*options,
            completionHandler: std::ptr::null::<AnyObject>(),
        ];
    }
    Ok(())
}

/// The app's own folder, which Files shows as "Stet" under On My iPhone.
pub fn documents_dir() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("HOME")?).join("Documents"))
}

/// Settings, themes and recovery: kept, but out of the way of Files.
pub fn support_dir() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("HOME")?).join("Library/Application Support/stet"))
}

pub fn cache_dir() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("HOME")?).join("Library/Caches/stet"))
}

/// Where the system keeps its fonts: on a device, and inside a simulator.
pub fn font_dirs() -> Vec<PathBuf> {
    let root = ["SIMULATOR_ROOT", "IPHONE_SIMULATOR_ROOT"]
        .iter()
        .find_map(std::env::var_os)
        .map(PathBuf::from)
        .unwrap_or(PathBuf::from("/"));
    ["System/Library/Fonts", "Library/Fonts"]
        .iter()
        .map(|dir| root.join(dir))
        .collect()
}

/// The general pasteboard.
#[derive(Default)]
pub struct Pasteboard {
    _shared: (),
}

impl Pasteboard {
    fn general() -> *mut AnyObject {
        // SAFETY: a class property returning the one shared pasteboard.
        unsafe { msg_send![class!(UIPasteboard), generalPasteboard] }
    }
}

impl Clipboard for Pasteboard {
    fn get(&mut self) -> Option<String> {
        // SAFETY: `string` is nil when the pasteboard holds no text.
        let text: Option<Retained<NSString>> = unsafe { msg_send_id![Self::general(), string] };
        text.map(|text| text.to_string())
    }

    fn set(&mut self, text: &str) {
        // SAFETY: sets the pasteboard's text to a live string.
        let _: () = unsafe { msg_send![Self::general(), setString: &*string(text)] };
    }

    fn contents(&mut self) -> Clip {
        // SAFETY: nil when the pasteboard holds no HTML; the bytes are
        // copied out before the data object is released.
        let html: Option<Retained<NSData>> =
            unsafe { msg_send_id![Self::general(), dataForPasteboardType: &*string("public.html")] };
        Clip {
            text: self.get(),
            html: html.and_then(|data| String::from_utf8(data.bytes().to_vec()).ok()),
            files: Vec::new(),
        }
    }

    fn image(&mut self) -> Option<Vec<u8>> {
        // SAFETY: `image` is nil when there is no picture; the PNG data is
        // autoreleased and copied out at once.
        unsafe {
            let image: *mut AnyObject = msg_send![Self::general(), image];
            if image.is_null() {
                return None;
            }
            let png = UIImagePNGRepresentation(image);
            png.as_ref().map(|data| data.bytes().to_vec())
        }
    }

    fn set_rich(&mut self, text: &str, html: &str) {
        let keys = NSArray::from_vec(vec![string("public.utf8-plain-text"), string("public.html")]);
        let values = NSArray::from_vec(vec![string(text), string(html)]);
        // SAFETY: one pasteboard item offering the same text two ways,
        // built from live arrays of equal length.
        unsafe {
            let item: Retained<AnyObject> =
                msg_send_id![class!(NSDictionary), dictionaryWithObjects: &*values, forKeys: &*keys];
            let items: Retained<AnyObject> = msg_send_id![class!(NSArray), arrayWithObject: &*item];
            let _: () = msg_send![Self::general(), setItems: &*items];
        }
    }
}
