use std::sync::atomic::{AtomicBool, Ordering};

use super::{Notice, Sink};

pub struct Desktop;

impl Sink for Desktop {
    fn send(&mut self, notice: Notice) {
        if rune::is_headless() {
            return;
        }
        let _ = std::thread::Builder::new()
            .name("notification".into())
            .spawn(move || {
                static WARNED: AtomicBool = AtomicBool::new(false);
                if notify_rust::Notification::new()
                    .summary(&notice.title)
                    .body(&notice.body)
                    .appname("Cipher")
                    .show()
                    .is_err()
                    && !WARNED.swap(true, Ordering::Relaxed)
                {
                    eprintln!("cipher: desktop notifications unavailable");
                }
            });
    }
}
