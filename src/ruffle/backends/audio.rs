use std::any::Any;

use ruffle_core::{
    FloatDuration, Player,
    backend::audio::{
        AudioBackend, DecodeError, NullAudioBackend, RegisterError, SoundHandle,
        SoundInstanceHandle, SoundStreamInfo, SoundTransform, Substream, swf,
    },
    tag_utils::SwfSlice,
};
use ruffle_frontend_utils::backends::audio::CpalAudioBackend;

pub(crate) fn audio() -> Audio {
    let output: Box<dyn AudioBackend> = match CpalAudioBackend::new(None) {
        Ok(audio) => Box::new(audio),
        Err(error) => {
            tracing::warn!(%error, "no audio output, the player runs silent");
            Box::new(NullAudioBackend::new())
        }
    };
    Audio::new(output)
}

pub(crate) fn follow_game_volume(player: &mut Player) {
    player.mutate_with_update_context(|context| {
        let silent = context.audio_manager.global_sound_transform().volume == 0;
        if let Some(audio) = (context.audio as &mut dyn Any).downcast_mut::<Audio>() {
            audio.silence(silent);
        }
    });
}

pub(crate) struct Audio {
    output: Box<dyn AudioBackend>,
    playing: bool,
    silent: bool,
    streams: Vec<SoundInstanceHandle>,
}

impl Audio {
    fn new(output: Box<dyn AudioBackend>) -> Self {
        Self {
            output,
            playing: true,
            silent: false,
            streams: Vec::new(),
        }
    }

    fn silence(&mut self, silent: bool) {
        if silent == self.silent {
            return;
        }
        self.silent = silent;
        if silent {
            // Ruffle holds a clip's timeline back to stay in sync with its stream, so a paused
            // stream would stall the game.
            for stream in self.streams.drain(..) {
                self.output.stop_sound(stream);
            }
            self.output.pause();
        } else if self.playing {
            self.output.play();
        }
    }

    fn track_stream(&mut self, stream: SoundInstanceHandle) -> SoundInstanceHandle {
        if self.silent {
            self.output.stop_sound(stream);
        } else {
            let output = &self.output;
            self.streams.retain(|&other| output.is_sound_playing(other));
            self.streams.push(stream);
        }
        stream
    }
}

impl AudioBackend for Audio {
    fn play(&mut self) {
        self.playing = true;
        if !self.silent {
            self.output.play();
        }
    }

    fn pause(&mut self) {
        self.playing = false;
        self.output.pause();
    }

    fn register_sound(&mut self, swf_sound: &swf::Sound) -> Result<SoundHandle, RegisterError> {
        self.output.register_sound(swf_sound)
    }

    fn register_mp3(&mut self, data: &[u8]) -> Result<SoundHandle, DecodeError> {
        self.output.register_mp3(data)
    }

    fn unregister_sound(&mut self, sound: SoundHandle) {
        self.output.unregister_sound(sound);
    }

    fn start_sound(
        &mut self,
        sound: SoundHandle,
        settings: &swf::SoundInfo,
    ) -> Result<SoundInstanceHandle, DecodeError> {
        let instance = self.output.start_sound(sound, settings)?;
        if self.silent && settings.num_loops <= 1 {
            self.output.stop_sound(instance);
        }
        Ok(instance)
    }

    fn start_stream(
        &mut self,
        clip_data: SwfSlice,
        handle: &swf::SoundStreamHead,
    ) -> Result<SoundInstanceHandle, DecodeError> {
        let stream = self.output.start_stream(clip_data, handle)?;
        Ok(self.track_stream(stream))
    }

    fn start_substream(
        &mut self,
        stream_data: Substream,
        stream_info: &SoundStreamInfo,
    ) -> Result<SoundInstanceHandle, DecodeError> {
        let stream = self.output.start_substream(stream_data, stream_info)?;
        Ok(self.track_stream(stream))
    }

    fn stop_sound(&mut self, sound: SoundInstanceHandle) {
        self.output.stop_sound(sound);
    }

    fn stop_all_sounds(&mut self) {
        self.output.stop_all_sounds();
    }

    fn get_sound_position(&self, instance: SoundInstanceHandle) -> Option<f64> {
        self.output.get_sound_position(instance)
    }

    fn get_sound_duration(&self, sound: SoundHandle) -> Option<FloatDuration> {
        self.output.get_sound_duration(sound)
    }

    fn get_sound_size(&self, sound: SoundHandle) -> Option<u32> {
        self.output.get_sound_size(sound)
    }

    fn get_sound_format(&self, sound: SoundHandle) -> Option<&swf::SoundFormat> {
        self.output.get_sound_format(sound)
    }

    fn set_sound_transform(&mut self, instance: SoundInstanceHandle, transform: SoundTransform) {
        self.output.set_sound_transform(instance, transform);
    }

    fn get_sound_peak(&mut self, instance: SoundInstanceHandle) -> Option<[f32; 2]> {
        self.output.get_sound_peak(instance)
    }

    fn volume(&self) -> f32 {
        self.output.volume()
    }

    fn set_volume(&mut self, volume: f32) {
        self.output.set_volume(volume);
    }

    fn get_sample_history(&self) -> [[f32; 2]; 1024] {
        self.output.get_sample_history()
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc, sync::Arc};

    use ruffle_core::{backend::audio::AudioMixer, impl_audio_mixer_backend, tag_utils::SwfMovie};

    use super::*;

    struct Speaker {
        mixer: AudioMixer,
        playing: Rc<Cell<bool>>,
    }

    impl AudioBackend for Speaker {
        impl_audio_mixer_backend!(mixer);

        fn play(&mut self) {
            self.playing.set(true);
        }

        fn pause(&mut self) {
            self.playing.set(false);
        }
    }

    #[test]
    fn a_muted_game_stops_its_output_and_resumes_looping_sounds_when_it_unmutes() {
        let playing = Rc::new(Cell::new(true));
        let mut audio = Audio::new(Box::new(Speaker {
            mixer: AudioMixer::new(2, 44_100),
            playing: playing.clone(),
        }));
        let format = swf::SoundFormat {
            compression: swf::AudioCompression::UncompressedUnknownEndian,
            sample_rate: 44_100,
            is_stereo: false,
            is_16_bit: false,
        };
        let samples = [0u8; 4_410];
        let sound = audio
            .register_sound(&swf::Sound {
                id: 1,
                format: format.clone(),
                num_samples: 4_410,
                data: &samples,
            })
            .unwrap();
        let play = |num_loops| swf::SoundInfo {
            event: swf::SoundEvent::Event,
            in_sample: None,
            out_sample: None,
            num_loops,
            envelope: None,
        };
        let head = swf::SoundStreamHead {
            stream_format: format.clone(),
            playback_format: format,
            num_samples_per_block: 441,
            latency_seek: 0,
        };
        let clip = SwfSlice::empty(Arc::new(SwfMovie::empty(10, None)));

        let music = audio.start_sound(sound, &play(9999)).unwrap();
        let effect = audio.start_sound(sound, &play(1)).unwrap();
        let stream = audio.start_stream(clip.clone(), &head).unwrap();
        assert!(audio.is_sound_playing(stream));

        audio.silence(true);
        assert!(!playing.get());
        assert!(audio.is_sound_playing(music));
        assert!(audio.is_sound_playing(effect));
        assert!(!audio.is_sound_playing(stream));
        let muted_music = audio.start_sound(sound, &play(9999)).unwrap();
        let muted_effect = audio.start_sound(sound, &play(1)).unwrap();
        let muted_stream = audio.start_stream(clip, &head).unwrap();
        assert!(audio.is_sound_playing(muted_music));
        assert!(!audio.is_sound_playing(muted_effect));
        assert!(!audio.is_sound_playing(muted_stream));

        audio.play();
        assert!(!playing.get());

        audio.silence(false);
        assert!(playing.get());
        assert!(audio.is_sound_playing(music));
        assert!(audio.is_sound_playing(muted_music));
        let after = audio.start_sound(sound, &play(1)).unwrap();
        assert!(audio.is_sound_playing(after));
    }
}
