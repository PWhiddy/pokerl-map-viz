use anyhow::{Context, Result};
use std::io::Write;
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};

pub struct ProResEncoder {
    ffmpeg_process: Child,
    stdin: Option<ChildStdin>,
    width: u32,
    height: u32,
    fps: u32,
}

impl ProResEncoder {
    /// Create a new ProRes 4444 encoder that streams to a file
    pub fn new<P: AsRef<Path>>(
        output_path: P,
        width: u32,
        height: u32,
        fps: u32,
    ) -> Result<Self> {
        log::info!(
            "Starting FFmpeg encoder: {}x{} @ {} fps -> {:?}",
            width,
            height,
            fps,
            output_path.as_ref()
        );

        // Build ffmpeg command
        // Input: raw RGBA frames from stdin
        // Output: ProRes 4444 with alpha channel
        let mut ffmpeg_process = Command::new("ffmpeg")
            .args(&[
                "-y", // Overwrite output file
                "-f",
                "rawvideo",
                "-pixel_format",
                "rgba",
                "-video_size",
                &format!("{}x{}", width, height),
                "-framerate",
                &format!("{}", fps),
                "-i",
                "pipe:0", // Read from stdin
                "-max_interleave_delta",
                "0", // Disable interleaving limit
                "-c:v",
                "prores_ks", // ProRes encoder
                "-profile:v",
                "4444", // ProRes 4444 with alpha
                "-pix_fmt",
                "yuva444p10le", // Pixel format with alpha
                "-vendor",
                "apl0", // Apple vendor ID
                output_path.as_ref().to_str().unwrap(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .context("Failed to spawn ffmpeg process")?;

        let stdin = ffmpeg_process
            .stdin
            .take()
            .context("Failed to open ffmpeg stdin")?;

        Ok(Self {
            ffmpeg_process,
            stdin: Some(stdin),
            width,
            height,
            fps,
        })
    }

    /// Write a single frame (RGBA8, row-major, top-to-bottom)
    pub fn write_frame(&mut self, frame_data: &[u8]) -> Result<()> {
        let expected_size = (self.width * self.height * 4) as usize;
        if frame_data.len() != expected_size {
            anyhow::bail!(
                "Invalid frame size: expected {} bytes, got {}",
                expected_size,
                frame_data.len()
            );
        }

        if let Some(stdin) = &mut self.stdin {
            stdin
                .write_all(frame_data)
                .context("Failed to write frame to ffmpeg")?;
            Ok(())
        } else {
            anyhow::bail!("Encoder stdin is closed")
        }
    }

    /// Finish encoding and close the file
    pub fn finish(mut self) -> Result<()> {
        log::info!("Finalizing video encoding...");

        // Close stdin to signal end of input
        drop(self.stdin.take());

        // Wait for ffmpeg to finish
        let status = self
            .ffmpeg_process
            .wait()
            .context("Failed to wait for ffmpeg process")?;

        if status.success() {
            log::info!("Video encoding completed successfully");
            Ok(())
        } else {
            anyhow::bail!("FFmpeg exited with error: {:?}", status);
        }
    }
}

impl Drop for ProResEncoder {
    fn drop(&mut self) {
        // Try to terminate ffmpeg if it's still running
        let _ = self.ffmpeg_process.kill();
    }
}
