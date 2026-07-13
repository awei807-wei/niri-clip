// SPDX-License-Identifier: GPL-3.0-only

use std::cell::RefCell;
use std::collections::VecDeque;

use gtk::gdk;
use gtk::glib;
use gtk4 as gtk;

const PREVIEW_CACHE_CAPACITY: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PreviewStart {
    Cached,
    Pending,
    Load,
}

#[derive(Default)]
struct PreviewRequestState {
    current_id: Option<i64>,
    pending_id: Option<i64>,
}

impl PreviewRequestState {
    fn begin(&mut self, id: i64, cached: bool) -> PreviewStart {
        self.current_id = Some(id);
        if cached {
            self.pending_id = None;
            PreviewStart::Cached
        } else if self.pending_id == Some(id) {
            PreviewStart::Pending
        } else {
            self.pending_id = Some(id);
            PreviewStart::Load
        }
    }

    fn should_load(&self, id: i64) -> bool {
        self.current_id == Some(id) && self.pending_id == Some(id)
    }

    fn finish(&mut self, id: i64) -> bool {
        if !self.should_load(id) {
            return false;
        }
        self.pending_id = None;
        true
    }

    fn hide(&mut self, id: i64) -> bool {
        if self.current_id != Some(id) {
            return false;
        }
        self.cancel();
        true
    }

    fn cancel(&mut self) {
        self.current_id = None;
        self.pending_id = None;
    }
}

struct PreviewCache<T> {
    capacity: usize,
    entries: VecDeque<(i64, T)>,
}

pub(super) struct PreviewController {
    revealer: gtk::Revealer,
    stack: gtk::Stack,
    picture: gtk::Picture,
    state_title: gtk::Label,
    state_detail: gtk::Label,
    requests: RefCell<PreviewRequestState>,
    cache: RefCell<PreviewCache<gdk::Texture>>,
}

impl PreviewController {
    pub(super) fn new(
        revealer: gtk::Revealer,
        stack: gtk::Stack,
        picture: gtk::Picture,
        state_title: gtk::Label,
        state_detail: gtk::Label,
    ) -> Self {
        Self {
            revealer,
            stack,
            picture,
            state_title,
            state_detail,
            requests: RefCell::new(PreviewRequestState::default()),
            cache: RefCell::new(PreviewCache::new(PREVIEW_CACHE_CAPACITY)),
        }
    }

    /// Starts or reuses a preview request without queueing duplicate loads.
    pub(super) fn begin(&self, id: i64) -> PreviewStart {
        let texture = self.cache.borrow_mut().get(id);
        let start = self.requests.borrow_mut().begin(id, texture.is_some());
        if let Some(texture) = texture {
            self.show_texture(&texture);
            self.revealer.set_reveal_child(true);
            return start;
        }
        self.stack.set_visible_child_name("loading");
        self.revealer.set_reveal_child(true);
        start
    }

    pub(super) fn should_load(&self, id: i64) -> bool {
        self.requests.borrow().should_load(id)
    }

    pub(super) fn finish(&self, id: i64, preview: Option<Vec<u8>>) {
        if !self.requests.borrow_mut().finish(id) {
            return;
        }
        let Some(texture) = preview.and_then(texture_from_png) else {
            self.show_error();
            return;
        };
        self.cache.borrow_mut().insert(id, texture.clone());
        self.show_texture(&texture);
    }

    pub(super) fn fail(&self, id: i64) {
        if self.requests.borrow_mut().finish(id) {
            self.show_error();
        }
    }

    pub(super) fn hide(&self, id: i64) {
        if self.requests.borrow_mut().hide(id) {
            self.revealer.set_reveal_child(false);
        }
    }

    pub(super) fn cancel(&self) {
        self.requests.borrow_mut().cancel();
        self.revealer.set_reveal_child(false);
    }

    pub(super) fn retain(&self, mut keep: impl FnMut(i64) -> bool) {
        self.cache.borrow_mut().retain(&mut keep);
    }

    fn show_texture(&self, texture: &gdk::Texture) {
        self.picture.set_paintable(Some(texture));
        self.stack.set_visible_child_name("image");
    }

    fn show_error(&self) {
        self.state_title.set_text("PREVIEW UNAVAILABLE");
        self.state_detail
            .set_text("无法读取或解码图片；仍可按 Enter 恢复。");
        self.stack.set_visible_child_name("error");
        self.revealer.set_reveal_child(true);
    }
}

fn texture_from_png(bytes: Vec<u8>) -> Option<gdk::Texture> {
    let bytes = glib::Bytes::from_owned(bytes);
    gdk::Texture::from_bytes(&bytes).ok()
}

impl<T> PreviewCache<T> {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            entries: VecDeque::with_capacity(capacity),
        }
    }

    fn insert(&mut self, id: i64, value: T) {
        self.entries.retain(|(cached_id, _)| *cached_id != id);
        self.entries.push_front((id, value));
        self.entries.truncate(self.capacity);
    }

    fn retain(&mut self, mut keep: impl FnMut(i64) -> bool) {
        self.entries.retain(|(id, _)| keep(*id));
    }
}

impl<T: Clone> PreviewCache<T> {
    fn get(&mut self, id: i64) -> Option<T> {
        let index = self
            .entries
            .iter()
            .position(|(cached_id, _)| *cached_id == id)?;
        let entry = self.entries.remove(index)?;
        let value = entry.1.clone();
        self.entries.push_front(entry);
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::{PreviewCache, PreviewRequestState, PreviewStart};

    #[test]
    fn cache_is_bounded_and_promotes_hits() {
        let mut cache = PreviewCache::new(2);
        cache.insert(1, "one");
        cache.insert(2, "two");

        assert_eq!(cache.get(1), Some("one"));
        cache.insert(3, "three");

        assert_eq!(cache.get(1), Some("one"));
        assert_eq!(cache.get(2), None);
        assert_eq!(cache.get(3), Some("three"));
    }

    #[test]
    fn cache_retains_only_current_history_ids() {
        let mut cache = PreviewCache::new(3);
        cache.insert(1, "one");
        cache.insert(2, "two");
        cache.insert(3, "three");

        cache.retain(|id| id != 2);

        assert_eq!(cache.get(1), Some("one"));
        assert_eq!(cache.get(2), None);
        assert_eq!(cache.get(3), Some("three"));
    }

    #[test]
    fn requests_coalesce_duplicates_and_reject_stale_work() {
        let mut requests = PreviewRequestState::default();

        assert_eq!(requests.begin(1, false), PreviewStart::Load);
        assert_eq!(requests.begin(1, false), PreviewStart::Pending);
        assert!(requests.should_load(1));

        assert_eq!(requests.begin(2, false), PreviewStart::Load);
        assert!(!requests.should_load(1));
        assert!(requests.should_load(2));
        assert!(!requests.finish(1));
        assert!(requests.finish(2));
    }

    #[test]
    fn cancel_and_cached_hits_invalidate_pending_loads() {
        let mut requests = PreviewRequestState::default();
        assert_eq!(requests.begin(1, false), PreviewStart::Load);
        requests.cancel();
        assert!(!requests.should_load(1));

        assert_eq!(requests.begin(2, false), PreviewStart::Load);
        assert_eq!(requests.begin(3, true), PreviewStart::Cached);
        assert!(!requests.should_load(2));
        assert!(!requests.should_load(3));
    }
}
