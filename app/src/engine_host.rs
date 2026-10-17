//! Background thread that turns settings into ready-to-run processors. Filter design (HRTF/room
//! synthesis) happens here, never on the audio thread. Rapid changes (slider drags) are coalesced.

use aether_engine::{EngineSettings, FilterCache, Meters, Processor, ProcessorSlot};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

struct Pending {
    settings: EngineSettings,
    format: Option<(f32, usize)>,
    dirty: bool,
}

pub struct EngineHost {
    shared: Arc<(Mutex<Pending>, Condvar)>,
    pub slot: Arc<ProcessorSlot>,
    pub meters: Arc<Meters>,
}

impl EngineHost {
    pub fn spawn() -> Self {
        let shared = Arc::new((Mutex::new(Pending { settings: EngineSettings::default(), format: None, dirty: false }), Condvar::new()));
        let slot = ProcessorSlot::new();
        let meters = Meters::new();
        let (sh, sl) = (shared.clone(), slot.clone());
        std::thread::Builder::new()
            .name("aether-builder".into())
            .spawn(move || {
                let mut cache = FilterCache::default();
                loop {
                    let (settings, format) = {
                        let (lock, cv) = &*sh;
                        let mut p = lock.lock().unwrap();
                        while !p.dirty {
                            let (g, _) = cv.wait_timeout(p, Duration::from_millis(500)).unwrap();
                            p = g;
                            sl.collect_garbage();
                        }
                        p.dirty = false;
                        (p.settings.clone(), p.format)
                    };
                    if let Some((fs, ch)) = format {
                        let proc = Processor::new(&settings, fs, ch, &mut cache);
                        sl.publish(Box::new(proc));
                    }
                    sl.collect_garbage();
                }
            })
            .expect("spawn builder");
        Self { shared, slot, meters }
    }

    fn mark(&self, f: impl FnOnce(&mut Pending)) {
        let (lock, cv) = &*self.shared;
        let mut p = lock.lock().unwrap();
        f(&mut p);
        p.dirty = true;
        cv.notify_one();
    }

    pub fn update(&self, settings: EngineSettings) {
        self.mark(|p| p.settings = settings);
    }

    /// Called when an audio stream (re)starts with a given sample rate and input channel count.
    pub fn set_format(&self, fs: f32, in_ch: usize) {
        self.mark(|p| p.format = Some((fs, in_ch)));
    }
}
