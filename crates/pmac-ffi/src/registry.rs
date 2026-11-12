use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use pmac_core::{Client, Request, Runtime};

const KIND_MASK: u64 = 0xf000_0000_0000_0000;
const COUNTER_MASK: u64 = !KIND_MASK;
const RUNTIME: u64 = 0x1000_0000_0000_0000;
const CLIENT: u64 = 0x2000_0000_0000_0000;
const REQUEST: u64 = 0x3000_0000_0000_0000;

#[derive(Clone)]
pub(crate) enum Entry {
    Reserved,
    Runtime(Arc<Runtime>),
    Client {
        client: Arc<Client>,
        runtime: Arc<Runtime>,
    },
    Request {
        request: Arc<Request>,
        runtime: Arc<Runtime>,
    },
}

pub(crate) struct Registry {
    inner: Mutex<RegistryInner>,
}

struct RegistryInner {
    next: u64,
    entries: HashMap<u64, Entry>,
    counts: [usize; 4],
}

impl Registry {
    fn new() -> Self {
        Self {
            inner: Mutex::new(RegistryInner {
                next: 1,
                entries: HashMap::new(),
                counts: [0; 4],
            }),
        }
    }

    pub(crate) fn insert(&self, kind: u64, entry: Entry) -> Result<u64, pmac_core::Error> {
        let mut inner = self.lock();
        let category = (kind >> 60) as usize;
        let limit = match kind {
            RUNTIME => 16,
            CLIENT => 1024,
            REQUEST => 16_384,
            _ => return Err(pmac_core::Error::invalid("unknown handle kind")),
        };
        if inner.counts[category] >= limit {
            return Err(pmac_core::Error::new(
                pmac_core::ErrorCode::ResourceLimit,
                "process-wide handle quota is full",
            ));
        }
        if inner.next > COUNTER_MASK {
            return Err(pmac_core::Error::new(
                pmac_core::ErrorCode::ResourceLimit,
                "handle ID space exhausted",
            ));
        }
        let id = kind | inner.next;
        inner.next += 1;
        inner.entries.insert(id, entry);
        inner.counts[category] += 1;
        Ok(id)
    }

    pub(crate) fn reserve(&self, kind: u64) -> Result<u64, pmac_core::Error> {
        self.insert(kind, Entry::Reserved)
    }

    pub(crate) fn put(&self, id: u64, entry: Entry) -> Result<(), pmac_core::Error> {
        let mut inner = self.lock();
        if !matches!(inner.entries.get(&id), Some(Entry::Reserved)) {
            return Err(pmac_core::Error::internal(
                "handle reservation is missing or already published",
            ));
        }
        inner.entries.insert(id, entry);
        Ok(())
    }

    pub(crate) fn runtime(&self, id: u64) -> Option<Arc<Runtime>> {
        match self.get(id) {
            Some(Entry::Runtime(runtime)) => Some(runtime),
            _ => None,
        }
    }

    pub(crate) fn client(&self, id: u64) -> Option<(Arc<Client>, Arc<Runtime>)> {
        match self.get(id) {
            Some(Entry::Client { client, runtime }) => Some((client, runtime)),
            _ => None,
        }
    }

    pub(crate) fn request(&self, id: u64) -> Option<(Arc<Request>, Arc<Runtime>)> {
        match self.get(id) {
            Some(Entry::Request { request, runtime }) => Some((request, runtime)),
            _ => None,
        }
    }

    pub(crate) fn has_children(&self, owner: &Arc<Runtime>) -> bool {
        self.lock().entries.values().any(|entry| match entry {
            Entry::Client { runtime, .. } | Entry::Request { runtime, .. } => {
                Arc::ptr_eq(runtime, owner)
            }
            _ => false,
        })
    }
    pub(crate) fn remove(&self, id: u64, kind: u64) -> Option<Entry> {
        if id & KIND_MASK != kind {
            return None;
        }
        let mut inner = self.lock();
        let removed = inner.entries.remove(&id);
        if removed.is_some() {
            inner.counts[(kind >> 60) as usize] -= 1;
        }
        removed
    }

    fn get(&self, id: u64) -> Option<Entry> {
        self.lock().entries.get(&id).cloned()
    }
    fn lock(&self) -> MutexGuard<'_, RegistryInner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

pub(crate) fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(Registry::new)
}
pub(crate) fn runtime_kind() -> u64 {
    RUNTIME
}
pub(crate) fn client_kind() -> u64 {
    CLIENT
}
pub(crate) fn request_kind() -> u64 {
    REQUEST
}
