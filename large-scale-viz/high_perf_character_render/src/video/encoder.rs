use anyhow::{Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};

pub struct ProResEncoder {
    value_process: Child,
    mask_process: Child,
    value_stdin: Option<ChildStdin>,
    mask_stdin: Option<ChildStdin>,
    width: u32,
    height: u32,
    value_buffer: Vec<u8>,
    mask_buffer: Vec<u8>,
}

impl ProResEncoder {
    /// Create a new encoder (value + mask) that streams to two files
    pub fn new<P: AsRef<Path>>(
        output_path: P,
        width: u32,
        height: u32,
        fps: u32,
    ) -> Result<Self> {
        let output_path = output_path.as_ref();

        // Create two output paths: _value.mov and _mask.mov
        let mut value_path = PathBuf::from(output_path);
        let mut mask_path = PathBuf::from(output_path);

        let stem = output_path.file_stem().unwrap().to_str().unwrap();
        value_path.set_file_name(format!("{}_value.mov", stem));
        mask_path.set_file_name(format!("{}_mask.mov", stem));

        log::info!(
            "Starting dual ProRes 422 HQ encoders: {}x{} @ {} fps",
            width, height, fps
        );
        log::info!("  value output: {:?}", value_path);
        log::info!("  Mask output: {:?}", mask_path);

        // Build value encoder (ProRes 422 HQ)
        let mut value_process = Command::new("ffmpeg")
            .args(&[
                "-y",
                "-f", "rawvideo",
                "-pixel_format", "gray",
                "-video_size", &format!("{}x{}", width, height),
                "-framerate", &format!("{}", fps),
                "-i", "pipe:0",
                "-c:v", "prores_ks",
                "-profile:v", "3", // ProRes 422 HQ
                "-pix_fmt", "yuv422p10le",
                "-vendor", "apl0",
                "-threads", "12",
                value_path.to_str().unwrap(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .context("Failed to spawn value encoder")?;

        // Build mask encoder (ProRes 422 HQ, grayscale)
        let mut mask_process = Command::new("ffmpeg")
            .args(&[
                "-y",
                "-f", "rawvideo",
                "-pixel_format", "gray",
                "-video_size", &format!("{}x{}", width, height),
                "-framerate", &format!("{}", fps),
                "-i", "pipe:0",
                "-c:v", "prores_ks",
                "-profile:v", "3", // ProRes 422 HQ
                "-pix_fmt", "yuv422p10le",
                "-vendor", "apl0",
                "-threads", "12",
                mask_path.to_str().unwrap(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .context("Failed to spawn mask encoder")?;

        let value_stdin = value_process
            .stdin
            .take()
            .context("Failed to open value encoder stdin")?;

        let mask_stdin = mask_process
            .stdin
            .take()
            .context("Failed to open mask encoder stdin")?;

        let pixel_count = (width * height) as usize;
        let value_buffer = vec![0u8; pixel_count];
        let mask_buffer = vec![0u8; pixel_count];

        Ok(Self {
            value_process,
            mask_process,
            value_stdin: Some(value_stdin),
            mask_stdin: Some(mask_stdin),
            width,
            height,
            value_buffer,
            mask_buffer,
        })
    }

    /// Write a single frame row-major, top-to-bottom
    /// Splits into RGB and alpha mask streams
    pub fn write_frame(&mut self, frame_data: &[u8]) -> Result<()> {
        let expected_size = (self.width * self.height * 4) as usize;
        if frame_data.len() != expected_size {
            anyhow::bail!(
                "Invalid frame size: expected {} bytes, got {}",
                expected_size,
                frame_data.len()
            );
        }

        // Split RGBA into value and alpha channels
        let pixel_count = (self.width * self.height) as usize;

        for i in 0..pixel_count {
            let rgba_idx = i * 4;

            // RGB channels
            let rgb_avg = ((frame_data[rgba_idx] as f32 + frame_data[rgba_idx + 1] as f32 + frame_data[rgba_idx + 2] as f32) / 3.0) as u8;
            self.value_buffer[i] = rgb_avg;
            // Alpha channel
            self.mask_buffer[i] = frame_data[rgba_idx + 3];
        }

        // Write value frame
        if let Some(stdin) = &mut self.value_stdin {
            stdin
                .write_all(&self.value_buffer)
                .context("Failed to write value frame to ffmpeg")?;
        } else {
            anyhow::bail!("value encoder stdin is closed")
        }

        // Write mask frame
        if let Some(stdin) = &mut self.mask_stdin {
            stdin
                .write_all(&self.mask_buffer)
                .context("Failed to write mask frame to ffmpeg")?;
        } else {
            anyhow::bail!("Mask encoder stdin is closed")
        }

        Ok(())
    }

    /// Finish encoding and close both files
    pub fn finish(mut self) -> Result<()> {
        log::info!("Finalizing video encoding...");

        // Close stdin to signal end of input
        drop(self.value_stdin.take());
        drop(self.mask_stdin.take());

        // Wait for both encoders to finish
        let value_status = self
            .value_process
            .wait()
            .context("Failed to wait for value encoder")?;

        let mask_status = self
            .mask_process
            .wait()
            .context("Failed to wait for mask encoder")?;

        if value_status.success() && mask_status.success() {
            log::info!("Both video encodings completed successfully");
            Ok(())
        } else {
            anyhow::bail!(
                "FFmpeg exited with errors - value: {:?}, Mask: {:?}",
                value_status,
                mask_status
            );
        }
    }
}

impl Drop for ProResEncoder {
    fn drop(&mut self) {
        // Try to terminate both encoders if they're still running
        let _ = self.value_process.kill();
        let _ = self.mask_process.kill();
    }
}
