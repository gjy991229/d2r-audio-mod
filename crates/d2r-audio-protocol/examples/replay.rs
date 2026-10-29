//! Replay diagnostics through the same streaming path as the monitor.
//! cargo run --release --example replay -- recording.wav [...]
use d2r_audio_protocol::protocol::{interleaved_i32_to_mono, StreamingDetector};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    for path in std::env::args().skip(1) {
        let mut reader = hound::WavReader::open(&path)?;
        let spec = reader.spec();
        if spec.sample_format != hound::SampleFormat::Int {
            return Err("expected integer PCM diagnostic WAV".into());
        }
        let samples = reader.samples::<i32>().collect::<Result<Vec<_>, _>>()?;
        let mono = interleaved_i32_to_mono(
            &samples,
            spec.channels as usize,
            spec.bits_per_sample as u32,
        )?;
        let mut detector = StreamingDetector::new(spec.sample_rate, 0.56)?;
        let began = std::time::Instant::now();
        let mut detections = Vec::new();
        let mut max_chunk_ms = 0.0f64;
        for chunk in mono.chunks(spec.sample_rate as usize / 10) {
            let start = std::time::Instant::now();
            detections.extend(detector.push(chunk));
            max_chunk_ms = max_chunk_ms.max(start.elapsed().as_secs_f64() * 1000.0);
        }
        println!(
            "{}",
            serde_json::json!({"path":path,
            "audio_seconds":mono.len() as f64/spec.sample_rate as f64,
            "elapsed_seconds":began.elapsed().as_secs_f64(),"max_chunk_ms":max_chunk_ms,
            "detections":detections})
        );
    }
    Ok(())
}
