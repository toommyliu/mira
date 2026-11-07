use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use rquickjs::{
    CatchResultExt, CaughtError, Context, Ctx, Exception, Function, Module, Object, Persistent,
    Promise, Runtime, function::Opt, promise::PromiseState,
};

use super::{
    calls::{self, Call, CallId, Replier, Reply, Request, RunId, ToPlayer},
    limits::{Clock, Limits, Tripped},
    modules::{BOOTSTRAP, ModuleLoader, ModuleResolver, Modules, PRELUDE},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    Completed,
    Failed {
        message: String,
        stack: Option<String>,
    },
    Stopped,
    Killed(Limit),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Limit {
    Turn,
    Memory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

pub(crate) type Log = Box<dyn Fn(Level, &str) + Send>;

pub(crate) type Player = Box<dyn Fn(ToPlayer) + Send>;

pub(crate) enum Command {
    Stop,
    Reply { id: CallId, reply: Reply },
}

#[derive(Clone)]
struct Link {
    run: RunId,
    player: Rc<dyn Fn(ToPlayer)>,
    commands: mpsc::Sender<Command>,
}

pub(crate) struct Run {
    commands: mpsc::Sender<Command>,
    clock: Arc<Clock>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Run {
    pub(crate) fn start(
        entry: PathBuf,
        limits: Limits,
        log: Log,
        player: Player,
        finish: impl FnOnce(Outcome) + Send + 'static,
    ) -> std::io::Result<Self> {
        static RUNS: AtomicU64 = AtomicU64::new(0);
        let run = RunId(RUNS.fetch_add(1, Ordering::Relaxed));
        let (commands, inbox) = mpsc::channel();
        let replies = commands.clone();
        let clock = Clock::new();
        let shared = clock.clone();
        let thread = thread::Builder::new()
            .name("lyra-script".into())
            .spawn(move || {
                let link = Link {
                    run,
                    player: Rc::new(player),
                    commands: replies,
                };
                let outcome = execute(&entry, limits, &shared, &inbox, log, link.clone());
                (link.player)(ToPlayer::Ended(run));
                finish(outcome);
            })?;
        Ok(Self {
            commands,
            clock,
            thread: Some(thread),
        })
    }

    pub(crate) fn stop(&self) {
        self.clock.request_stop();
        let _ = self.commands.send(Command::Stop);
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        self.clock.stop_now();
        let _ = self.commands.send(Command::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

enum Phase {
    Importing(Persistent<Promise<'static>>),
    Running(Persistent<Function<'static>>),
}

type Settled = Rc<RefCell<Option<Outcome>>>;

fn execute(
    entry: &Path,
    limits: Limits,
    clock: &Arc<Clock>,
    commands: &mpsc::Receiver<Command>,
    log: Log,
    link: Link,
) -> Outcome {
    let root = match entry.parent().map(std::fs::canonicalize) {
        Some(Ok(root)) => root,
        _ => return failed(format!("{} doesn't exist", entry.display())),
    };
    let modules = Modules::new(root);
    let entry = match modules.entry(entry) {
        Ok(entry) => entry,
        Err(message) => return failed(message),
    };
    let Ok(runtime) = Runtime::new() else {
        return failed("couldn't start the script runtime".into());
    };
    runtime.set_memory_limit(limits.memory);
    runtime.set_max_stack_size(limits.stack);
    runtime.set_interrupt_handler(Some(clock.interrupt_handler(&limits)));
    runtime.set_loader(
        ModuleResolver(modules.clone()),
        ModuleLoader(modules.clone()),
    );
    let Ok(context) = Context::full(&runtime) else {
        return failed("couldn't start the script runtime".into());
    };
    let settled: Settled = Rc::default();
    let started = Instant::now();
    let setup = context.with(|ctx| {
        setup(&ctx, &entry, started, Rc::new(log), link, clock)
            .map_err(|error| describe(error, &modules))
    });
    let (hooks, start, import) = match setup {
        Ok(setup) => setup,
        Err(outcome) => return clock_outcome(clock).unwrap_or(outcome),
    };
    let mut phase = Phase::Importing(import);
    let mut stopping: Option<Instant> = None;
    loop {
        let drained = clock.turn(|| {
            loop {
                match runtime.execute_pending_job() {
                    Ok(true) => {}
                    Ok(false) => return Ok(()),
                    Err(error) => return Err(error),
                }
            }
        });
        if let Err(error) = drained {
            if let Some(outcome) = clock_outcome(clock) {
                return outcome;
            }
            let outcome = error.0.with(|ctx| {
                describe(
                    CaughtError::from_error(&ctx, rquickjs::Error::Exception),
                    &modules,
                )
            });
            return finalize(outcome, stopping.is_some());
        }
        if let Some(outcome) = clock_outcome(clock) {
            return outcome;
        }
        if let Phase::Importing(import) = &phase {
            let next = context.with(|ctx| -> Result<Option<Phase>, Outcome> {
                let promise = import
                    .clone()
                    .restore(&ctx)
                    .map_err(|_| failed("lost the script module".into()))?;
                match promise.state() {
                    PromiseState::Pending => Ok(None),
                    PromiseState::Rejected => Err(describe(
                        promise.result::<()>().unwrap().catch(&ctx).unwrap_err(),
                        &modules,
                    )),
                    PromiseState::Resolved => {
                        let namespace: Object = promise
                            .result()
                            .unwrap()
                            .catch(&ctx)
                            .map_err(|error| describe(error, &modules))?;
                        let exported: rquickjs::Value = namespace
                            .get("default")
                            .unwrap_or_else(|_| rquickjs::Value::new_undefined(ctx.clone()));
                        let start = start
                            .clone()
                            .restore(&ctx)
                            .map_err(|_| failed("lost the bootstrap".into()))?;
                        let settle = settle_function(&ctx, settled.clone(), modules.clone())
                            .map_err(|error| describe(CaughtError::Error(error), &modules))?;
                        let interrupt: Function = clock
                            .turn(|| start.call((exported, settle)))
                            .catch(&ctx)
                            .map_err(|error| describe(error, &modules))?;
                        Ok(Some(Phase::Running(Persistent::save(&ctx, interrupt))))
                    }
                }
            });
            match next {
                Ok(Some(next)) => {
                    phase = next;
                    continue;
                }
                Ok(None) => {}
                Err(outcome) => return clock_outcome(clock).unwrap_or(outcome),
            }
        }
        if let Some(outcome) = settled.borrow_mut().take() {
            return finalize(outcome, stopping.is_some());
        }
        let next_timer = context.with(|ctx| {
            hooks
                .next_timer
                .clone()
                .restore(&ctx)
                .ok()
                .and_then(|next: Function| clock.turn(|| next.call::<_, f64>(())).ok())
        });
        let mut wait = next_timer
            .filter(|ms| *ms >= 0.0)
            .map(|ms| Duration::from_secs_f64(ms / 1000.0));
        if let Some(since) = stopping {
            let left = limits.stop_grace.saturating_sub(since.elapsed());
            if left.is_zero() {
                return Outcome::Stopped;
            }
            wait = Some(wait.map_or(left, |wait| wait.min(left)));
        }
        let command = match wait {
            Some(wait) => commands.recv_timeout(wait),
            None => commands
                .recv()
                .map_err(|_| mpsc::RecvTimeoutError::Disconnected),
        };
        let delivered = match command {
            Ok(Command::Stop) if stopping.is_none() => {
                stopping = Some(Instant::now());
                match &phase {
                    Phase::Running(interrupt) => context.with(|ctx| {
                        if let Ok(interrupt) = interrupt.clone().restore(&ctx) {
                            let _ = clock.turn(|| interrupt.call::<_, ()>(()));
                        }
                    }),
                    Phase::Importing(_) => return Outcome::Stopped,
                }
                Ok(())
            }
            Ok(Command::Reply { id, reply }) => hooks.call(
                &context,
                clock,
                &modules,
                |hooks| &hooks.deliver,
                (id.0, calls::encode(reply)),
            ),
            Ok(Command::Stop) | Err(mpsc::RecvTimeoutError::Timeout) => Ok(()),
            Err(mpsc::RecvTimeoutError::Disconnected) => return Outcome::Stopped,
        };
        let fired = delivered
            .and_then(|()| hooks.call(&context, clock, &modules, |hooks| &hooks.run_timers, ()));
        if let Err(outcome) = fired {
            return clock_outcome(clock).unwrap_or(finalize(outcome, stopping.is_some()));
        }
    }
}

struct Hooks {
    next_timer: Persistent<Function<'static>>,
    run_timers: Persistent<Function<'static>>,
    deliver: Persistent<Function<'static>>,
}

impl Hooks {
    fn call<A: for<'js> rquickjs::function::IntoArgs<'js>>(
        &self,
        context: &Context,
        clock: &Clock,
        modules: &Modules,
        hook: impl Fn(&Self) -> &Persistent<Function<'static>>,
        args: A,
    ) -> Result<(), Outcome> {
        context.with(|ctx| {
            let function = hook(self)
                .clone()
                .restore(&ctx)
                .map_err(|_| failed("lost the runtime hooks".into()))?;
            clock
                .turn(|| function.call::<_, ()>(args))
                .catch(&ctx)
                .map_err(|error| describe(error, modules))
        })
    }
}

fn setup<'js>(
    ctx: &Ctx<'js>,
    entry: &str,
    started: Instant,
    log: Rc<Log>,
    link: Link,
    clock: &Clock,
) -> Result<
    (
        Hooks,
        Persistent<Function<'static>>,
        Persistent<Promise<'static>>,
    ),
    CaughtError<'js>,
> {
    let prelude: Object = Module::import(ctx, PRELUDE)
        .catch(ctx)?
        .finish()
        .catch(ctx)?;
    let install: Function = prelude.get("install").catch(ctx)?;
    let host = Object::new(ctx.clone()).catch(ctx)?;
    host.set(
        "now",
        Function::new(ctx.clone(), move || {
            started.elapsed().as_secs_f64() * 1000.0
        }),
    )
    .catch(ctx)?;
    host.set(
        "log",
        Function::new(ctx.clone(), move |level: String, message: String| {
            let level = match level.as_str() {
                "debug" => Level::Debug,
                "warn" => Level::Warn,
                "error" => Level::Error,
                _ => Level::Info,
            };
            log(level, &message);
        }),
    )
    .catch(ctx)?;
    let calling = link.clone();
    host.set(
        "call",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, id: u64, request: String| -> rquickjs::Result<()> {
                let request: Request = serde_json::from_str(&request).map_err(|error| {
                    Exception::throw_type(&ctx, &format!("Lyra can't send {request}: {error}"))
                })?;
                let id = CallId(id);
                (calling.player)(ToPlayer::Call(Call {
                    run: calling.run,
                    id,
                    request,
                    reply: Replier::new(id, calling.commands.clone()),
                }));
                Ok(())
            },
        ),
    )
    .catch(ctx)?;
    host.set(
        "cancel",
        Function::new(ctx.clone(), move |id: u64| {
            (link.player)(ToPlayer::Cancel {
                run: link.run,
                id: CallId(id),
            })
        }),
    )
    .catch(ctx)?;
    let hooks: Object = install.call((host,)).catch(ctx)?;
    let next: Function = hooks.get("nextTimer").catch(ctx)?;
    let run: Function = hooks.get("runTimers").catch(ctx)?;
    let deliver: Function = hooks.get("deliver").catch(ctx)?;
    let bootstrap: Object = Module::import(ctx, BOOTSTRAP)
        .catch(ctx)?
        .finish()
        .catch(ctx)?;
    let start: Function = bootstrap.get("start").catch(ctx)?;
    let import = clock.turn(|| Module::import(ctx, entry)).catch(ctx)?;
    Ok((
        Hooks {
            next_timer: Persistent::save(ctx, next),
            run_timers: Persistent::save(ctx, run),
            deliver: Persistent::save(ctx, deliver),
        },
        Persistent::save(ctx, start),
        Persistent::save(ctx, import),
    ))
}

fn settle_function<'js>(
    ctx: &Ctx<'js>,
    settled: Settled,
    modules: Modules,
) -> rquickjs::Result<Function<'js>> {
    Function::new(
        ctx.clone(),
        move |kind: String, message: Opt<String>, stack: Opt<String>| {
            let outcome = match kind.as_str() {
                "completed" => Outcome::Completed,
                "stopped" => Outcome::Stopped,
                _ => {
                    let message = message.0.unwrap_or_default();
                    if message == "out of memory" {
                        Outcome::Killed(Limit::Memory)
                    } else {
                        Outcome::Failed {
                            message,
                            stack: stack.0.map(|stack| modules.rewrite_stack(&stack)),
                        }
                    }
                }
            };
            settled.borrow_mut().get_or_insert(outcome);
        },
    )
}

fn describe(error: CaughtError<'_>, modules: &Modules) -> Outcome {
    match error {
        CaughtError::Exception(exception) => {
            let message = exception.message().unwrap_or_default();
            if message == "out of memory" {
                return Outcome::Killed(Limit::Memory);
            }
            Outcome::Failed {
                message,
                stack: exception
                    .stack()
                    .filter(|stack| !stack.trim().is_empty())
                    .map(|stack| modules.rewrite_stack(&stack)),
            }
        }
        CaughtError::Value(value) => failed(format!("{value:?}")),
        CaughtError::Error(rquickjs::Error::Allocation) => Outcome::Killed(Limit::Memory),
        CaughtError::Error(error) => failed(error.to_string()),
    }
}

fn clock_outcome(clock: &Clock) -> Option<Outcome> {
    clock.tripped().map(|tripped| match tripped {
        Tripped::Stop => Outcome::Stopped,
        Tripped::Turn => Outcome::Killed(Limit::Turn),
    })
}

fn finalize(outcome: Outcome, stopping: bool) -> Outcome {
    if stopping { Outcome::Stopped } else { outcome }
}

fn failed(message: String) -> Outcome {
    Outcome::Failed {
        message,
        stack: None,
    }
}
