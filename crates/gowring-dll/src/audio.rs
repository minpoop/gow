//! hooks.audio_out — sound effects and Mimir's voice, played from the converted cache on a worker
//! thread (a failure here never reaches the game thread).

use rodio::{Decoder, OutputStream, Sink, Source};
use std::collections::{HashMap, VecDeque};
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Default, Debug)]
pub struct VoiceState {
    pub set: String,
    pub stem: String,
    pub until: Option<Instant>,
    pub subtitle: Option<String>,
}

enum Msg {
    Sfx { path: PathBuf, volume: f32 },
    Voice { dir: PathBuf, set: String, no_repeat: usize, interrupt: bool, volume: f32 },
}

pub struct Audio {
    tx: Sender<Msg>,
    pub voice: Arc<Mutex<VoiceState>>,
}

fn pick(dir: &PathBuf, recent: &mut VecDeque<String>, no_repeat: usize, rng: &mut u32) -> Option<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir).ok()?.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().map(|x| x == "wav").unwrap_or(false)).collect();
    files.sort();
    if files.is_empty() {
        return None;
    }
    let fresh: Vec<&PathBuf> = files.iter().filter(|p| !recent.contains(&p.file_stem().unwrap().to_string_lossy().into_owned())).collect();
    let pool: Vec<&PathBuf> = if fresh.is_empty() { files.iter().collect() } else { fresh };
    *rng ^= *rng << 13;
    *rng ^= *rng >> 17;
    *rng ^= *rng << 5;
    let chosen = pool[(*rng as usize) % pool.len()].clone();
    recent.push_back(chosen.file_stem().unwrap().to_string_lossy().into_owned());
    while recent.len() > no_repeat.max(1) {
        recent.pop_front();
    }
    Some(chosen)
}

fn subtitles(cache: &PathBuf) -> HashMap<String, String> {
    std::fs::read_to_string(cache.join("text").join("subtitles.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<HashMap<String, String>>(&t).ok())
        .unwrap_or_default()
}

impl Audio {
    pub fn start(cache: PathBuf) -> Audio {
        let (tx, rx) = channel::<Msg>();
        let voice = Arc::new(Mutex::new(VoiceState::default()));
        let vshared = voice.clone();
        std::thread::Builder::new()
            .name("gowring-audio".into())
            .spawn(move || {
                let Ok((_stream, handle)) = OutputStream::try_default() else {
                    crate::glog!("audio", "no audio output device; sound disabled");
                    // drain messages so senders never block
                    for _ in rx {}
                    return;
                };
                crate::glog!("audio", "output ready");
                let subs = subtitles(&cache);
                let mut voice_sink: Option<Sink> = None;
                let mut recent: HashMap<String, VecDeque<String>> = HashMap::new();
                let mut rng: u32 = 0x9E37_79B9;
                for msg in rx {
                    match msg {
                        Msg::Sfx { path, volume } => match std::fs::File::open(&path).ok().and_then(|f| Decoder::new(BufReader::new(f)).ok()) {
                            Some(dec) => {
                                if let Ok(sink) = Sink::try_new(&handle) {
                                    sink.set_volume(volume);
                                    sink.append(dec);
                                    sink.detach();
                                }
                            }
                            None => crate::log::once("sfx-missing", "audio", &format!("cannot play {}", path.display())),
                        },
                        Msg::Voice { dir, set, no_repeat, interrupt, volume } => {
                            let playing = voice_sink.as_ref().map(|s| !s.empty()).unwrap_or(false);
                            if playing && !interrupt {
                                continue;
                            }
                            if let Some(s) = voice_sink.take() {
                                s.stop();
                            }
                            let r = recent.entry(set.clone()).or_default();
                            let Some(path) = pick(&dir, r, no_repeat, &mut rng) else {
                                crate::log::once("voice-empty", "audio", &format!("no Mimir lines in {} (prepare did not find them)", dir.display()));
                                continue;
                            };
                            let Some(dec) = std::fs::File::open(&path).ok().and_then(|f| Decoder::new(BufReader::new(f)).ok()) else {
                                crate::glog!("audio", "cannot decode {}", path.display());
                                continue;
                            };
                            let dur = dec.total_duration().unwrap_or(Duration::from_secs(4));
                            let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
                            if let Ok(sink) = Sink::try_new(&handle) {
                                sink.set_volume(volume);
                                sink.append(dec);
                                voice_sink = Some(sink);
                                let mut v = vshared.lock().unwrap();
                                *v = VoiceState { set, stem: stem.clone(), until: Some(Instant::now() + dur), subtitle: subs.get(&stem).cloned() };
                                crate::glog!("audio", "Mimir line {stem} ({:.1}s)", dur.as_secs_f32());
                            }
                        }
                    }
                }
            })
            .ok();
        Audio { tx, voice }
    }

    pub fn sfx(&self, path: PathBuf, volume: f32) {
        let _ = self.tx.send(Msg::Sfx { path, volume });
    }

    pub fn voice(&self, dir: PathBuf, set: &str, no_repeat: usize, interrupt: bool, volume: f32) {
        let _ = self.tx.send(Msg::Voice { dir, set: set.to_string(), no_repeat, interrupt, volume });
    }

    /// Remaining seconds of the current line and its subtitle, if one is playing.
    pub fn current(&self) -> Option<(String, f32, Option<String>)> {
        let v = self.voice.lock().ok()?;
        let until = v.until?;
        let now = Instant::now();
        (until > now).then(|| (v.set.clone(), (until - now).as_secs_f32(), v.subtitle.clone()))
    }
}
