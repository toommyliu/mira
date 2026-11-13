use std::{
    cell::Cell,
    ptr,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use rquickjs::{
    allocator::{Allocator, RustAllocator},
    runtime::InterruptHandler,
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct Limits {
    pub(crate) turn: Duration,
    pub(crate) memory: usize,
    pub(crate) stack: usize,
    pub(crate) stop_grace: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            turn: Duration::from_secs(5),
            memory: 64 << 20,
            stack: 1 << 20,
            stop_grace: Duration::from_secs(5),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Tripped {
    Stop,
    Turn,
}

const BUSY_AT_STOP: Duration = Duration::from_millis(50);

pub(super) struct Clock {
    epoch: Instant,
    turn_started: AtomicU64,
    stop_requested: AtomicU64,
    stop: AtomicBool,
    tripped: AtomicU8,
}

impl Clock {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            epoch: Instant::now(),
            turn_started: AtomicU64::new(0),
            stop_requested: AtomicU64::new(0),
            stop: AtomicBool::new(false),
            tripped: AtomicU8::new(0),
        })
    }

    fn now(&self) -> u64 {
        (self.epoch.elapsed().as_nanos() as u64).max(1)
    }

    pub(super) fn turn<T>(&self, run: impl FnOnce() -> T) -> T {
        self.turn_started.store(self.now(), Ordering::SeqCst);
        let result = run();
        self.turn_started.store(0, Ordering::SeqCst);
        result
    }

    pub(super) fn request_stop(&self) {
        let _ =
            self.stop_requested
                .compare_exchange(0, self.now(), Ordering::SeqCst, Ordering::SeqCst);
    }

    pub(super) fn stop_requested(&self) -> bool {
        self.stop_requested.load(Ordering::SeqCst) != 0 || self.stop.load(Ordering::SeqCst)
    }

    pub(super) fn stop_now(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    pub(super) fn tripped(&self) -> Option<Tripped> {
        match self.tripped.load(Ordering::SeqCst) {
            1 => Some(Tripped::Stop),
            2 => Some(Tripped::Turn),
            _ => None,
        }
    }

    pub(super) fn interrupt_handler(self: &Arc<Self>, limits: &Limits) -> InterruptHandler {
        let clock = self.clone();
        let turn_limit = limits.turn.as_nanos() as u64;
        let grace = limits.stop_grace.as_nanos() as u64;
        let busy = BUSY_AT_STOP.as_nanos() as u64;
        Box::new(move || {
            let now = clock.now();
            let started = clock.turn_started.load(Ordering::SeqCst);
            let requested = clock.stop_requested.load(Ordering::SeqCst);
            let stopping = requested != 0
                && (now.saturating_sub(requested) > grace
                    || (started != 0 && started < requested && now - requested > busy));
            let tripped = if clock.stop.load(Ordering::SeqCst) || stopping {
                1
            } else if started != 0 && now.saturating_sub(started) > turn_limit {
                2
            } else {
                0
            };
            if tripped != 0 {
                let _ =
                    clock
                        .tripped
                        .compare_exchange(0, tripped, Ordering::SeqCst, Ordering::SeqCst);
            }
            tripped != 0
        })
    }
}

pub(super) struct CappedAllocator {
    limit: usize,
    used: usize,
    refused: Rc<Cell<bool>>,
}

impl CappedAllocator {
    pub(super) fn new(limit: usize, refused: Rc<Cell<bool>>) -> Self {
        Self {
            limit,
            used: 0,
            refused,
        }
    }

    fn admit(&mut self, size: usize) -> bool {
        let fits = self.used.saturating_add(size) <= self.limit;
        if !fits {
            self.refused.set(true);
        }
        fits
    }
}

// SAFETY: every allocation is made and released by RustAllocator; this type only tracks sizes.
unsafe impl Allocator for CappedAllocator {
    fn alloc(&mut self, size: usize) -> *mut u8 {
        if !self.admit(size) {
            return ptr::null_mut();
        }
        let allocated = RustAllocator.alloc(size);
        if !allocated.is_null() {
            self.used += unsafe { RustAllocator::usable_size(allocated) };
        }
        allocated
    }

    fn calloc(&mut self, count: usize, size: usize) -> *mut u8 {
        let Some(total) = count.checked_mul(size) else {
            return ptr::null_mut();
        };
        if !self.admit(total) {
            return ptr::null_mut();
        }
        let allocated = RustAllocator.calloc(count, size);
        if !allocated.is_null() {
            self.used += unsafe { RustAllocator::usable_size(allocated) };
        }
        allocated
    }

    unsafe fn dealloc(&mut self, allocated: *mut u8) {
        unsafe {
            self.used -= RustAllocator::usable_size(allocated);
            RustAllocator.dealloc(allocated);
        }
    }

    unsafe fn realloc(&mut self, allocated: *mut u8, new_size: usize) -> *mut u8 {
        if allocated.is_null() {
            return self.alloc(new_size);
        }
        let old_size = unsafe { RustAllocator::usable_size(allocated) };
        if new_size > old_size && !self.admit(new_size - old_size) {
            return ptr::null_mut();
        }
        let moved = unsafe { RustAllocator.realloc(allocated, new_size) };
        if !moved.is_null() {
            self.used = self.used - old_size + unsafe { RustAllocator::usable_size(moved) };
        }
        moved
    }

    unsafe fn usable_size(allocated: *mut u8) -> usize {
        unsafe { RustAllocator::usable_size(allocated) }
    }
}
