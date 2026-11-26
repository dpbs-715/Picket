//! macOS LocalAuthentication (`Touch ID` with automatic Mac login password fallback)
//! via Apple `LocalAuthentication.framework` and Objective-C runtime FFI.
//! Supports deterministic replacement in automated tests and `--foundation-ui-smoke`
//! so automated suites never trigger blocking OS authentication dialogs.
use crate::i18n::{self, LanguageKind};
use std::{
    ffi::{c_char, c_void},
    future::Future,
    mem,
    pin::Pin,
    ptr,
    sync::{Arc, Mutex},
};
use tokio::sync::oneshot;

const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
/// `LAPolicyDeviceOwnerAuthentication`: uses Touch ID / Apple Watch when available
/// and automatically falls back to the Mac user's login password otherwise.
const LA_POLICY_DEVICE_OWNER_AUTHENTICATION: isize = 2;
const LA_ERROR_USER_CANCEL: isize = -2;
const LA_ERROR_SYSTEM_CANCEL: isize = -4;
const LA_ERROR_APP_CANCEL: isize = -9;
const BLOCK_HAS_COPY_DISPOSE: i32 = 1 << 25;

type CFTypeRef = *const c_void;
type CFStringRef = *const c_void;
type CFAllocatorRef = *const c_void;
type CFIndex = isize;
type ObjcBool = u8;

#[link(name = "LocalAuthentication", kind = "framework")]
unsafe extern "C" {}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(cf: CFTypeRef);
    fn CFStringCreateWithBytes(
        alloc: CFAllocatorRef,
        bytes: *const u8,
        num_bytes: CFIndex,
        encoding: u32,
        is_external_representation: u8,
    ) -> CFStringRef;
}

#[link(name = "objc", kind = "dylib")]
unsafe extern "C" {
    fn objc_getClass(name: *const c_char) -> *mut c_void;
    fn sel_registerName(name: *const c_char) -> *const c_void;
    fn objc_msgSend();
    fn objc_release(obj: *mut c_void);
}

#[link(name = "System", kind = "dylib")]
unsafe extern "C" {
    static _NSConcreteStackBlock: *const c_void;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocalAuthOutcome {
    Authenticated,
    Cancelled,
    Failed(String),
}

pub trait LocalAuthenticator: Send + Sync {
    fn authenticate<'a>(
        &'a self,
        reason: &'a str,
        locale: LanguageKind,
    ) -> Pin<Box<dyn Future<Output = LocalAuthOutcome> + Send + 'a>>;
}

#[derive(Clone, Copy, Default)]
pub struct MacOsLocalAuthenticator;

struct CfStringOwned(CFStringRef);

impl CfStringOwned {
    fn new(text: &str) -> Option<Self> {
        let raw = unsafe {
            CFStringCreateWithBytes(
                ptr::null(),
                text.as_ptr(),
                text.len() as CFIndex,
                K_CF_STRING_ENCODING_UTF8,
                0,
            )
        };
        if raw.is_null() { None } else { Some(Self(raw)) }
    }

    fn as_ptr(&self) -> CFStringRef {
        self.0
    }
}

impl Drop for CfStringOwned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) };
        }
    }
}

struct LacContextGuard(usize);

impl Drop for LacContextGuard {
    fn drop(&mut self) {
        let ctx = self.0 as *mut c_void;
        if !ctx.is_null() {
            unsafe {
                let invalidate_sel = sel_registerName(c"invalidate".as_ptr());
                let send_invalidate: unsafe extern "C" fn(*mut c_void, *const c_void) =
                    mem::transmute(objc_msgSend as *const ());
                send_invalidate(ctx, invalidate_sel);
                objc_release(ctx);
            }
        }
    }
}

struct ReplyPayload {
    tx: Mutex<Option<oneshot::Sender<LocalAuthOutcome>>>,
    locale: LanguageKind,
}

#[repr(C)]
struct BlockDescriptor {
    reserved: usize,
    size: usize,
    copy_helper: unsafe extern "C" fn(*mut AuthReplyBlock, *const AuthReplyBlock),
    dispose_helper: unsafe extern "C" fn(*mut AuthReplyBlock),
}

#[repr(C)]
struct AuthReplyBlock {
    isa: *const c_void,
    flags: i32,
    reserved: i32,
    invoke: unsafe extern "C" fn(*mut AuthReplyBlock, ObjcBool, *mut c_void),
    descriptor: *const BlockDescriptor,
    payload: *const ReplyPayload,
}

static AUTH_BLOCK_DESCRIPTOR: BlockDescriptor = BlockDescriptor {
    reserved: 0,
    size: mem::size_of::<AuthReplyBlock>(),
    copy_helper: auth_block_copy,
    dispose_helper: auth_block_dispose,
};

unsafe extern "C" fn auth_block_copy(_dst: *mut AuthReplyBlock, src: *const AuthReplyBlock) {
    let ptr = unsafe { (*src).payload };
    if !ptr.is_null() {
        unsafe { Arc::increment_strong_count(ptr) };
    }
}

unsafe extern "C" fn auth_block_dispose(block: *mut AuthReplyBlock) {
    let ptr = unsafe { (*block).payload };
    if !ptr.is_null() {
        unsafe { drop(Arc::from_raw(ptr)) };
    }
}

fn outcome_from_nserror(err_ptr: *mut c_void, locale: LanguageKind) -> LocalAuthOutcome {
    if !err_ptr.is_null() {
        let code = unsafe {
            let code_sel = sel_registerName(c"code".as_ptr());
            let send_code: unsafe extern "C" fn(*mut c_void, *const c_void) -> isize =
                mem::transmute(objc_msgSend as *const ());
            send_code(err_ptr, code_sel)
        };
        if matches!(
            code,
            LA_ERROR_USER_CANCEL | LA_ERROR_SYSTEM_CANCEL | LA_ERROR_APP_CANCEL
        ) {
            return LocalAuthOutcome::Cancelled;
        }
    }
    LocalAuthOutcome::Failed(i18n::models::model_reveal_auth_failed_message(locale).into())
}

unsafe extern "C" fn auth_block_invoke(
    block: *mut AuthReplyBlock,
    success: ObjcBool,
    err_ptr: *mut c_void,
) {
    let payload_ptr = unsafe { (*block).payload };
    if payload_ptr.is_null() {
        return;
    }
    let payload = unsafe { &*payload_ptr };
    let outcome = if success != 0 {
        LocalAuthOutcome::Authenticated
    } else {
        outcome_from_nserror(err_ptr, payload.locale)
    };
    if let Ok(mut slot) = payload.tx.lock()
        && let Some(tx) = slot.take()
    {
        let _ = tx.send(outcome);
    }
}

fn start_macos_evaluation(
    reason: &str,
    locale: LanguageKind,
) -> Result<(LacContextGuard, oneshot::Receiver<LocalAuthOutcome>), LocalAuthOutcome> {
    let Some(reason_cf) = CfStringOwned::new(reason) else {
        return Err(LocalAuthOutcome::Failed(
            i18n::models::model_reveal_auth_unavailable_message(locale).into(),
        ));
    };
    let la_class = unsafe { objc_getClass(c"LAContext".as_ptr()) };
    if la_class.is_null() {
        return Err(LocalAuthOutcome::Failed(
            i18n::models::model_reveal_auth_unavailable_message(locale).into(),
        ));
    }
    let ctx = unsafe {
        let alloc_sel = sel_registerName(c"alloc".as_ptr());
        let init_sel = sel_registerName(c"init".as_ptr());
        let send_obj: unsafe extern "C" fn(*mut c_void, *const c_void) -> *mut c_void =
            mem::transmute(objc_msgSend as *const ());
        let allocated = send_obj(la_class, alloc_sel);
        if allocated.is_null() {
            ptr::null_mut()
        } else {
            send_obj(allocated, init_sel)
        }
    };
    if ctx.is_null() {
        return Err(LocalAuthOutcome::Failed(
            i18n::models::model_reveal_auth_unavailable_message(locale).into(),
        ));
    }
    let guard = LacContextGuard(ctx as usize);

    let mut can_err: *mut c_void = ptr::null_mut();
    let can_eval = unsafe {
        let can_sel = sel_registerName(c"canEvaluatePolicy:error:".as_ptr());
        let send_can: unsafe extern "C" fn(
            *mut c_void,
            *const c_void,
            isize,
            *mut *mut c_void,
        ) -> ObjcBool = mem::transmute(objc_msgSend as *const ());
        send_can(
            ctx,
            can_sel,
            LA_POLICY_DEVICE_OWNER_AUTHENTICATION,
            &mut can_err,
        )
    };
    if can_eval == 0 {
        return Err(outcome_from_nserror(can_err, locale));
    }

    let (tx, rx) = oneshot::channel();
    let payload = Arc::new(ReplyPayload {
        tx: Mutex::new(Some(tx)),
        locale,
    });
    let raw_payload = Arc::into_raw(payload);
    let stack_block = AuthReplyBlock {
        isa: unsafe { _NSConcreteStackBlock },
        flags: BLOCK_HAS_COPY_DISPOSE,
        reserved: 0,
        invoke: auth_block_invoke,
        descriptor: &AUTH_BLOCK_DESCRIPTOR,
        payload: raw_payload,
    };

    unsafe {
        let eval_sel = sel_registerName(c"evaluatePolicy:localizedReason:reply:".as_ptr());
        let send_eval: unsafe extern "C" fn(
            *mut c_void,
            *const c_void,
            isize,
            CFStringRef,
            *const AuthReplyBlock,
        ) = mem::transmute(objc_msgSend as *const ());
        send_eval(
            ctx,
            eval_sel,
            LA_POLICY_DEVICE_OWNER_AUTHENTICATION,
            reason_cf.as_ptr(),
            &stack_block,
        );
        // Balance stack ownership; Objective-C retained its own heap copy via `auth_block_copy`.
        drop(Arc::from_raw(raw_payload));
    }

    Ok((guard, rx))
}

impl LocalAuthenticator for MacOsLocalAuthenticator {
    fn authenticate<'a>(
        &'a self,
        reason: &'a str,
        locale: LanguageKind,
    ) -> Pin<Box<dyn Future<Output = LocalAuthOutcome> + Send + 'a>> {
        Box::pin(async move {
            let (_guard, rx) = match start_macos_evaluation(reason, locale) {
                Ok(pair) => pair,
                Err(outcome) => return outcome,
            };
            rx.await.unwrap_or_else(|_| {
                LocalAuthOutcome::Failed(
                    i18n::models::model_reveal_auth_failed_message(locale).into(),
                )
            })
        })
    }
}

pub struct StaticLocalAuthenticator {
    outcome: Mutex<LocalAuthOutcome>,
    calls: Mutex<Vec<(String, LanguageKind)>>,
}

impl StaticLocalAuthenticator {
    pub fn new(outcome: LocalAuthOutcome) -> Self {
        Self {
            outcome: Mutex::new(outcome),
            calls: Mutex::new(Vec::new()),
        }
    }

    #[cfg(test)]
    pub fn set_outcome(&self, next: LocalAuthOutcome) {
        if let Ok(mut guard) = self.outcome.lock() {
            *guard = next;
        }
    }

    #[cfg(test)]
    pub fn calls(&self) -> Vec<(String, LanguageKind)> {
        self.calls.lock().map(|g| g.clone()).unwrap_or_default()
    }
}

impl LocalAuthenticator for StaticLocalAuthenticator {
    fn authenticate<'a>(
        &'a self,
        reason: &'a str,
        locale: LanguageKind,
    ) -> Pin<Box<dyn Future<Output = LocalAuthOutcome> + Send + 'a>> {
        if let Ok(mut calls) = self.calls.lock() {
            calls.push((reason.to_owned(), locale));
        }
        let outcome = self.outcome.lock().map(|g| g.clone()).unwrap_or_else(|_| {
            LocalAuthOutcome::Failed(i18n::models::model_reveal_auth_failed_message(locale).into())
        });
        Box::pin(async move { outcome })
    }
}
