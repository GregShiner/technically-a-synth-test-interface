use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use dasp_signal::Signal;
use eframe::egui;
use ringbuf::{
    HeapRb,
    traits::{Consumer, Observer, Producer, Split},
};
use std::sync::{Arc, Mutex};
use synthesis::{saw_oscillator, sine_oscillator, square_oscillator};

const SAMPLE_RATE_I: u32 = 44100;
const SAMPLE_RATE: f64 = SAMPLE_RATE_I as f64;
const FREQUENCY: f64 = 440.0;
const BUFFER_SIZE: usize = 1024;

struct OscilloscopeApp {
    samples: Arc<Mutex<Vec<f32>>>,
    frequency: f64,
    osc: Box<dyn Signal<Frame = f64>>,
    osc_type: OscillatorType,
    producer: ringbuf::HeapProd<f32>,
}

#[derive(PartialEq, Clone, Copy)]
enum OscillatorType {
    Sine,
    Square,
    Saw,
}

impl OscillatorType {
    fn build(&self, sample_rate: f64, freq: f64) -> Box<dyn Signal<Frame = f64>> {
        match self {
            OscillatorType::Sine => Box::new(sine_oscillator(sample_rate, freq)),
            OscillatorType::Square => Box::new(square_oscillator(sample_rate, freq)),
            OscillatorType::Saw => Box::new(saw_oscillator(sample_rate, freq)),
        }
    }
}

impl eframe::App for OscilloscopeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Technically a Synth");

            let prev_type = self.osc_type;
            let prev_freq = self.frequency;

            ui.horizontal(|ui| {
                ui.radio_value(&mut self.osc_type, OscillatorType::Sine, "Sine");
                ui.radio_value(&mut self.osc_type, OscillatorType::Square, "Square");
                ui.radio_value(&mut self.osc_type, OscillatorType::Saw, "Saw");
            });

            ui.add(
                egui::Slider::new(&mut self.frequency, 20.0..=2000.0)
                    .text("Frequency (Hz)")
                    .logarithmic(true),
            );

            if self.osc_type != prev_type || self.frequency != prev_freq {
                self.osc = self.osc_type.build(SAMPLE_RATE, self.frequency);
            }

            // Fill the ring buffer with fresh samples
            {
                let mut buf = self.samples.lock().unwrap();
                while self.producer.vacant_len() > 0 {
                    let s = self.osc.next() as f32;
                    self.producer.try_push(s).ok();
                    buf.rotate_left(1);
                    *buf.last_mut().unwrap() = s;
                }
            }

            let samples = self.samples.lock().unwrap().clone();

            // Draw the waveform using egui's Painter
            let (response, painter) = ui.allocate_painter(
                egui::vec2(ui.available_width(), 300.0),
                egui::Sense::hover(),
            );

            let rect = response.rect;
            painter.rect_filled(rect, 0.0, egui::Color32::BLACK);

            if samples.len() > 1 {
                let mid_y = rect.center().y;
                let amplitude = rect.height() / 2.0 * 0.8;
                let step = rect.width() / samples.len() as f32;

                let points: Vec<egui::Pos2> = samples
                    .iter()
                    .enumerate()
                    .map(|(i, &s)| egui::pos2(rect.left() + i as f32 * step, mid_y - s * amplitude))
                    .collect();

                for window in points.windows(2) {
                    painter.line_segment(
                        [window[0], window[1]],
                        egui::Stroke::new(1.5, egui::Color32::from_rgb(0, 255, 100)),
                    );
                }
            }
        });

        // Continuously repaint so the oscilloscope updates
        ctx.request_repaint();
    }
}

fn main() {
    let samples = Arc::new(Mutex::new(vec![0.0f32; BUFFER_SIZE]));
    let samples_for_audio = Arc::clone(&samples);

    // Set up cpal audio stream
    let host = cpal::default_host();
    let device = host.default_output_device().expect("no output device");
    let config = cpal::StreamConfig {
        channels: 1,
        sample_rate: SAMPLE_RATE_I,
        buffer_size: cpal::BufferSize::Default,
    };
    // Set up a ring buffer between app and audio thread
    let rb = HeapRb::<f32>::new(BUFFER_SIZE * 4);
    let (mut producer, mut consumer) = rb.split();

    let stream = device
        .build_output_stream(
            &config,
            move |data: &mut [f32], _| {
                let mut buf = samples_for_audio.lock().unwrap();
                for sample in data.iter_mut() {
                    let s = consumer.try_pop().unwrap_or(0.0);
                    *sample = s;
                    // Rolling buffer — shift old samples out
                    buf.rotate_left(1);
                    *buf.last_mut().unwrap() = s;
                }
            },
            |err| eprintln!("audio error: {err}"),
            None,
        )
        .expect("failed to build stream");

    stream.play().expect("failed to play stream");

    // Launch egui window
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([800.0, 400.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Oscilloscope",
        options,
        Box::new(|_cc| {
            Ok(Box::new(OscilloscopeApp {
                samples,
                frequency: FREQUENCY,
                osc: Box::new(sine_oscillator(SAMPLE_RATE, FREQUENCY)),
                osc_type: OscillatorType::Sine,
                producer,
            }))
        }),
    )
    .unwrap();
}
