//! Microphone capture with cpal into an in-memory f32 buffer.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use std::sync::{Arc, Mutex};

pub struct Recording {
    stream: cpal::Stream,
    buf: Arc<Mutex<Vec<f32>>>,
    pub channels: u16,
    pub rate: u32,
}

pub fn start() -> Result<Recording, String> {
    let device = cpal::default_host()
        .default_input_device()
        .ok_or("Mikrofoni ei leitud")?;
    let cfg = device
        .default_input_config()
        .map_err(|e| format!("Mikrofon: {e}"))?;
    let buf = Arc::new(Mutex::new(Vec::new()));
    let (channels, rate, format) = (cfg.channels(), cfg.sample_rate(), cfg.sample_format());
    let config: cpal::StreamConfig = cfg.into();
    let stream = match format {
        SampleFormat::F32 => build::<f32>(&device, &config, buf.clone()),
        SampleFormat::I16 => build::<i16>(&device, &config, buf.clone()),
        SampleFormat::U16 => build::<u16>(&device, &config, buf.clone()),
        SampleFormat::I32 => build::<i32>(&device, &config, buf.clone()),
        f => return Err(format!("Toetamata helivorming: {f}")),
    }?;
    stream.play().map_err(|e| format!("Mikrofon: {e}"))?;
    Ok(Recording {
        stream,
        buf,
        channels,
        rate,
    })
}

fn build<S>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    buf: Arc<Mutex<Vec<f32>>>,
) -> Result<cpal::Stream, String>
where
    S: SizedSample,
    f32: FromSample<S>,
{
    device
        .build_input_stream(
            *config,
            move |data: &[S], _: &cpal::InputCallbackInfo| {
                if let Ok(mut b) = buf.lock() {
                    b.extend(data.iter().map(|s| s.to_sample::<f32>()));
                }
            },
            |_| {},
            None,
        )
        .map_err(|e| format!("Mikrofon: {e}"))
}

impl Recording {
    /// Seconds of audio captured so far.
    pub fn seconds(&self) -> f32 {
        let n = self.buf.lock().map(|b| b.len()).unwrap_or(0);
        n as f32 / (self.rate as f32 * self.channels.max(1) as f32)
    }

    /// Stop the stream and return the interleaved samples.
    pub fn finish(self) -> Vec<f32> {
        drop(self.stream);
        std::mem::take(&mut *self.buf.lock().unwrap_or_else(|p| p.into_inner()))
    }
}
