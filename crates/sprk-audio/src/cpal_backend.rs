//! cpal implementation of [`Backend`] (WASAPI on Windows). The System source opens the default
//! render device as an input, which cpal turns into WASAPI loopback on Windows.
//! The callback only pushes into a lock-free ring buffer: no allocation, no locks.
//! Compiled with the `cpal-backend` feature.
use crate::live::{Backend, PcmStream};
use crate::{AudioError, LinearResampler, Source};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::traits::{Consumer, Producer, Split};
use ringbuf::HeapRb;

pub struct CpalBackend {
    host: cpal::Host,
}

impl Default for CpalBackend {
    fn default() -> Self {
        Self {
            host: cpal::default_host(),
        }
    }
}

struct CpalStream {
    // Dropping the stream stops capture.
    _stream: cpal::Stream,
    cons: ringbuf::HeapCons<f32>,
    resampler: LinearResampler,
    scratch: Vec<f32>,
}

impl PcmStream for CpalStream {
    fn read(&mut self, out: &mut Vec<f32>) {
        let n = self.cons.pop_slice(&mut self.scratch);
        if n > 0 {
            out.extend(self.resampler.process(&self.scratch[..n]));
        }
    }
}

impl Backend for CpalBackend {
    fn default_device(&self, source: Source) -> Option<String> {
        let d = match source {
            Source::Mic => self.host.default_input_device(),
            Source::System => self.host.default_output_device(),
        };
        d.and_then(|d| d.name().ok())
    }

    fn open(&mut self, source: Source, name: &str) -> Result<Box<dyn PcmStream>, AudioError> {
        let device = match source {
            Source::Mic => self
                .host
                .input_devices()
                .ok()
                .and_then(|mut it| it.find(|d| d.name().is_ok_and(|n| n == name))),
            Source::System => self
                .host
                .output_devices()
                .ok()
                .and_then(|mut it| it.find(|d| d.name().is_ok_and(|n| n == name))),
        }
        .ok_or(AudioError::Unavailable)?;
        let cfg = match source {
            Source::Mic => device.default_input_config(),
            // For loopback the render format is the capture format.
            Source::System => device.default_output_config(),
        }
        .map_err(|_| AudioError::Unavailable)?;
        let (rate, channels) = (cfg.sample_rate().0, usize::from(cfg.channels()).max(1));
        let (mut prod, cons) = HeapRb::<f32>::new(rate as usize * 10).split();
        let err_fn = |_e| {}; // errors carry device text; the poll loop notices a dead device instead
        let stream = match cfg.sample_format() {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &cfg.config(),
                move |data: &[f32], _| {
                    for f in data.chunks_exact(channels) {
                        let _ = prod.try_push(f.iter().sum::<f32>() / channels as f32);
                    }
                },
                err_fn,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_input_stream(
                &cfg.config(),
                move |data: &[i16], _| {
                    for f in data.chunks_exact(channels) {
                        let s: f32 = f.iter().map(|x| f32::from(*x) / 32768.0).sum();
                        let _ = prod.try_push(s / channels as f32);
                    }
                },
                err_fn,
                None,
            ),
            _ => return Err(AudioError::Unavailable),
        }
        .map_err(|_| AudioError::Unavailable)?;
        stream.play().map_err(|_| AudioError::Unavailable)?;
        Ok(Box::new(CpalStream {
            _stream: stream,
            cons,
            resampler: LinearResampler::new(rate, 16_000),
            scratch: vec![0.0; 8192],
        }))
    }
}
