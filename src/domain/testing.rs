use std::cell::RefCell;

use tracing_subscriber::util::SubscriberInitExt as _;

use crate::bridge::{CallError, Loader, Value};

type Handler = Box<dyn FnMut(&'static str, &[Value]) -> Result<Value, CallError>>;

pub(crate) struct FakeLoader {
    handler: RefCell<Handler>,
    calls: RefCell<Vec<&'static str>>,
}

impl FakeLoader {
    pub(crate) fn new(
        handler: impl FnMut(&'static str, &[Value]) -> Result<Value, CallError> + 'static,
    ) -> Self {
        Self {
            handler: RefCell::new(Box::new(handler)),
            calls: RefCell::default(),
        }
    }

    pub(crate) fn count(&self, callback: &str) -> usize {
        self.calls
            .borrow()
            .iter()
            .filter(|called| **called == callback)
            .count()
    }
}

impl<L: Loader> Loader for std::rc::Rc<L> {
    fn call(&self, callback: &'static str, args: Vec<Value>) -> Result<Value, CallError> {
        (**self).call(callback, args)
    }
}

impl Loader for FakeLoader {
    fn call(&self, callback: &'static str, args: Vec<Value>) -> Result<Value, CallError> {
        self.calls.borrow_mut().push(callback);
        (self.handler.borrow_mut())(callback, &args)
    }
}

pub(crate) struct DebugLogs {
    _guard: tracing::subscriber::DefaultGuard,
    _other: tracing::Dispatch,
}

// With a single dispatcher registered, tracing-core caches a callsite's interest from the
// default subscriber of whichever thread reaches it first, so a parallel test without
// this subscriber can turn this test's debug callsites off. A second registered
// dispatcher makes tracing-core ask every registered dispatcher instead.
pub(crate) fn debug_logs() -> DebugLogs {
    DebugLogs {
        _other: tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default()),
        _guard: tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_test_writer()
            .set_default(),
    }
}
