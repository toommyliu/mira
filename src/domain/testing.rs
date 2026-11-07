use std::cell::RefCell;

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

impl Loader for FakeLoader {
    fn call(&self, callback: &'static str, args: Vec<Value>) -> Result<Value, CallError> {
        self.calls.borrow_mut().push(callback);
        (self.handler.borrow_mut())(callback, &args)
    }
}
