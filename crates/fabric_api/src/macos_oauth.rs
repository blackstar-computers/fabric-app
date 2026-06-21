//! In-app Google SSO via ASWebAuthenticationSession (ephemeral sheet, not Safari).

use std::sync::mpsc;

use block2::RcBlock;
use objc2::define_class;
use objc2::rc::Retained;
use objc2::runtime::{NSObjectProtocol, ProtocolObject};
use objc2::{AnyThread, MainThreadMarker, MainThreadOnly, msg_send};
use objc2_app_kit::NSApplication;
use objc2_authentication_services::{
    ASWebAuthenticationPresentationContextProviding, ASWebAuthenticationSession,
    ASWebAuthenticationSessionCallback, ASWebAuthenticationSessionCompletionHandler,
    ASPresentationAnchor,
};
use objc2_foundation::{NSError, NSURL, NSString};

#[derive(Debug, thiserror::Error)]
pub enum MacAuthError {
    #[error("sign in canceled")]
    Canceled,
    #[error("authentication failed: {0}")]
    Failed(String),
    #[error("main thread unavailable")]
    NoMainThread,
}

define_class!(
    #[unsafe(super(objc2::runtime::NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FabricAuthPresentationAnchor"]
    struct PresentationAnchor;

    unsafe impl NSObjectProtocol for PresentationAnchor {}

    unsafe impl ASWebAuthenticationPresentationContextProviding for PresentationAnchor {
        #[unsafe(method_id(presentationAnchorForWebAuthenticationSession:))]
        fn presentation_anchor_for_web_authentication_session(
            &self,
            _session: &ASWebAuthenticationSession,
        ) -> Retained<ASPresentationAnchor> {
            let mtm = MainThreadMarker::new().expect("main thread");
            let app = NSApplication::sharedApplication(mtm);
            let window = app
                .keyWindow()
                .or_else(|| app.mainWindow())
                .or_else(|| app.windows().firstObject())
                .expect("no window for auth presentation");
            unsafe { Retained::cast_unchecked(window) }
        }
    }
);

/// Present the portal SSO page in an in-app authentication sheet.
pub fn authenticate_in_app(auth_url: &str) -> Result<String, MacAuthError> {
    let (tx, rx) = mpsc::sync_channel(1);
    dispatch2::DispatchQueue::main().exec_sync(move || {
        let result = run_session(auth_url);
        let _ = tx.send(result);
    });
    rx.recv()
        .unwrap_or(Err(MacAuthError::Failed("channel closed".into())))
}

fn run_session(auth_url: &str) -> Result<String, MacAuthError> {
    let mtm = MainThreadMarker::new().ok_or(MacAuthError::NoMainThread)?;
    let (tx, rx) = mpsc::sync_channel(1);

    let handler = RcBlock::new(
        move |url: *mut NSURL, error: *mut NSError| {
            let result = unsafe {
                if !url.is_null() {
                    let url = &*url;
                    url.absoluteString()
                        .map(|s| s.to_string())
                        .ok_or_else(|| MacAuthError::Failed("empty callback url".into()))
                } else if !error.is_null() {
                    let err = &*error;
                    let msg = err.localizedDescription().to_string();
                    if msg.to_ascii_lowercase().contains("cancel") {
                        Err(MacAuthError::Canceled)
                    } else {
                        Err(MacAuthError::Failed(msg))
                    }
                } else {
                    Err(MacAuthError::Canceled)
                }
            };
            let _ = tx.send(result);
        },
    );
    let handler: ASWebAuthenticationSessionCompletionHandler =
        RcBlock::into_raw(handler).cast();

    let ns_url = NSURL::URLWithString(&NSString::from_str(auth_url))
        .ok_or_else(|| MacAuthError::Failed(format!("invalid auth url: {auth_url}")))?;
    let scheme = NSString::from_str(fabric_types::AUTH_CALLBACK_SCHEME);
    let callback =
        unsafe { ASWebAuthenticationSessionCallback::callbackWithCustomScheme(&scheme) };

    let session = unsafe {
        ASWebAuthenticationSession::initWithURL_callback_completionHandler(
            ASWebAuthenticationSession::alloc(),
            &ns_url,
            &callback,
            handler,
        )
    };

    unsafe {
        session.setPrefersEphemeralWebBrowserSession(true);
        let app = NSApplication::sharedApplication(mtm);
        if app.keyWindow().is_none()
            && app.mainWindow().is_none()
            && app.windows().firstObject().is_none()
        {
            return Err(MacAuthError::Failed(
                "no application window for sign-in".into(),
            ));
        }
        let allocated = PresentationAnchor::alloc(mtm);
        let provider: Retained<PresentationAnchor> = msg_send![allocated, init];
        let provider = ProtocolObject::from_ref(&*provider);
        session.setPresentationContextProvider(Some(provider));
        if !session.start() {
            return Err(MacAuthError::Failed("could not start auth session".into()));
        }
    }

    rx.recv()
        .unwrap_or(Err(MacAuthError::Failed("auth session dropped".into())))
}
