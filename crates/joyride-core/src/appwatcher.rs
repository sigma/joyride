use std::cell::Cell;
use std::collections::HashSet;
use std::ptr::NonNull;
use std::rc::Rc;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSNotification, NSObjectProtocol, NSString};

pub struct AppWatcher {
    excluded: HashSet<String>,
    pub is_excluded_active: Rc<Cell<bool>>,
    _observer: RefCell<Option<Retained<objc2::runtime::ProtocolObject<dyn NSObjectProtocol>>>>,
}

use std::cell::RefCell;

impl AppWatcher {
    pub fn new(excluded_ids: &[String]) -> Self {
        Self {
            excluded: excluded_ids.iter().cloned().collect(),
            is_excluded_active: Rc::new(Cell::new(false)),
            _observer: RefCell::new(None),
        }
    }

    pub fn start(&self) {
        self.check_frontmost();

        let excluded = self.excluded.clone();
        let is_excluded = Rc::clone(&self.is_excluded_active);

        let block = RcBlock::new(move |_notif: NonNull<NSNotification>| {
            let workspace = NSWorkspace::sharedWorkspace();
            let bundle_id = workspace
                .frontmostApplication()
                .and_then(|app| app.bundleIdentifier())
                .map(|s| s.to_string())
                .unwrap_or_default();
            let was = is_excluded.get();
            let now = excluded.contains(&bundle_id);
            is_excluded.set(now);
            if now != was {
                if now {
                    eprintln!("joyride: paused (gaming app: {bundle_id})");
                } else {
                    eprintln!("joyride: resumed");
                }
            }
        });

        let workspace = NSWorkspace::sharedWorkspace();
        let center = workspace.notificationCenter();
        let name = NSString::from_str("NSWorkspaceDidActivateApplicationNotification");
        let observer = unsafe {
            center.addObserverForName_object_queue_usingBlock(
                Some(&name),
                None,
                None,
                &block,
            )
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
        self.is_excluded_active.set(self.excluded.contains(&bundle_id));
    }
}
