//! Account-free, unpaced one-hour synthetic PCM decode. No media files or audio output.
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
struct Wave {
    position: u64,
    length: u64,
    header: [u8; 44],
    max_read: Arc<AtomicUsize>,
}
impl Wave {
    fn new(max_read: Arc<AtomicUsize>) -> Self {
        let data = 48000_u32 * 2 * 2 * 3600;
        let mut header = [0; 44];
        header[0..4].copy_from_slice(b"RIFF");
        header[4..8].copy_from_slice(&(data + 36).to_le_bytes());
        header[8..16].copy_from_slice(b"WAVEfmt ");
        header[16..20].copy_from_slice(&16_u32.to_le_bytes());
        header[20..22].copy_from_slice(&1_u16.to_le_bytes());
        header[22..24].copy_from_slice(&2_u16.to_le_bytes());
        header[24..28].copy_from_slice(&48000_u32.to_le_bytes());
        header[28..32].copy_from_slice(&192000_u32.to_le_bytes());
        header[32..34].copy_from_slice(&4_u16.to_le_bytes());
        header[34..36].copy_from_slice(&16_u16.to_le_bytes());
        header[36..40].copy_from_slice(b"data");
        header[40..44].copy_from_slice(&data.to_le_bytes());
        Self {
            position: 0,
            length: u64::from(data) + 44,
            header,
            max_read,
        }
    }
}
impl Read for Wave {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        self.max_read.fetch_max(output.len(), Ordering::Relaxed);
        let count = output
            .len()
            .min(self.length.saturating_sub(self.position) as usize);
        output[..count].fill(0);
        let header_count = count.min(44_u64.saturating_sub(self.position) as usize);
        if header_count > 0 {
            output[..header_count].copy_from_slice(
                &self.header[self.position as usize..self.position as usize + header_count],
            );
        }
        self.position += count as u64;
        Ok(count)
    }
}
impl Seek for Wave {
    fn seek(&mut self, offset: SeekFrom) -> io::Result<u64> {
        let position = match offset {
            SeekFrom::Start(n) => i128::from(n),
            SeekFrom::End(n) => i128::from(self.length) + i128::from(n),
            SeekFrom::Current(n) => i128::from(self.position) + i128::from(n),
        };
        self.position =
            u64::try_from(position).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        Ok(self.position)
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let max_read = Arc::new(AtomicUsize::new(0));
    let input = Wave::new(max_read.clone());
    let length = input.length;
    let decoder = rodio::Decoder::builder()
        .with_data(input)
        .with_byte_len(length)
        .with_seekable(true)
        .with_hint("wav")
        .build()?;
    let started = std::time::Instant::now();
    let mut samples = 0_u64;
    for sample in decoder {
        if sample != 0.0 {
            return Err("Unexpected synthetic PCM value".into());
        }
        samples += 1;
        if samples.is_multiple_of(48000 * 2 * 600) {
            println!("decoded_seconds={}", samples / (48000 * 2));
        }
    }
    if samples != 48000 * 2 * 3600 {
        return Err("Synthetic stream ended early".into());
    }
    let request = max_read.load(Ordering::Relaxed);
    if request > 1024 * 1024 {
        return Err("Decoder requested an unexpectedly large input buffer".into());
    }
    println!(
        "synthetic_hour=passed virtual_bytes={length} max_read_request={request} elapsed_ms={}",
        started.elapsed().as_millis()
    );
    Ok(())
}
