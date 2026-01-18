use anyhow::{Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};

pub struct ProResEncoder {
    rgb_process: Child,
    mask_process: Child,
    rgb_stdin: Option<ChildStdin>,
    mask_stdin: Option<ChildStdin>,
    width: u32,
    height: u32,
    rgb_buffer: Vec<u8>,
    mask_buffer: Vec<u8>,
}

impl ProResEncoder {
    /// Create a new dual H.264 encoder (RGB + mask) that streams to two files
    pub fn new<P: AsRef<Path>>(
        output_path: P,
        width: u32,
        height: u32,
        fps: u32,
    ) -> Result<Self> {
        let output_path = output_path.as_ref();

        // Create two output paths: _rgb.mp4 and _mask.mp4
        let mut rgb_path = PathBuf::from(output_path);
        let mut mask_path = PathBuf::from(output_path);

        let stem = output_path.file_stem().unwrap().to_str().unwrap();
        rgb_path.set_file_name(format!("{}_rgb.mp4", stem));
        mask_path.set_file_name(format!("{}_mask.mp4", stem));

        log::info!(
            "Starting dual H.264 encoders: {}x{} @ {} fps",
            width, height, fps
        );
        log::info!("  RGB output: {:?}", rgb_path);
        log::info!("  Mask output: {:?}", mask_path);

        // Build RGB encoder (H.264, high quality)
        let mut rgb_process = Command::new("ffmpeg")
            .args(&[
                "-y",
                "-f", "rawvideo",
                "-pixel_format", "rgb24",
                "-video_size", &format!("{}x{}", width, height),
                "-framerate", &format!("{}", fps),
                "-i", "pipe:0",
                "-c:v", "libx264",
                "-preset", "slow",
                "-crf", "15", // Near-lossless quality (comparable to ProRes 4444)
                "-pix_fmt", "yuv444p", // 4:4:4 chroma subsampling for max quality
                "-threads", "8",
                rgb_path.to_str().unwrap(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .context("Failed to spawn RGB encoder")?;

        // Build mask encoder (H.264, grayscale)
        let mut mask_process = Command::new("ffmpeg")
            .args(&[
                "-y",
                "-f", "rawvideo",
                "-pixel_format", "gray",
                "-video_size", &format!("{}x{}", width, height),
                "-framerate", &format!("{}", fps),
                "-i", "pipe:0",
                "-c:v", "libx264",
                "-preset", "slow",
                "-crf", "15",
                "-pix_fmt", "yuv420p",
                "-threads", "8",
                mask_path.to_str().unwrap(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .context("Failed to spawn mask encoder")?;

        let rgb_stdin = rgb_process
            .stdin
            .take()
            .context("Failed to open RGB encoder stdin")?;

        let mask_stdin = mask_process
            .stdin
            .take()
            .context("Failed to open mask encoder stdin")?;

        let pixel_count = (width * height) as usize;
        let rgb_buffer = vec![0u8; pixel_count * 3];
        let mask_buffer = vec![0u8; pixel_count];

        Ok(Self {
            rgb_process,
            mask_process,
            rgb_stdin: Some(rgb_stdin),
            mask_stdin: Some(mask_stdin),
            width,
            height,
            rgb_buffer,
            mask_buffer,
        })
    }

    /// Write a single frame (RGBA8, row-major, top-to-bottom)
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

        // Split RGBA into RGB and alpha channels
        let pixel_count = (self.width * self.height) as usize;

        for i in 0..pixel_count {
            let rgba_idx = i * 4;
            let rgb_idx = i * 3;

            // RGB channels
            self.rgb_buffer[rgb_idx] = frame_data[rgba_idx];
            self.rgb_buffer[rgb_idx + 1] = frame_data[rgba_idx + 1];
            self.rgb_buffer[rgb_idx + 2] = frame_data[rgba_idx + 2];

            // Alpha channel
            self.mask_buffer[i] = frame_data[rgba_idx + 3];
        }

        // Write RGB frame
        if let Some(stdin) = &mut self.rgb_stdin {
            stdin
                .write_all(&self.rgb_buffer)
                .context("Failed to write RGB frame to ffmpeg")?;
        } else {
            anyhow::bail!("RGB encoder stdin is closed")
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
        drop(self.rgb_stdin.take());
        drop(self.mask_stdin.take());

        // Wait for both encoders to finish
        let rgb_status = self
            .rgb_process
            .wait()
            .context("Failed to wait for RGB encoder")?;

        let mask_status = self
            .mask_process
            .wait()
            .context("Failed to wait for mask encoder")?;

        if rgb_status.success() && mask_status.success() {
            log::info!("Both video encodings completed successfully");
            Ok(())
        } else {
            anyhow::bail!(
                "FFmpeg exited with errors - RGB: {:?}, Mask: {:?}",
                rgb_status,
                mask_status
            );
        }
    }
}

impl Drop for ProResEncoder {
    fn drop(&mut self) {
        // Try to terminate both encoders if they're still running
        let _ = self.rgb_process.kill();
        let _ = self.mask_process.kill();
    }
}
