use std::sync::{Mutex, MutexGuard};

use crate::{Error, ErrorCode};

#[derive(Debug)]
pub(crate) struct Budget {
    inner: Mutex<BudgetInner>,
    maximum_requests: usize,
    maximum_bytes: usize,
    parent: Option<std::sync::Arc<Budget>>,
}

#[derive(Debug)]
struct BudgetInner {
    requests: usize,
    bytes: usize,
}

impl Budget {
    pub(crate) fn new(maximum_requests: usize, maximum_bytes: usize) -> Self {
        Self {
            inner: Mutex::new(BudgetInner {
                requests: 0,
                bytes: 0,
            }),
            maximum_requests,
            maximum_bytes,
            parent: None,
        }
    }

    pub(crate) fn with_parent(
        maximum_requests: usize,
        maximum_bytes: usize,
        parent: std::sync::Arc<Budget>,
    ) -> Self {
        let mut budget = Self::new(maximum_requests, maximum_bytes);
        budget.parent = Some(parent);
        budget
    }

    pub(crate) fn reserve(
        self: &std::sync::Arc<Self>,
        bytes: usize,
    ) -> Result<AdmissionPermit, Error> {
        let upstream = self
            .parent
            .as_ref()
            .map(|p| p.reserve(bytes).map(Box::new))
            .transpose()?;
        let mut inner = self.lock();
        if inner.requests >= self.maximum_requests
            || inner.bytes.saturating_add(bytes) > self.maximum_bytes
        {
            return Err(Error::new(
                ErrorCode::QueueFull,
                "per-device request or retained-byte budget is full",
            ));
        }
        inner.requests += 1;
        inner.bytes += bytes;
        Ok(AdmissionPermit {
            budget: self.clone(),
            bytes,
            _upstream: upstream,
        })
    }

    fn lock(&self) -> MutexGuard<'_, BudgetInner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[derive(Debug)]
pub(crate) struct AdmissionPermit {
    budget: std::sync::Arc<Budget>,
    bytes: usize,
    _upstream: Option<Box<AdmissionPermit>>,
}

impl Drop for AdmissionPermit {
    fn drop(&mut self) {
        let mut inner = self.budget.lock();
        inner.requests = inner.requests.saturating_sub(1);
        inner.bytes = inner.bytes.saturating_sub(self.bytes);
    }
}
