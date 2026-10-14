//! System notifications on macOS, silent: Pitwall plays its own sound (`sound.rs`). The
//! installed app posts through UserNotifications under its own name: it asks for permission
//! once, shows the banner even while it is in front, a click opens the project, and a newer
//! notification of a project replaces the older one. A dev binary has no bundle for that API; it
//! posts through the older one on behalf of Script Editor, the one borrowed name macOS still
//! shows banners for (Terminal's are dropped).
//!
//! Whether macOS lets Pitwall post is asked of it when the first run's last step shows it
//! (`access`), never kept.

use std::path::Path;
use std::ptr::NonNull;
use std::sync::{mpsc, OnceLock};
use std::thread;
use std::time::Duration;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, AllocAnyThread};
use objc2_foundation::{NSArray, NSError, NSString, NSURL};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNAuthorizationStatus, UNMutableNotificationContent, UNNotification, UNNotificationAttachment,
    UNNotificationPresentationOptions, UNNotificationRequest, UNNotificationResponse, UNNotificationSettings,
    UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};

/// Who a dev binary's notifications come from.
const DEV_SENDER: &str = "com.apple.ScriptEditor2";
/// How long the system is given to say what it allows.
const READ_WAIT: Duration = Duration::from_secs(3);
/// How long a test waits for the answer to macOS's question, when it asks one.
const ASK_WAIT: Duration = Duration::from_secs(60);

type OnClick = Box<dyn Fn(String) + Send + Sync>;

/// What a click on a notification does with its project.
static ON_CLICK: OnceLock<OnClick> = OnceLock::new();
static BUNDLED: OnceLock<bool> = OnceLock::new();

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and the class has no Drop.
    #[unsafe(super(NSObject))]
    #[name = "PitwallNotificationDelegate"]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl UNUserNotificationCenterDelegate for Delegate {
        /// In front, too: the banner shows (macOS hides it by default then).
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            handler: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            handler.call((UNNotificationPresentationOptions::Banner | UNNotificationPresentationOptions::List,));
        }

        /// A click: the request's identifier is the project.
        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            handler: &block2::DynBlock<dyn Fn()>,
        ) {
            clicked(response.notification().request().identifier().to_string());
            handler.call(());
        }
    }
);

impl Delegate {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(());
        // SAFETY: NSObject's init on a freshly allocated object.
        unsafe { msg_send![super(this), init] }
    }
}

fn clicked(path: String) {
    if let Some(on_click) = ON_CLICK.get() {
        on_click(path);
    }
}

/// Once, at start: from the installed app, asks for permission (macOS shows its prompt the first
/// time) and takes the clicks; from a dev binary, picks the borrowed sender.
pub fn start(bundled: bool, on_click: impl Fn(String) + Send + Sync + 'static) {
    let _ = BUNDLED.set(bundled);
    let _ = ON_CLICK.set(Box::new(on_click));

    if !bundled {
        let _ = mac_notification_sys::set_application(DEV_SENDER);
        return;
    }

    let center = UNUserNotificationCenter::currentNotificationCenter();
    let delegate = Delegate::new();
    center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    // The center holds its delegate weakly; this one lives as long as the app.
    std::mem::forget(delegate);

    let answered = RcBlock::new(|granted: Bool, _error: *mut NSError| {
        if cfg!(debug_assertions) {
            eprintln!("[pitwall] notifications allowed: {}", granted.as_bool());
        }
    });
    center.requestAuthorizationWithOptions_completionHandler(UNAuthorizationOptions::Alert, &answered);
}

fn bundled() -> bool {
    BUNDLED.get().copied().unwrap_or(false)
}

/// Whether macOS lets Pitwall post, asked of it now: `allowed`, `off` or `notAsked` (no answer to
/// its question yet). A dev binary's are Script Editor's, and so is their permission:
/// `borrowed`. `unknown` when the system doesn't answer in time.
pub fn access() -> &'static str {
    if !bundled() {
        return "borrowed";
    }
    let (tell, heard) = mpsc::channel();
    let answered = RcBlock::new(move |settings: NonNull<UNNotificationSettings>| {
        // SAFETY: the system hands the handler a live settings object.
        let _ = tell.send(unsafe { settings.as_ref() }.authorizationStatus());
    });
    UNUserNotificationCenter::currentNotificationCenter().getNotificationSettingsWithCompletionHandler(&answered);
    match heard.recv_timeout(READ_WAIT) {
        Ok(UNAuthorizationStatus::NotDetermined) => "notAsked",
        Ok(UNAuthorizationStatus::Denied) => "off",
        Ok(_) => "allowed",
        Err(_) => "unknown",
    }
}

/// Whether Pitwall may post, once macOS has said: it asks first when it never had an answer (its
/// question shows then), and waits a while for it. A dev binary has nothing to ask.
pub fn ask() -> bool {
    if !bundled() {
        return true;
    }
    let (tell, heard) = mpsc::channel();
    let answered = RcBlock::new(move |granted: Bool, _error: *mut NSError| {
        let _ = tell.send(granted.as_bool());
    });
    UNUserNotificationCenter::currentNotificationCenter().requestAuthorizationWithOptions_completionHandler(UNAuthorizationOptions::Alert, &answered);
    heard.recv_timeout(ASK_WAIT).unwrap_or(false)
}

/// System Settings › Notifications at the entry whose permission it is: Pitwall's (`identifier`,
/// the bundle's), or for a dev binary the app it posts as.
pub fn settings_url(identifier: &str) -> Option<String> {
    let id = if bundled() { identifier } else { DEV_SENDER };
    Some(format!("x-apple.systempreferences:com.apple.Notifications-Settings.extension?id={id}"))
}

/// A notification about `path`, with `image` (a PNG) beside the text.
pub fn post(path: String, title: String, body: String, image: Option<&Path>) {
    if bundled() {
        post_bundled(&path, &title, &body, image);
        return;
    }

    let image = image.map(|image| image.to_string_lossy().into_owned());
    // Waiting for the click blocks, so each notification gets its own thread.
    thread::spawn(move || {
        let mut notification = mac_notification_sys::Notification::new();
        notification.title(&title).message(&body).wait_for_click(true);
        if let Some(image) = image.as_deref() {
            notification.content_image(image);
        }
        if let Ok(mac_notification_sys::NotificationResponse::Click) = notification.send() {
            clicked(path);
        }
    });
}

fn post_bundled(path: &str, title: &str, body: &str, image: Option<&Path>) {
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(title));
    content.setBody(&NSString::from_str(body));

    if let Some(attachment) = image.and_then(attach) {
        content.setAttachments(&NSArray::from_retained_slice(&[attachment]));
    }

    let request = UNNotificationRequest::requestWithIdentifier_content_trigger(&NSString::from_str(path), &content, None);
    UNUserNotificationCenter::currentNotificationCenter().addNotificationRequest_withCompletionHandler(&request, None);
}

/// macOS moves an attached file into its own store, so each notification gets a copy.
fn attach(image: &Path) -> Option<Retained<UNNotificationAttachment>> {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos();
    let copy = image.with_file_name(format!("claude-mark-{nanos}.png"));
    std::fs::copy(image, &copy).ok()?;

    let url = NSURL::fileURLWithPath(&NSString::from_str(&copy.to_string_lossy()));
    // SAFETY: no options dictionary is passed.
    let attachment = unsafe { UNNotificationAttachment::attachmentWithIdentifier_URL_options_error(&NSString::from_str("claude"), &url, None) };
    if attachment.is_err() {
        let _ = std::fs::remove_file(&copy);
    }
    attachment.ok()
}
