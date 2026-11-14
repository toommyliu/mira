use serde::Deserialize;
use serde_json::Value as Json;

use super::calls::{CallId, Replier};

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    tag = "op",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum Request {
    Alert {
        message: String,
        title: Option<String>,
    },
    Confirm {
        message: String,
        title: Option<String>,
        confirm: Option<String>,
        cancel: Option<String>,
    },
    Prompt {
        message: String,
        title: Option<String>,
        default: Option<String>,
        placeholder: Option<String>,
    },
}

pub(crate) enum ToDialogs {
    Open(Dialog),
    Close(CallId),
}

pub(crate) struct Dialog {
    pub(crate) id: CallId,
    pub(crate) request: Request,
    reply: Replier,
}

impl Dialog {
    pub(super) fn new(id: CallId, request: Request, reply: Replier) -> Self {
        Self { id, request, reply }
    }

    pub(crate) fn accept(self, entered: &str) {
        let value = match self.request {
            Request::Alert { .. } => Json::Null,
            Request::Confirm { .. } => Json::Bool(true),
            Request::Prompt { .. } => Json::from(entered),
        };
        self.reply.send(Ok(value));
    }

    pub(crate) fn cancel(self) {
        let value = match self.request {
            Request::Alert { .. } | Request::Prompt { .. } => Json::Null,
            Request::Confirm { .. } => Json::Bool(false),
        };
        self.reply.send(Ok(value));
    }
}

#[cfg(test)]
impl Dialog {
    pub(crate) fn test(id: u64, request: Request) -> (Self, impl Fn() -> Option<Json>) {
        let (commands, replies) = std::sync::mpsc::channel();
        let id = CallId(id);
        let answer = move || match replies.try_recv() {
            Ok(super::runtime::Command::Reply {
                reply: Ok(value), ..
            }) => Some(value),
            _ => None,
        };
        (Self::new(id, request, Replier::new(id, commands)), answer)
    }
}
