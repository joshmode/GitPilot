//! Native OS notification delivery.
//!
//! Delivery is advisory and best-effort: an unavailable notification centre
//! must never block sync, review or the in-app notification inbox. macOS uses
//! UserNotifications and requires a real application bundle; invoking the API
//! from a bare executable raises an Objective-C exception, so this adapter
//! validates the bundle first and catches any framework exception.

use crate::domain::{Notification, Severity};
use crate::error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeNotificationStatus {
    Available,
    Unavailable(&'static str),
    NotRequested,
}

impl NativeNotificationStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Available => "native delivery available",
            Self::Unavailable(reason) => reason,
            Self::NotRequested => "native delivery not requested",
        }
    }
}

pub fn status() -> NativeNotificationStatus {
    #[cfg(target_os = "macos")]
    {
        if macos::has_application_bundle() {
            NativeNotificationStatus::Available
        } else {
            NativeNotificationStatus::Unavailable(
                "native notifications require a packaged .app bundle",
            )
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        NativeNotificationStatus::Unavailable(
            "native OS notifications are not implemented for this platform",
        )
    }
}

/// Deliver a notification through the platform notification centre. The
/// in-app record has already been persisted by the caller. This function
/// never creates, mutates or publishes GitHub content.
pub fn deliver(notification: &Notification) -> Result<NativeNotificationStatus> {
    #[cfg(target_os = "macos")]
    {
        if !macos::has_application_bundle() {
            return Ok(NativeNotificationStatus::Unavailable(
                "native notifications require a packaged .app bundle",
            ));
        }
        macos::deliver(notification)?;
        Ok(NativeNotificationStatus::Available)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = notification;
        Ok(NativeNotificationStatus::Unavailable(
            "native OS notifications are not implemented for this platform",
        ))
    }
}

fn severity_prefix(severity: Severity) -> &'static str {
    match severity {
        Severity::Info => "Info",
        Severity::Low => "Low",
        Severity::Medium => "Medium",
        Severity::High => "High",
        Severity::Critical => "Critical",
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use block2::RcBlock;
    use objc2::rc::autoreleasepool;
    use objc2_foundation::{NSError, NSString};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationRequest,
        UNTimeIntervalNotificationTrigger, UNUserNotificationCenter,
    };
    use std::sync::atomic::{AtomicBool, Ordering};

    static AUTHORIZATION_REQUESTED: AtomicBool = AtomicBool::new(false);

    pub(super) fn has_application_bundle() -> bool {
        std::env::current_exe()
            .ok()
            .and_then(|path| {
                path.ancestors()
                    .find(|ancestor| {
                        ancestor
                            .extension()
                            .is_some_and(|extension| extension == "app")
                    })
                    .map(|_| true)
            })
            .unwrap_or(false)
    }

    pub(super) fn deliver(notification: &Notification) -> Result<()> {
        let title = format!("GitPilot · {}", severity_prefix(notification.severity));
        let body = format!(
            "{}\n{}",
            notification.title,
            notification.body.chars().take(600).collect::<String>()
        );
        let identifier = format!("gitpilot-{}", notification.id);
        let result = objc2::exception::catch(|| {
            autoreleasepool(|_| {
                let center = UNUserNotificationCenter::currentNotificationCenter();
                if !AUTHORIZATION_REQUESTED.swap(true, Ordering::SeqCst) {
                    let auth_block =
                        RcBlock::new(|_granted: objc2::runtime::Bool, _error: *mut NSError| {});
                    center.requestAuthorizationWithOptions_completionHandler(
                        UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
                        &auth_block,
                    );
                }
                let content = UNMutableNotificationContent::new();
                content.setTitle(&NSString::from_str(&title));
                content.setBody(&NSString::from_str(&body));
                let trigger =
                    UNTimeIntervalNotificationTrigger::triggerWithTimeInterval_repeats(1.0, false);
                let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
                    &NSString::from_str(&identifier),
                    &content,
                    Some(&trigger),
                );
                let completion = RcBlock::new(|_error: *mut NSError| {});
                center.addNotificationRequest_withCompletionHandler(&request, Some(&completion));
            });
        });
        match result {
            Ok(()) => Ok(()),
            Err(_) => Err(crate::error::GitPilotError::Unavailable(
                "the native notification centre rejected this process".into(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_prefix_is_explicit() {
        assert_eq!(severity_prefix(Severity::Critical), "Critical");
    }
}
