/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::{sync::Arc, time::Duration};

/// One interleaved decoded-audio buffer with the timing and layout facts a
/// host-owned audio runtime needs to place it accurately.
#[derive(Clone, Debug, PartialEq)]
pub struct DecodedAudioChunk {
    samples: Arc<[f32]>,
    sample_rate: u32,
    channels: u32,
    channel_positions: Arc<[u32]>,
    presentation_time: Option<Duration>,
}

impl DecodedAudioChunk {
    pub fn new(
        samples: Arc<[f32]>,
        sample_rate: u32,
        channels: u32,
        channel_positions: Arc<[u32]>,
        presentation_time: Option<Duration>,
    ) -> Self {
        debug_assert!(sample_rate > 0);
        debug_assert!(channels > 0);
        debug_assert_eq!(channels as usize, channel_positions.len());
        Self {
            samples,
            sample_rate,
            channels,
            channel_positions,
            presentation_time,
        }
    }

    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channels(&self) -> u32 {
        self.channels
    }

    pub fn channel_positions(&self) -> &[u32] {
        &self.channel_positions
    }

    pub fn presentation_time(&self) -> Option<Duration> {
        self.presentation_time
    }
}

pub trait AudioRenderer: Send + 'static {
    fn render(&mut self, sample: Box<dyn AsRef<[f32]>>, channel: u32);

    /// Render one whole decoded buffer. New host integrations should override
    /// this method. The default preserves the historical per-channel callback
    /// so existing renderers continue to work while consumers migrate.
    fn render_chunk(&mut self, chunk: DecodedAudioChunk) {
        for channel in chunk.channel_positions.iter().copied() {
            self.render(Box::new(chunk.samples.clone()), channel);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AudioRenderer, DecodedAudioChunk};
    use std::{sync::Arc, time::Duration};

    #[derive(Default)]
    struct LegacyRenderer {
        calls: Vec<(Vec<f32>, u32)>,
    }

    impl AudioRenderer for LegacyRenderer {
        fn render(&mut self, sample: Box<dyn AsRef<[f32]>>, channel: u32) {
            self.calls
                .push((sample.as_ref().as_ref().to_vec(), channel));
        }
    }

    #[test]
    fn decoded_chunk_adapts_to_legacy_channel_callbacks() {
        let mut renderer = LegacyRenderer::default();
        renderer.render_chunk(DecodedAudioChunk::new(
            Arc::from([0.25, -0.25, 0.5, -0.5]),
            48_000,
            2,
            Arc::from([1, 2]),
            Some(Duration::from_millis(125)),
        ));

        assert_eq!(renderer.calls.len(), 2);
        assert_eq!(renderer.calls[0].0, [0.25, -0.25, 0.5, -0.5]);
        assert_eq!(renderer.calls[0].1, 1);
        assert_eq!(renderer.calls[1].1, 2);
    }

    #[test]
    fn decoded_chunk_retains_capture_grade_facts() {
        let chunk = DecodedAudioChunk::new(
            Arc::from([0.0, 0.5]),
            44_100,
            1,
            Arc::from([4]),
            Some(Duration::from_secs_f64(2.25)),
        );

        assert_eq!(chunk.samples(), [0.0, 0.5]);
        assert_eq!(chunk.sample_rate(), 44_100);
        assert_eq!(chunk.channels(), 1);
        assert_eq!(chunk.channel_positions(), [4]);
        assert_eq!(
            chunk.presentation_time(),
            Some(Duration::from_secs_f64(2.25))
        );
    }
}
