//! HERDR: the shared, bounded image cache behind `reel`.

use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

use gpui::{
    App, AppContext, Entity, Global, ImageCache, ImageCacheError, ImageCacheItem, RenderImage,
    Resource, Window, hash,
};

/// How many decoded frames all reels together keep.
pub(crate) const KEEP: usize = 48;

pub(crate) struct ReelCache {
    items: HashMap<u64, ImageCacheItem>,
    order: VecDeque<u64>,
}

struct Shared(Entity<ReelCache>);

impl Global for Shared {}

/// The one cache every `reel` loads through.
pub(crate) fn cache(cx: &mut App) -> Entity<ReelCache> {
    if let Some(shared) = cx.try_global::<Shared>() {
        return shared.0.clone();
    }
    let cache = cx.new(|_| ReelCache {
        items: HashMap::new(),
        order: VecDeque::new(),
    });
    cx.set_global(Shared(cache.clone()));
    cache
}

impl ReelCache {
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.items.len()
    }
}

impl ImageCache for ReelCache {
    fn load(
        &mut self,
        resource: &Resource,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
        let key = hash(resource);
        if self.items.contains_key(&key) {
            self.order.retain(|kept| *kept != key);
        } else {
            self.items.insert(key, ImageCacheItem::new(resource, cx));
        }
        self.order.push_back(key);
        while self.order.len() > KEEP {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(item) = self.items.remove(&oldest)
                && let Some(Ok(image)) = item.get()
            {
                cx.drop_image(image, Some(window));
            }
        }
        self.items.get(&key)?.use_image(window)
    }
}
