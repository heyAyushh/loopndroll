/* Live preview: plays the trailer against the synthesized score, audio-clocked. */
(function () {
  'use strict';

  const exporting = new URLSearchParams(location.search).has('export');
  if (exporting) {
    document.body.classList.add('export');
    return;
  }

  const trailer = window.LooperTrailer;
  const canvas = document.getElementById('screen');
  const context = canvas.getContext('2d');
  const playButton = document.getElementById('play');
  const scrub = document.getElementById('scrub');
  const clock = document.getElementById('clock');
  const status = document.getElementById('status');

  let audioContext = null;
  let scoreBuffer = null;
  let source = null;
  let playing = false;
  let startedAt = 0;
  let position = 0;

  function draw(time) {
    position = Math.min(trailer.DURATION, Math.max(0, time));
    trailer.renderFrame(context, position);
    clock.textContent = position.toFixed(2);
    scrub.value = String(position);
  }

  function currentTime() {
    return playing ? audioContext.currentTime - startedAt : position;
  }

  function tick() {
    if (!playing) return;
    const time = currentTime();
    if (time >= trailer.DURATION) {
      stop();
      draw(trailer.DURATION - 0.001);
      return;
    }
    draw(time);
    requestAnimationFrame(tick);
  }

  function start() {
    if (!scoreBuffer) return;
    audioContext.resume();
    source = audioContext.createBufferSource();
    source.buffer = scoreBuffer;
    source.connect(audioContext.destination);
    const offset = position >= trailer.DURATION - 0.01 ? 0 : position;
    source.start(0, offset);
    startedAt = audioContext.currentTime - offset;
    playing = true;
    playButton.textContent = 'Pause';
    requestAnimationFrame(tick);
  }

  function stop() {
    if (source) source.stop();
    source = null;
    position = currentTime();
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

  trailer.init().then(() => {
    draw(0);
    setTimeout(() => {
      audioContext = new AudioContext({ sampleRate: window.LooperTrailerAudio.SAMPLE_RATE });
      const mix = window.LooperTrailerAudio.synthesize(trailer.CUES, audioContext.sampleRate);
      scoreBuffer = audioContext.createBuffer(2, mix.length, mix.sampleRate);
      scoreBuffer.copyToChannel(mix.left, 0);
      scoreBuffer.copyToChannel(mix.right, 1);
      status.textContent = 'ready · space to play';
    }, 30);
  });
})();
