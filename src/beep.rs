use std::{
    f64::consts::TAU,
    num::NonZeroU32,
    sync::{
        Arc, Mutex, OnceLock, PoisonError,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tokio::sync::oneshot;

const REPEAT_INTERVAL: f64 = 0.28;
const RING: f64 = 1.05;
const SILENCE: f64 = 0.0001;
const MASTER_GAIN: f64 = 0.5;
const ECHO_DELAY: f64 = 0.1;
const ECHO_CUTOFF: f64 = 4_500.0;
const WEB_AUDIO_DEFAULT_LOWPASS_Q_DB: f64 = 1.0;
const ECHO_FEEDBACK: f64 = 0.22;
const ECHO_WET: f64 = 0.16;
const MAX_CHIMES: usize = 16;
const IDLE_CHECK: Duration = Duration::from_millis(250);

struct Note {
    frequency: f64,
    offset: f64,
    attack: f64,
    decay: f64,
    peak: f64,
}

impl Note {
    fn length(&self) -> f64 {
        self.attack + self.decay
    }

    fn sample(&self, since_start: f64) -> f64 {
        let local = since_start - self.offset;
        if !(0.0..self.length()).contains(&local) {
            return 0.0;
        }
        let level = if local < self.attack {
            SILENCE * (self.peak / SILENCE).powf(local / self.attack)
        } else {
            self.peak * (SILENCE / self.peak).powf((local - self.attack) / self.decay)
        };
        level * (TAU * self.frequency * local).sin()
    }
}

const NOTES: [Note; 3] = [
    Note {
        frequency: 880.0,
        offset: 0.0,
        attack: 0.004,
        decay: 0.09,
        peak: 0.06,
    },
    Note {
        frequency: 1_108.73,
        offset: 0.06,
        attack: 0.004,
        decay: 0.1,
        peak: 0.06,
    },
    Note {
        frequency: 1_318.51,
        offset: 0.12,
        attack: 0.004,
        decay: 0.18,
        peak: 0.07,
    },
];

fn notes_length() -> f64 {
    NOTES
        .iter()
        .map(|note| note.offset + note.length())
        .fold(0.0, f64::max)
}

struct Lowpass {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl Lowpass {
    fn new(cutoff: f64, rate: f64) -> Self {
        let w0 = TAU * cutoff / rate;
        let alpha = w0.sin() / (2.0 * 10f64.powf(WEB_AUDIO_DEFAULT_LOWPASS_Q_DB / 20.0));
        let cos = w0.cos();
        let a0 = 1.0 + alpha;
        Self {
            b0: (1.0 - cos) / 2.0 / a0,
            b1: (1.0 - cos) / a0,
            b2: (1.0 - cos) / 2.0 / a0,
            a1: -2.0 * cos / a0,
            a2: (1.0 - alpha) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn process(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        (self.x2, self.x1) = (self.x1, x);
        (self.y2, self.y1) = (self.y1, y);
        y
    }
}

struct Echo {
    line: Vec<f64>,
    at: usize,
    filter: Lowpass,
}

impl Echo {
    fn new(rate: f64) -> Self {
        Self {
            line: vec![0.0; (ECHO_DELAY * rate).round() as usize],
            at: 0,
            filter: Lowpass::new(ECHO_CUTOFF, rate),
        }
    }

    fn process(&mut self, input: f64) -> f64 {
        let echoed = self.filter.process(self.line[self.at]);
        self.line[self.at] = input + ECHO_FEEDBACK * echoed;
        self.at = (self.at + 1) % self.line.len();
        echoed
    }
}

struct Chime {
    times: u32,
    rate: f64,
    frame: u64,
    notes_end: u64,
    end: u64,
    echo: Echo,
    done: Option<oneshot::Sender<()>>,
}

impl Chime {
    fn new(times: NonZeroU32, rate: u32, done: oneshot::Sender<()>) -> Self {
        let rate = f64::from(rate);
        let last = f64::from(times.get() - 1) * REPEAT_INTERVAL;
        Self {
            times: times.get(),
            rate,
            frame: 0,
            notes_end: ((last + notes_length()) * rate).ceil() as u64,
            end: ((last + RING) * rate).ceil() as u64,
            echo: Echo::new(rate),
            done: Some(done),
        }
    }

    fn interrupted(&self) -> bool {
        self.done.as_ref().is_some_and(oneshot::Sender::is_closed)
    }

    fn finished(&self) -> bool {
        self.frame >= self.end
    }

    fn next(&mut self) -> f64 {
        if self.finished() {
            return 0.0;
        }
        let t = self.frame as f64 / self.rate;
        let latest = ((t / REPEAT_INTERVAL) as u32).min(self.times - 1);
        let dry = MASTER_GAIN
            * (latest.saturating_sub(1)..=latest)
                .map(|repetition| {
                    let since_start = t - f64::from(repetition) * REPEAT_INTERVAL;
                    NOTES
                        .iter()
                        .map(|note| note.sample(since_start))
                        .sum::<f64>()
                })
                .sum::<f64>();
        let sample = dry + ECHO_WET * self.echo.process(dry);
        self.frame += 1;
        if self.frame >= self.notes_end
            && let Some(done) = self.done.take()
        {
            let _ = done.send(());
        }
        sample
    }
}

struct Request {
    times: NonZeroU32,
    done: oneshot::Sender<()>,
}

pub(crate) async fn beep(times: NonZeroU32) {
    let Some(requests) = requests() else {
        return;
    };
    let (done, finished) = oneshot::channel();
    let _ = requests.send(Request { times, done });
    let _ = finished.await;
}

fn requests() -> Option<&'static mpsc::Sender<Request>> {
    static REQUESTS: OnceLock<Option<mpsc::Sender<Request>>> = OnceLock::new();
    REQUESTS
        .get_or_init(|| {
            let (requests, inbox) = mpsc::channel();
            thread::Builder::new()
                .name("mira-beep".into())
                .spawn(move || serve(&inbox))
                .inspect_err(|error| tracing::warn!(%error, "couldn't start the beep thread"))
                .ok()
                .map(|_| requests)
        })
        .as_ref()
}

fn serve(inbox: &mpsc::Receiver<Request>) {
    while let Ok(first) = inbox.recv() {
        let output = match Output::open() {
            Ok(output) => output,
            Err(error) => {
                tracing::warn!(%error, "couldn't open an audio output to beep");
                continue;
            }
        };
        output.add(first);
        loop {
            match inbox.recv_timeout(IDLE_CHECK) {
                Ok(request) => output.add(request),
                Err(mpsc::RecvTimeoutError::Timeout) if output.idle() => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
    }
}

struct Output {
    _stream: cpal::Stream,
    rate: u32,
    chimes: Arc<Mutex<Vec<Chime>>>,
    failed: Arc<AtomicBool>,
}

impl Output {
    fn open() -> anyhow::Result<Self> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or_else(|| anyhow::anyhow!("no default output device"))?;
        let supported = device.default_output_config()?;
        anyhow::ensure!(
            supported.sample_format() == cpal::SampleFormat::F32,
            "the output takes {} samples, not f32",
            supported.sample_format()
        );
        let config = supported.config();
        let channels = usize::from(config.channels);
        let chimes = Arc::new(Mutex::new(Vec::<Chime>::with_capacity(MAX_CHIMES)));
        let failed = Arc::new(AtomicBool::new(false));
        let mixing = chimes.clone();
        let failing = failed.clone();
        let stream = device.build_output_stream(
            &config,
            move |data: &mut [f32], _| {
                let mut chimes = mixing.lock().unwrap_or_else(PoisonError::into_inner);
                chimes.retain(|chime| !chime.interrupted());
                for frame in data.chunks_mut(channels) {
                    let sample = chimes.iter_mut().map(Chime::next).sum::<f64>();
                    frame.fill(sample.clamp(-1.0, 1.0) as f32);
                }
                chimes.retain(|chime| !chime.finished());
            },
            move |error| {
                tracing::warn!(%error, "the beep output failed");
                failing.store(true, Ordering::Relaxed);
            },
            None,
        )?;
        stream.play()?;
        Ok(Self {
            _stream: stream,
            rate: config.sample_rate.0,
            chimes,
            failed,
        })
    }

    fn add(&self, request: Request) {
        let chime = Chime::new(request.times, self.rate, request.done);
        let mut chimes = self.chimes.lock().unwrap_or_else(PoisonError::into_inner);
        if chimes.len() < MAX_CHIMES {
            chimes.push(chime);
        }
    }

    fn idle(&self) -> bool {
        self.failed.load(Ordering::Relaxed)
            || self
                .chimes
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn render(times: u32) -> (Vec<f64>, oneshot::Receiver<()>) {
        let (done, finished) = oneshot::channel();
        let mut chime = Chime::new(NonZeroU32::new(times).unwrap(), RATE, done);
        let mut samples = Vec::new();
        while !chime.finished() {
            samples.push(chime.next());
        }
        (samples, finished)
    }

    fn loudest(samples: &[f64], from: f64, to: f64) -> f64 {
        let at = |seconds: f64| ((seconds * f64::from(RATE)) as usize).min(samples.len());
        samples[at(from)..at(to)]
            .iter()
            .fold(0.0, |loudest, sample| sample.abs().max(loudest))
    }

    #[test]
    fn each_repetition_chimes_and_the_echo_rings_quieter_until_the_chime_ends() {
        let (samples, _) = render(3);
        assert_eq!(
            samples.len(),
            ((0.56 + RING) * f64::from(RATE)).ceil() as usize
        );
        let note = loudest(&samples, 0.0, 0.01);
        for start in [0.28, 0.56] {
            assert!(
                loudest(&samples, start, start + 0.01) >= note * 0.9,
                "{start}"
            );
        }
        let echo = loudest(&samples, 0.56 + notes_length() + 0.05, 1.0);
        assert!(echo > 0.0 && echo < note / 2.0, "echo {echo} note {note}");
        assert!(samples.iter().all(|sample| sample.abs() <= 1.0));
    }

    #[test]
    fn a_chime_reports_done_when_its_last_notes_end_and_not_before() {
        let (done, mut finished) = oneshot::channel();
        let mut chime = Chime::new(NonZeroU32::new(2).unwrap(), RATE, done);
        let notes_end = ((REPEAT_INTERVAL + notes_length()) * f64::from(RATE)).ceil() as usize;
        for _ in 1..notes_end {
            chime.next();
        }
        assert!(finished.try_recv().is_err());
        chime.next();
        assert_eq!(finished.try_recv(), Ok(()));
        assert!(!chime.finished());
    }

    #[test]
    fn a_chime_is_interrupted_once_nobody_waits_for_it() {
        let (done, finished) = oneshot::channel();
        let chime = Chime::new(NonZeroU32::MIN, RATE, done);
        assert!(!chime.interrupted());
        drop(finished);
        assert!(chime.interrupted());
    }
}
