mod decode;
mod game_action;
mod generated;
pub(crate) mod packets;

use std::{
    cell::{Cell, OnceCell, RefCell},
    collections::VecDeque,
    future::poll_fn,
    rc::{Rc, Weak},
    sync::{self, Arc, Mutex, TryLockError},
    task::{Poll, Waker},
};

use ruffle_core::{
    Player,
    context::UpdateContext,
    external::{CallbackError, ExternalInterfaceProvider},
};

pub(crate) use decode::DecodeError;
use decode::{arg, decode};
pub(crate) use game_action::GameAction;
pub(crate) use generated::*;
pub(crate) use packets::{Format, Packet, Packets};
pub(crate) use ruffle_core::external::Value;

const MAILBOX_CAPACITY: usize = 256;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub(crate) enum CallError {
    #[error("the loader hasn't registered {0}")]
    Unavailable(&'static str),
    #[error("{0} ran before the loader was ready")]
    NotReady(&'static str),
    #[error("{callback} threw: {message}")]
    Threw {
        callback: &'static str,
        message: String,
    },
    #[error("{0} was called while the player was running")]
    Reentrant(&'static str),
    #[error("the player has stopped")]
    Stopped,
    #[error(transparent)]
    Decode(#[from] DecodeError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("loader events were dropped because the subscriber fell behind")]
pub(crate) struct Lagged;

pub(crate) trait Loader {
    fn call(&self, callback: &'static str, args: Vec<Value>) -> Result<Value, CallError>;
}

#[derive(Clone, Default)]
pub(crate) struct Bridge(Rc<Shared>);

#[derive(Default)]
struct Shared {
    player: OnceCell<sync::Weak<Mutex<Player>>>,
    loaded: Cell<bool>,
    events: Events,
}

impl Bridge {
    pub(crate) fn attach(&self, player: &Arc<Mutex<Player>>) {
        let _ = self.0.player.set(Arc::downgrade(player));
    }

    pub(crate) fn loaded(&self) -> bool {
        self.0.loaded.get()
    }

    pub(crate) fn subscribe(&self, wants: impl Fn(&Event) -> bool + 'static) -> Subscription {
        self.0.events.subscribe(wants)
    }
}

#[derive(Default)]
pub(crate) struct Events(RefCell<Vec<Weak<Mailbox>>>);

impl Events {
    pub(crate) fn subscribe(&self, wants: impl Fn(&Event) -> bool + 'static) -> Subscription {
        let mailbox = Rc::new(Mailbox {
            wants: Box::new(wants),
            state: RefCell::default(),
        });
        self.0.borrow_mut().push(Rc::downgrade(&mailbox));
        Subscription(mailbox)
    }

    pub(crate) fn publish(&self, event: &Event) {
        let mailboxes: Vec<_> = {
            let mut mailboxes = self.0.borrow_mut();
            mailboxes.retain(|mailbox| mailbox.strong_count() > 0);
            mailboxes.iter().filter_map(Weak::upgrade).collect()
        };
        for mailbox in mailboxes {
            mailbox.deliver(event);
        }
    }
}

impl Loader for Bridge {
    fn call(&self, callback: &'static str, args: Vec<Value>) -> Result<Value, CallError> {
        let player = self
            .0
            .player
            .get()
            .and_then(sync::Weak::upgrade)
            .ok_or(CallError::Stopped)?;
        let mut player = player.try_lock().map_err(|error| match error {
            TryLockError::WouldBlock => CallError::Reentrant(callback),
            TryLockError::Poisoned(_) => CallError::Stopped,
        })?;
        player.mutate_with_update_context(|context| {
            let function = context
                .external_interface
                .get_callback(callback)
                .ok_or(CallError::Unavailable(callback))?;
            function
                .call_result(context, callback, args)
                .map_err(|error| match error {
                    CallbackError::NotReady => CallError::NotReady(callback),
                    CallbackError::Thrown(message) | CallbackError::ReplyConversion(message) => {
                        CallError::Threw { callback, message }
                    }
                })
        })
    }
}

impl ExternalInterfaceProvider for Bridge {
    fn call_method(&self, _context: &mut UpdateContext<'_>, name: &str, args: &[Value]) -> Value {
        match Event::parse(name, args) {
            Ok(Some(event)) => {
                if event == Event::Loaded {
                    self.0.loaded.set(true);
                }
                self.0.events.publish(&event);
            }
            Ok(None) => tracing::debug!(callback = name, "unhandled loader call"),
            Err(error) => tracing::warn!(%error, "couldn't read a loader event"),
        }
        Value::Undefined
    }

    fn on_callback_available(&self, name: &str) {
        tracing::debug!(callback = name, "loader callback registered");
    }

    fn get_id(&self) -> Option<String> {
        None
    }
}

pub(crate) struct Subscription(Rc<Mailbox>);

impl Subscription {
    pub(crate) async fn next(&self) -> Result<Event, Lagged> {
        poll_fn(|cx| {
            let mut state = self.0.state.borrow_mut();
            if std::mem::take(&mut state.lagged) {
                return Poll::Ready(Err(Lagged));
            }
            if let Some(event) = state.events.pop_front() {
                return Poll::Ready(Ok(event));
            }
            state.waker = Some(cx.waker().clone());
            Poll::Pending
        })
        .await
    }

    pub(crate) fn clear(&self) {
        let mut state = self.0.state.borrow_mut();
        state.events.clear();
        state.lagged = false;
    }
}

struct Mailbox {
    wants: Box<dyn Fn(&Event) -> bool>,
    state: RefCell<MailboxState>,
}

#[derive(Default)]
struct MailboxState {
    events: VecDeque<Event>,
    lagged: bool,
    waker: Option<Waker>,
}

impl Mailbox {
    fn deliver(&self, event: &Event) {
        if !(self.wants)(event) {
            return;
        }
        let mut state = self.state.borrow_mut();
        if state.events.len() == MAILBOX_CAPACITY {
            state.events.clear();
            state.lagged = true;
        } else {
            state.events.push_back(event.clone());
        }
        if let Some(waker) = state.waker.take() {
            drop(state);
            waker.wake();
        }
    }
}
#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        pin::pin,
        sync::atomic::{AtomicUsize, Ordering},
        task::{Context, Wake},
    };

    use ruffle_core::{PlayerBuilder, tag_utils::SwfMovie};
    use swf::avm1::types::{Action, DefineFunction, Push, Value as Avm1};

    use super::*;

    struct Recorder {
        calls: RefCell<Vec<(&'static str, Vec<Value>)>>,
        reply: Value,
    }

    impl Loader for Recorder {
        fn call(&self, callback: &'static str, args: Vec<Value>) -> Result<Value, CallError> {
            self.calls.borrow_mut().push((callback, args));
            Ok(self.reply.clone())
        }
    }

    fn replying(value: Value) -> Recorder {
        Recorder {
            calls: RefCell::default(),
            reply: value,
        }
    }

    #[test]
    fn omitted_arguments_send_the_loader_defaults() {
        let loader = replying(Value::Bool(true));
        player::join_map(&loader, "battleon", None, None).unwrap();
        player::join_map(&loader, "battleon", Some("r2"), Some("Left")).unwrap();
        world::set_spawn_point(&loader, None, None).unwrap();
        shops::sell(&loader, Value::String("Sword".into()), None).unwrap();
        assert_eq!(
            loader.calls.into_inner(),
            vec![
                (
                    "player.joinMap",
                    vec!["battleon".into(), "Enter".into(), "Spawn".into()]
                ),
                (
                    "player.joinMap",
                    vec!["battleon".into(), "r2".into(), "Left".into()]
                ),
                ("world.setSpawnPoint", vec![Value::Null, Value::Null]),
                ("shops.sell", vec!["Sword".into(), Value::Number(-1.0)]),
            ]
        );
    }

    #[test]
    fn an_omitted_number_defaults_to_nan() {
        let loader = replying(Value::Bool(true));
        player::walk_to(&loader, 10, 20, None).unwrap();
        let calls = loader.calls.into_inner();
        assert!(matches!(
            calls.as_slice(),
            [("player.walkTo", args)] if matches!(
                args.as_slice(),
                [Value::Number(10.0), Value::Number(20.0), Value::Number(speed)] if speed.is_nan()
            )
        ));
    }

    #[test]
    fn game_actions_are_sent_with_the_game_spelling() {
        let loader = replying(Value::Bool(true));
        assert_eq!(
            world::is_action_available(&loader, GameAction::TryQuestComplete),
            Ok(true)
        );
        assert_eq!(
            loader.calls.into_inner(),
            [("world.isActionAvailable", vec!["tryQuestComplete".into()])]
        );
    }

    #[test]
    fn replies_decode_to_the_declared_type() {
        assert_eq!(auth::is_logged_in(&replying(Value::Bool(true))), Ok(true));
        assert_eq!(flash::get_conn_mc_text(&replying(Value::Null)), Ok(None));
        assert!(matches!(
            auth::is_logged_in(&replying(Value::String("yes".into()))),
            Err(CallError::Decode(_))
        ));
        assert!(matches!(
            bank::get_slots(&replying(Value::Number(1.5))),
            Err(CallError::Decode(_))
        ));
    }

    #[test]
    fn loader_events_parse_by_name() {
        assert_eq!(
            Event::parse("onConnection", &["OnConnectionLost".into()]),
            Ok(Some(Event::Connection {
                status: "OnConnectionLost".into()
            }))
        );
        assert_eq!(Event::parse("onLoaded", &[]), Ok(Some(Event::Loaded)));
        assert_eq!(Event::parse("somethingElse", &[]), Ok(None));
        assert!(Event::parse("onConnection", &[]).is_err());
    }

    fn text(value: &str) -> &swf::SwfStr {
        swf::SwfStr::from_utf8_str(value)
    }

    fn push(values: Vec<Avm1<'_>>) -> Action<'_> {
        Action::Push(Push { values })
    }

    fn external_interface() -> [Action<'static>; 6] {
        [
            push(vec![Avm1::Str(text("flash"))]),
            Action::GetVariable,
            push(vec![Avm1::Str(text("external"))]),
            Action::GetMember,
            push(vec![Avm1::Str(text("ExternalInterface"))]),
            Action::GetMember,
        ]
    }

    fn bytes(actions: &[Action<'_>]) -> Vec<u8> {
        let mut output = Vec::new();
        let mut writer = swf::avm1::write::Writer::new(&mut output, 10);
        for action in actions {
            writer.write_action(action).unwrap();
        }
        output
    }

    fn calling<'a>(method: &'a str, mut args: Vec<Action<'a>>, count: i32) -> Vec<Action<'a>> {
        args.push(push(vec![Avm1::Int(count)]));
        args.extend(external_interface());
        args.extend([
            push(vec![Avm1::Str(text(method))]),
            Action::CallMethod,
            Action::Pop,
        ]);
        args
    }

    fn fixture_swf() -> Vec<u8> {
        let echo = bytes(&[
            push(vec![Avm1::Str(text("x"))]),
            Action::GetVariable,
            Action::Return,
        ]);
        let throw = bytes(&[push(vec![Avm1::Str(text("boom"))]), Action::Throw]);
        let mut raise = calling(
            "call",
            vec![
                push(vec![Avm1::Str(text("status"))]),
                Action::GetVariable,
                push(vec![Avm1::Str(text("onConnection"))]),
            ],
            2,
        );
        raise.extend([push(vec![Avm1::Bool(true)]), Action::Return]);
        let raise = bytes(&raise);
        let mut main = Vec::new();
        for (name, params, body) in [
            ("probe.echo", vec![text("x")], echo.as_slice()),
            ("probe.throw", vec![], throw.as_slice()),
            ("probe.raise", vec![text("status")], raise.as_slice()),
        ] {
            main.push(Action::DefineFunction(DefineFunction {
                name: text(""),
                params,
                actions: body,
            }));
            main.extend(calling(
                "addCallback",
                vec![push(vec![Avm1::Null, Avm1::Str(text(name))])],
                3,
            ));
        }
        main.extend(calling(
            "call",
            vec![push(vec![Avm1::Str(text("onLoaded"))])],
            1,
        ));
        main.push(Action::Stop);
        let main = bytes(&main);
        let header = swf::Header {
            compression: swf::Compression::None,
            version: 10,
            stage_size: swf::Rectangle {
                x_min: swf::Twips::ZERO,
                x_max: swf::Twips::from_pixels(100.0),
                y_min: swf::Twips::ZERO,
                y_max: swf::Twips::from_pixels(100.0),
            },
            frame_rate: swf::Fixed8::from_f32(24.0),
            num_frames: 1,
        };
        let mut output = Vec::new();
        swf::write_swf(
            &header,
            &[swf::Tag::DoAction(&main), swf::Tag::ShowFrame],
            &mut output,
        )
        .unwrap();
        output
    }

    fn fixture() -> (Arc<Mutex<Player>>, Bridge) {
        let movie = SwfMovie::from_data(
            &fixture_swf(),
            "file:///lyra/fixture.swf".into(),
            None,
            None,
        )
        .unwrap();
        let bridge = Bridge::default();
        let player = PlayerBuilder::new()
            .with_movie(movie)
            .with_external_interface(Box::new(bridge.clone()))
            .build();
        bridge.attach(&player);
        player.lock().unwrap().run_frame();
        (player, bridge)
    }

    fn ready<T>(future: impl Future<Output = T>) -> Option<T> {
        match pin!(future).poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(value) => Some(value),
            Poll::Pending => None,
        }
    }

    fn connection(status: &str) -> Event {
        Event::Connection {
            status: status.into(),
        }
    }

    #[test]
    fn calls_reach_loader_callbacks_and_report_their_failures() {
        let (_player, bridge) = fixture();
        assert!(bridge.loaded());
        assert_eq!(
            bridge.call("probe.echo", vec!["hello".into()]),
            Ok("hello".into())
        );
        assert_eq!(
            bridge.call("probe.throw", vec![]),
            Err(CallError::Threw {
                callback: "probe.throw",
                message: "boom".into()
            })
        );
        assert_eq!(
            bridge.call("probe.missing", vec![]),
            Err(CallError::Unavailable("probe.missing"))
        );
    }

    #[test]
    fn a_call_after_the_player_is_gone_reports_that_it_stopped() {
        let (player, bridge) = fixture();
        drop(player);
        assert_eq!(
            bridge.call("probe.echo", vec!["hello".into()]),
            Err(CallError::Stopped)
        );
    }

    #[test]
    fn events_raised_during_a_call_reach_matching_subscribers_in_order() {
        let (_player, bridge) = fixture();
        let connections = bridge.subscribe(|event| matches!(event, Event::Connection { .. }));
        let debug = bridge.subscribe(|event| matches!(event, Event::Debug { .. }));
        bridge
            .call("probe.raise", vec!["OnConnection".into()])
            .unwrap();
        bridge
            .call("probe.raise", vec!["OnConnectionLost".into()])
            .unwrap();
        assert_eq!(
            ready(connections.next()),
            Some(Ok(connection("OnConnection")))
        );
        assert_eq!(
            ready(connections.next()),
            Some(Ok(connection("OnConnectionLost")))
        );
        assert_eq!(ready(connections.next()), None);
        assert_eq!(ready(debug.next()), None);
    }

    #[test]
    fn a_subscriber_that_falls_behind_is_told_once_and_then_resumes() {
        let (_player, bridge) = fixture();
        let events = bridge.subscribe(|_| true);
        for _ in 0..=MAILBOX_CAPACITY {
            bridge.call("probe.raise", vec!["old".into()]).unwrap();
        }
        assert_eq!(ready(events.next()), Some(Err(Lagged)));
        assert_eq!(ready(events.next()), None);
        bridge.call("probe.raise", vec!["new".into()]).unwrap();
        assert_eq!(ready(events.next()), Some(Ok(connection("new"))));
    }

    #[test]
    fn a_waiting_subscriber_is_woken_when_its_event_arrives() {
        struct Count(AtomicUsize);
        impl Wake for Count {
            fn wake(self: Arc<Self>) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let (_player, bridge) = fixture();
        let events = bridge.subscribe(|event| matches!(event, Event::Connection { .. }));
        let count = Arc::new(Count(AtomicUsize::new(0)));
        let waker = Waker::from(count.clone());
        let mut next = pin!(events.next());
        assert!(
            next.as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
        bridge
            .call("probe.raise", vec!["OnConnectionLost".into()])
            .unwrap();
        assert_eq!(count.0.load(Ordering::SeqCst), 1);
        assert_eq!(
            next.poll(&mut Context::from_waker(&waker)),
            Poll::Ready(Ok(connection("OnConnectionLost")))
        );
    }
}
