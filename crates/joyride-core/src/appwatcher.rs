use std::cell::RefCell;
use std::ptr::NonNull;
use std::rc::Rc;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSNotification, NSObjectProtocol, NSString};

/// Monitors the frontmost application via NSWorkspace notifications.
/// Exposes the current bundle ID for profile switching.
pub struct AppWatcher {
    /// The bundle ID of the currently frontmost application.
    pub frontmost_bundle_id: Rc<RefCell<String>>,
    _observer: RefCell<Option<Retained<objc2::runtime::ProtocolObject<dyn NSObjectProtocol>>>>,
}

impl Default for AppWatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl AppWatcher {
    pub fn new() -> Self {
        Self {
            frontmost_bundle_id: Rc::new(RefCell::new(String::new())),
            _observer: RefCell::new(None),
        }
    }

    pub fn start(&self) {
        self.check_frontmost();

        let frontmost = Rc::clone(&self.frontmost_bundle_id);

        let block = RcBlock::new(move |_notif: NonNull<NSNotification>| {
            let workspace = NSWorkspace::sharedWorkspace();
            let bundle_id = workspace
                .frontmostApplication()
                .and_then(|app| app.bundleIdentifier())
                .map(|s| s.to_string())
                .unwrap_or_default();
            *frontmost.borrow_mut() = bundle_id;
        });

        let workspace = NSWorkspace::sharedWorkspace();
        let center = workspace.notificationCenter();
        let name = NSString::from_str("NSWorkspaceDidActivateApplicationNotification");
        let observer = unsafe {
            center.addObserverForName_object_queue_usingBlock(Some(&name), None, None, &block)
        };
        *self._observer.borrow_mut() = Some(observer);
    }

    fn check_frontmost(&self) {
        let workspace = NSWorkspace::sharedWorkspace();
        let bundle_id = workspace
            .frontmostApplication()
            .and_then(|app| app.bundleIdentifier())
            .map(|s| s.to_string())
            .unwrap_or_default();
        *self.frontmost_bundle_id.borrow_mut() = bundle_id;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_has_empty_frontmost() {
        let watcher = AppWatcher::new();
        assert!(watcher.frontmost_bundle_id.borrow().is_empty());
    }
}
