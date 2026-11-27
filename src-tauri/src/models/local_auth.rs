//! macOS LocalAuthentication (`Touch ID` with automatic Mac login password fallback)
//! via Apple `LocalAuthentication.framework`, `objc2`, and `block2`.
//! Supports deterministic replacement in automated tests and `--foundation-ui-smoke`
//! so automated suites never trigger blocking OS authentication dialogs.

use crate::i18n::{self, LanguageKind};
use block2::RcBlock;
use objc2::{
    msg_send,
    rc::Retained,
    runtime::{AnyClass, AnyObject, Bool},
};
use objc2_foundation::{NSError, NSString};
use std::{
    future::Future,
    pin::Pin,
    ptr,
    sync::{Arc, Mutex},
};
use tokio::sync::oneshot;

/// `LAPolicyDeviceOwnerAuthentication`: uses Touch ID / Apple Watch when available
/// and automatically falls back to the Mac user's login password otherwise.
const LA_POLICY_DEVICE_OWNER_AUTHENTICATION: isize = 2;
const LA_ERROR_USER_CANCEL: isize = -2;
const LA_ERROR_SYSTEM_CANCEL: isize = -4;
const LA_ERROR_APP_CANCEL: isize = -9;

#[link(name = "LocalAuthentication", kind = "framework")]
unsafe extern "C" {}

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

/// Keeps the `LAContext` alive across the async reply wait and invalidates it on drop.
struct LacContextGuard(Retained<AnyObject>);

// Apple documents `LAContext` evaluation & `invalidate` as thread-safe; we never share
// mutable references across threads and only invoke `invalidate` once during Drop.
unsafe impl Send for LacContextGuard {}

impl Drop for LacContextGuard {
    fn drop(&mut self) {
        unsafe {
            let _: () = msg_send![&*self.0, invalidate];
        }
    }
}

fn outcome_from_nserror(err_ptr: *mut NSError, locale: LanguageKind) -> LocalAuthOutcome {
    if let Some(err) = unsafe { err_ptr.as_ref() } {
        let code = err.code();
        if matches!(
            code,
            LA_ERROR_USER_CANCEL | LA_ERROR_SYSTEM_CANCEL | LA_ERROR_APP_CANCEL
        ) {
            return LocalAuthOutcome::Cancelled;
        }
    }
    LocalAuthOutcome::Failed(i18n::models::model_reveal_auth_failed_message(locale).into())
}

fn start_macos_evaluation(
    reason: &str,
    locale: LanguageKind,
) -> Result<(LacContextGuard, oneshot::Receiver<LocalAuthOutcome>), LocalAuthOutcome> {
    let Some(la_class) = AnyClass::get(c"LAContext") else {
        return Err(LocalAuthOutcome::Failed(
            i18n::models::model_reveal_auth_unavailable_message(locale).into(),
        ));
    };
    let ctx: Option<Retained<AnyObject>> = unsafe { msg_send![la_class, new] };
    let Some(ctx) = ctx else {
        return Err(LocalAuthOutcome::Failed(
            i18n::models::model_reveal_auth_unavailable_message(locale).into(),
        ));
    };
    let guard = LacContextGuard(ctx);

    let mut can_err: *mut NSError = ptr::null_mut();
    let can_eval: Bool = unsafe {
        msg_send![
            &*guard.0,
            canEvaluatePolicy: LA_POLICY_DEVICE_OWNER_AUTHENTICATION,
            error: &mut can_err
        ]
    };
    if !can_eval.as_bool() {
        return Err(outcome_from_nserror(can_err, locale));
    }

    let (tx, rx) = oneshot::channel();
    let tx_slot = Arc::new(Mutex::new(Some(tx)));
    let reply_block = RcBlock::new(move |success: Bool, err_ptr: *mut NSError| {
        let outcome = if success.as_bool() {
            LocalAuthOutcome::Authenticated
        } else {
            outcome_from_nserror(err_ptr, locale)
        };
        if let Ok(mut slot) = tx_slot.lock()
            && let Some(sender) = slot.take()
        {
            let _ = sender.send(outcome);
        }
    });

    let reason_ns = NSString::from_str(reason);
    unsafe {
        let _: () = msg_send![
            &*guard.0,
            evaluatePolicy: LA_POLICY_DEVICE_OWNER_AUTHENTICATION,
            localizedReason: &*reason_ns,
            reply: &*reply_block
        ];
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

#[cfg(any(test, feature = "smoke-harness"))]
pub struct StaticLocalAuthenticator {
    outcome: Mutex<LocalAuthOutcome>,
    calls: Mutex<Vec<(String, LanguageKind)>>,
}

#[cfg(any(test, feature = "smoke-harness"))]
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

#[cfg(any(test, feature = "smoke-harness"))]
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
