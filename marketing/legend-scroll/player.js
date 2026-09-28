/* Live preview: plays the scroll against the synthesized score and narration, audio-clocked. */
(function () {
  'use strict';

  if (new URLSearchParams(location.search).has('export')) {
    document.body.classList.add('export');
    return;
  }

  const trailer = window.LegendTrailer;
  const canvas = document.getElementById('screen');
  const context = canvas.getContext('2d');
  const playButton = document.getElementById('play');
  const scrub = document.getElementById('scrub');
  const clock = document.getElementById('clock');
  const status = document.getElementById('status');

  let audioContext = null;
  let scoreBuffer = null;
  const voiceBuffers = [];
  let sources = [];
  let playing = false;
  let startedAt = 0;
  let position = 0;

  function draw(time) {
    position = Math.min(trailer.DURATION, Math.max(0, time));
    trailer.renderFrame(context, position);
    clock.textContent = position.toFixed(2);
    scrub.value = String(position);
  }

  const now = () => (playing ? audioContext.currentTime - startedAt : position);

  function tick() {
    if (!playing) return;
    const time = now();
    if (time >= trailer.DURATION) {
      stop();
      draw(trailer.DURATION - 0.001);
      return;
    }
    draw(time);
    requestAnimationFrame(tick);
  }

  function schedule(buffer, at, offset, gain = 1) {
    const source = audioContext.createBufferSource();
    source.buffer = buffer;
    const volume = audioContext.createGain();
    volume.gain.value = gain;
    source.connect(volume).connect(audioContext.destination);
    const startIn = at - offset;
    if (startIn >= 0) source.start(audioContext.currentTime + startIn);
    else if (-startIn < buffer.duration) source.start(0, -startIn);
    else return;
    sources.push(source);
  }

  function start() {
    if (!scoreBuffer) return;
    audioContext.resume();
    const offset = position >= trailer.DURATION - 0.01 ? 0 : position;
    schedule(scoreBuffer, 0, offset, 0.8);
    voiceBuffers.forEach(({ buffer, entry }) => schedule(buffer, entry.start, offset, 1.3));
    startedAt = audioContext.currentTime - offset;
    playing = true;
    playButton.textContent = 'Pause';
    requestAnimationFrame(tick);
  }

  function stop() {
    sources.forEach((source) => {
      try {
        source.stop();
      } catch {
        /* already finished */
      }
    });
    sources = [];
    position = now();
    playing = false;
    playButton.textContent = 'Play';
  }

  playButton.addEventListener('click', () => (playing ? stop() : start()));
  scrub.addEventListener('input', () => {
    const wasPlaying = playing;
    if (wasPlaying) stop();
    draw(Number(scrub.value));
    if (wasPlaying) start();
  });
  window.addEventListener('keydown', (event) => {
    if (event.code === 'Space') {
      event.preventDefault();
      playing ? stop() : start();
    }
  });

  trailer.init().then(async () => {
    draw(0);
    audioContext = new AudioContext({ sampleRate: window.LegendScore.SAMPLE_RATE });
    status.textContent = 'composing the score…';
    await new Promise((resolve) => setTimeout(resolve, 30));
    const mix = window.LegendScore.synthesize(trailer.cues(), audioContext.sampleRate);
    scoreBuffer = audioContext.createBuffer(2, mix.length, mix.sampleRate);
    scoreBuffer.copyToChannel(mix.left, 0);
    scoreBuffer.copyToChannel(mix.right, 1);
    // Narration clips load over http(s); under file:// browsers block fetch, so the
    // preview plays score-only there (the exported MP4 always carries the voice).
    await Promise.all(trailer.NARRATION.map(async (entry) => {
      try {
        const response = await fetch(entry.file);
        voiceBuffers.push({ entry, buffer: await audioContext.decodeAudioData(await response.arrayBuffer()) });
      } catch {
        /* file:// preview: score only */
      }
    }));
    status.textContent = voiceBuffers.length ? 'ready · space to play' : 'ready · space to play (serve over http for narration)';
  });
})();
