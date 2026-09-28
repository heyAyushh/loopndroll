/* Looper: The Odyssey — the score, synthesized sample by sample.
 *
 * Deterministic (seeded noise, no WebAudio graph), so the browser preview and
 * the exported WAV are bit-identical. Every event time comes from
 * LooperTrailer.CUES, which trailer.js derives from the picture timeline.
 */
(function () {
  'use strict';

  const SAMPLE_RATE = 48000;
  const TWO_PI = Math.PI * 2;
  const MASTER_CEILING = 0.89;
  const DECLICK_SECONDS = 0.003;

  const SHEPARD_PARTIALS = 8;
  const SHEPARD_BASE_HZ = 30;

  function seededRandom(seed) {
    let state = seed >>> 0;
    return function next() {
      state = (state + 0x6d2b79f5) >>> 0;
      let r = Math.imul(state ^ (state >>> 15), 1 | state);
      r = (r + Math.imul(r ^ (r >>> 7), 61 | r)) ^ r;
      return ((r ^ (r >>> 14)) >>> 0) / 4294967296;
    };
  }

  const clamp = (value, low = 0, high = 1) => Math.min(high, Math.max(low, value));
  const progress = (t, start, end) => clamp((t - start) / (end - start));

  /** RBJ biquad band-pass (constant peak gain); returns a stateful filter. */
  function bandPass(frequency, q, sampleRate) {
    const omega = (TWO_PI * frequency) / sampleRate;
    const alpha = Math.sin(omega) / (2 * q);
    const a0 = 1 + alpha;
    const b0 = alpha / a0;
    const b2 = -alpha / a0;
    const a1 = (-2 * Math.cos(omega)) / a0;
    const a2 = (1 - alpha) / a0;
    let x1 = 0;
    let x2 = 0;
    let y1 = 0;
    let y2 = 0;
    return (x) => {
      const y = b0 * x + b2 * x2 - a1 * y1 - a2 * y2;
      x2 = x1;
      x1 = x;
      y2 = y1;
      y1 = y;
      return y;
    };
  }

  function onePoleLowPass(frequency, sampleRate) {
    const k = 1 - Math.exp((-TWO_PI * frequency) / sampleRate);
    let y = 0;
    return (x) => {
      y += k * (x - y);
      return y;
    };
  }

  function createMix(duration, sampleRate) {
    const length = Math.ceil(duration * sampleRate);
    return {
      sampleRate,
      length,
      left: new Float32Array(length),
      right: new Float32Array(length),
      sendLeft: new Float32Array(length),
      sendRight: new Float32Array(length),
    };
  }

  /** Renders `voice(seconds)` into the mix at `start` with equal-power pan. */
  function addVoice(mix, start, length, voice, pan = 0, send = 0) {
    const first = Math.max(0, Math.round(start * mix.sampleRate));
    const last = Math.min(mix.length, Math.round((start + length) * mix.sampleRate));
    const gainLeft = Math.cos(((pan + 1) * Math.PI) / 4);
    const gainRight = Math.sin(((pan + 1) * Math.PI) / 4);
    for (let i = first; i < last; i++) {
      const value = voice((i - first) / mix.sampleRate);
      mix.left[i] += value * gainLeft;
      mix.right[i] += value * gainRight;
      if (send) {
        mix.sendLeft[i] += value * gainLeft * send;
        mix.sendRight[i] += value * gainRight * send;
      }
    }
  }

  // ------------------------------------------------------------- instruments
  function shuttleClack(mix, at, gain, random, pan = 0, send = 0.4) {
    const bright = bandPass(2300, 5, mix.sampleRate);
    const hollow = bandPass(900, 3, mix.sampleRate);
    const secondHit = 0.026;
    addVoice(mix, at, 0.16, (s) => {
      const noise = random() * 2 - 1;
      const first = Math.exp(-s / 0.004) * 2.4;
      const second = s > secondHit ? Math.exp(-(s - secondHit) / 0.005) * 1.7 : 0;
      const body = Math.sin(TWO_PI * 380 * s) * Math.exp(-s / 0.02) * 0.35 + Math.sin(TWO_PI * 165 * s) * Math.exp(-s / 0.05) * 0.3;
      return gain * (bright(noise) * first + hollow(noise) * second + body);
    }, pan, send);
  }

  function bassNote(mix, at, frequency, length, gain, send = 0.2) {
    addVoice(mix, at, length, (s) => {
      const attack = Math.min(1, s / 0.04);
      const decay = Math.exp(-s / (length * 0.35));
      const phase = TWO_PI * frequency * s;
      return gain * attack * decay * Math.tanh(1.6 * (Math.sin(phase) + 0.35 * Math.sin(2 * phase))) * 0.6;
    }, 0, send);
  }

  function chime(mix, at, frequencies, gain, length = 1.4, send = 0.6) {
    addVoice(mix, at, length, (s) => {
      let value = 0;
      frequencies.forEach((frequency, i) => {
        value += Math.sin(TWO_PI * frequency * s) * Math.exp(-s / (length * (0.3 - i * 0.05)));
      });
      return gain * Math.min(1, s / 0.004) * value / frequencies.length;
    }, 0, send);
  }

  function noiseSweep(mix, at, length, fromHz, toHz, gain, random, send = 0.3) {
    let filter = bandPass(fromHz, 2.5, mix.sampleRate);
    let lastRetune = -1;
    addVoice(mix, at, length, (s) => {
      const p = s / length;
      const step = Math.floor(p * 64);
      if (step !== lastRetune) {
        filter = bandPass(fromHz * Math.pow(toHz / fromHz, p), 2.5, mix.sampleRate);
        lastRetune = step;
      }
      return gain * Math.sin(Math.PI * p) * filter(random() * 2 - 1) * 3;
    }, 0, send);
  }

  function taikoHit(mix, at, gain, random) {
    let phase = 0;
    const thump = onePoleLowPass(420, mix.sampleRate);
    addVoice(mix, at, 1.8, (s) => {
      const frequency = 42 + 62 * Math.exp(-s / 0.07);
      phase += (TWO_PI * frequency) / mix.sampleRate;
      const envelope = Math.exp(-s / 0.5) * Math.min(1, s / 0.002);
      return gain * (Math.sin(phase) * envelope + thump(random() * 2 - 1) * Math.exp(-s / 0.03) * 3);
    }, 0, 0.35);
  }

  function thunder(mix, at, gain, random, length = 2.6) {
    const rumble = onePoleLowPass(160, mix.sampleRate);
    const crack = bandPass(3000, 1.2, mix.sampleRate);
    addVoice(mix, at, length, (s) => {
      const noise = random() * 2 - 1;
      const swell = Math.min(1, s / 0.08) * Math.exp(-s / (length * 0.35));
      return gain * (rumble(noise) * swell * 7 + crack(noise) * Math.exp(-s / 0.05) * 0.9);
    }, 0, 0.4);
  }

  function stringSnap(mix, at, gain, random) {
    addVoice(mix, at, 0.3, (s) => {
      const frequency = 1300 - 600 * Math.min(1, s / 0.08);
      return gain * (Math.sin(TWO_PI * frequency * s) * Math.exp(-s / 0.05) + (random() * 2 - 1) * Math.exp(-s / 0.003));
    }, 0.2, 0.1);
  }

  function heartbeat(mix, at, gain) {
    addVoice(mix, at, 0.3, (s) => gain * Math.sin(TWO_PI * 48 * s + 4 * (1 - Math.exp(-s / 0.02))) * Math.exp(-s / 0.07) * Math.min(1, s / 0.004), 0, 0.1);
  }

  function watchTick(mix, at, gain, random) {
    addVoice(mix, at, 0.012, (s) => gain * (Math.sin(TWO_PI * 4500 * s) + (random() - 0.5)) * Math.exp(-s / 0.0025), 0.35, 0.15);
  }

  function woodKnock(mix, at, gain, random) {
    const click = bandPass(1500, 3, mix.sampleRate);
    addVoice(mix, at, 0.4, (s) => {
      const body = Math.sin(TWO_PI * 190 * s) * Math.exp(-s / 0.09) + Math.sin(TWO_PI * 330 * s) * Math.exp(-s / 0.05) * 0.6;
      return gain * (body + click(random() * 2 - 1) * Math.exp(-s / 0.004) * 4);
    }, 0, 0.5);
  }

  function breath(mix, at, gain, random) {
    const soften = onePoleLowPass(900, mix.sampleRate);
    addVoice(mix, at, 1.0, (s) => gain * soften(random() * 2 - 1) * Math.sin(Math.PI * clamp(s / 1.0)) * 3, 0, 0.3);
  }

  /** Brown-noise surf with slow swells, decorrelated per channel. */
  function sea(mix, start, end, gain, random) {
    const fade = 0.8;
    const first = Math.round(start * mix.sampleRate);
    const last = Math.min(mix.length, Math.round(end * mix.sampleRate));
    let brownLeft = 0;
    let brownRight = 0;
    const hissLeft = onePoleLowPass(2600, mix.sampleRate);
    const hissRight = onePoleLowPass(2600, mix.sampleRate);
    for (let i = first; i < last; i++) {
      const t = i / mix.sampleRate;
      brownLeft = 0.996 * brownLeft + 0.03 * (random() * 2 - 1);
      brownRight = 0.996 * brownRight + 0.03 * (random() * 2 - 1);
      const swell = Math.pow(0.5 + 0.5 * Math.sin(TWO_PI * 0.12 * t + 1.3), 2);
      const envelope = gain * Math.min(progress(t, start, start + fade), 1 - progress(t, end - fade, end));
      mix.left[i] += envelope * (brownLeft * 0.9 + hissLeft(random() * 2 - 1) * swell * 0.35);
      mix.right[i] += envelope * (brownRight * 0.9 + hissRight(random() * 2 - 1) * swell * 0.35);
    }
  }

  function stormRoar(mix, start, end, gain, random) {
    const roar = bandPass(380, 0.7, mix.sampleRate);
    const whistle = bandPass(1400, 8, mix.sampleRate);
    addVoice(mix, start, end - start, (s) => {
      const noise = random() * 2 - 1;
      const envelope = Math.min(1, s / 0.2) * (1 - clamp((s - (end - start - DECLICK_SECONDS)) / DECLICK_SECONDS));
      return gain * envelope * (roar(noise) * 2.4 + whistle(noise) * 0.5 * (0.5 + 0.5 * Math.sin(TWO_PI * 0.7 * s)));
    }, 0, 0.2);
  }

  /** Endlessly rising Shepard tone; `gainAt(t)` shapes the tension curve. */
  function shepardTone(mix, cues) {
    const { start, peak } = cues.shepard;
    const phases = new Float64Array(SHEPARD_PARTIALS);
    const smoothing = 1 - Math.exp(-1 / (DECLICK_SECONDS * mix.sampleRate));
    let gain = 0;
    const first = Math.round(start * mix.sampleRate);
    const last = Math.min(mix.length, Math.round((peak + 0.05) * mix.sampleRate));
    for (let i = first; i < last; i++) {
      const t = i / mix.sampleRate;
      const elapsed = t - start;
      const octaves = 0.12 * elapsed + 0.004 * elapsed * elapsed;
      gain += (shepardGain(t, cues) - gain) * smoothing;
      let value = 0;
      for (let partial = 0; partial < SHEPARD_PARTIALS; partial++) {
        const position = (partial + octaves) % SHEPARD_PARTIALS;
        const frequency = SHEPARD_BASE_HZ * Math.pow(2, position);
        phases[partial] += (TWO_PI * frequency) / mix.sampleRate;
        const weight = 0.5 - 0.5 * Math.cos((TWO_PI * position) / SHEPARD_PARTIALS);
        value += Math.sin(phases[partial]) * weight;
      }
      const sample = gain * value * 0.25;
      mix.left[i] += sample;
      mix.right[i] += sample;
      mix.sendLeft[i] += sample * 0.15;
      mix.sendRight[i] += sample * 0.15;
    }
  }

  function shepardGain(t, cues) {
    const { start, cut, resume, swell, peak } = cues.shepard;
    if (t < start || t >= peak) return 0;
    if (t >= cut && t < resume) return 0;
    if (t < cut) return 0.05 + 0.13 * progress(t, start, cut);
    if (t < swell) return 0.06;
    return 0.06 + 0.62 * Math.pow(progress(t, swell, peak), 1.4);
  }

  // ------------------------------------------------------------------ space
  function applyReverb(mix) {
    const combDelays = [0.0297, 0.0371, 0.0411, 0.0437];
    const allPassDelays = [0.005, 0.0017];
    const feedback = 0.8;
    const damping = 0.3;
    const wet = 0.28;
    [['sendLeft', 'left', 0], ['sendRight', 'right', 0.0023]].forEach(([sendKey, outKey, spread]) => {
      const input = mix[sendKey];
      const output = new Float32Array(mix.length);
      combDelays.forEach((delaySeconds) => {
        const size = Math.round((delaySeconds + spread) * mix.sampleRate);
        const buffer = new Float32Array(size);
        let index = 0;
        let filtered = 0;
        for (let i = 0; i < mix.length; i++) {
          const delayed = buffer[index];
          filtered = delayed * (1 - damping) + filtered * damping;
          buffer[index] = input[i] + filtered * feedback;
          output[i] += delayed;
          index = (index + 1) % size;
        }
      });
      allPassDelays.forEach((delaySeconds) => {
        const size = Math.round(delaySeconds * mix.sampleRate);
        const buffer = new Float32Array(size);
        let index = 0;
        for (let i = 0; i < mix.length; i++) {
          const delayed = buffer[index];
          const value = output[i];
          buffer[index] = value + delayed * 0.5;
          output[i] = delayed - value * 0.5;
          index = (index + 1) % size;
        }
      });
      const destination = mix[outKey];
      for (let i = 0; i < mix.length; i++) destination[i] += output[i] * wet * 0.25;
    });
  }

  /** Forces true silence (after reverb) for the storm freeze and the hard cut. */
  function applySilence(mix, windows) {
    const ramp = Math.round(DECLICK_SECONDS * mix.sampleRate);
    windows.forEach(([start, end]) => {
      const first = Math.round(start * mix.sampleRate);
      const last = Math.min(mix.length, Math.round(end * mix.sampleRate));
      for (let i = Math.max(0, first - ramp); i < Math.min(mix.length, last + ramp); i++) {
        let gain = 0;
        if (i < first) gain = (first - i) / ramp;
        else if (i >= last) gain = (i - last) / ramp;
        mix.left[i] *= gain;
        mix.right[i] *= gain;
      }
    });
  }

  function master(mix) {
    let peak = 0;
    for (let i = 0; i < mix.length; i++) {
      mix.left[i] = Math.tanh(mix.left[i] * 1.1);
      mix.right[i] = Math.tanh(mix.right[i] * 1.1);
      peak = Math.max(peak, Math.abs(mix.left[i]), Math.abs(mix.right[i]));
    }
    const scale = peak > 0 ? MASTER_CEILING / peak : 1;
    for (let i = 0; i < mix.length; i++) {
      mix.left[i] *= scale;
      mix.right[i] *= scale;
    }
  }

  // ------------------------------------------------------------------ score
  function synthesize(cues, sampleRate = SAMPLE_RATE) {
    const mix = createMix(cues.duration, sampleRate);
    const random = seededRandom(7);

    shuttleClack(mix, cues.coldOpenClack, 0.9, random, 0, 0.9);
    cues.seaBeds.forEach(([start, end, gain]) => sea(mix, start, end, gain, random));
    bassNote(mix, cues.pairingBass, 41.2, 3.4, 0.9);
    noiseSweep(mix, cues.owlSweep[0], cues.owlSweep[1] - cues.owlSweep[0], 300, 2400, 0.12, random);
    chime(mix, cues.pairedChime, [880, 1318.5], 0.22);
    shuttleClack(mix, cues.cardClack, 0.85, random);
    cues.montageClacks.forEach((at, i) => shuttleClack(mix, at, 0.55 + 0.35 * (i / cues.montageClacks.length), random, i % 2 ? 0.3 : -0.3, 0.25));
    breath(mix, cues.breath, 0.1, random);
    chime(mix, cues.autoContinueTick, [1568], 0.06, 0.4, 0.3);

    sea(mix, cues.sirenSea[0], cues.sirenSea[1], 0.3, random);
    shepardTone(mix, cues);
    for (let at = cues.shepard.start; at < cues.shepard.cut; at += 0.5) watchTick(mix, at, 0.16, random);
    for (let at = cues.shepard.swell, interval = 0.5; at < cues.shepard.peak; interval = Math.max(0.2, interval * 0.93)) {
      watchTick(mix, at, 0.3, random);
      at += interval;
    }
    cues.proteusHits.forEach((at) => taikoHit(mix, at, 0.9, random));

    stormRoar(mix, cues.storm.start, cues.storm.snap, 0.5, random);
    thunder(mix, cues.storm.lightning[0], 0.8, random);
    thunder(mix, cues.storm.lightning[1], 0.9, random);
    stringSnap(mix, cues.storm.snap, 0.7, random);
    stormRoar(mix, cues.shepard.resume, cues.storm.end, 0.12, random);
    thunder(mix, cues.storm.lightning[2], 0.35, random, 2.0);

    cues.heartbeats.forEach((at) => {
      heartbeat(mix, at, 0.9);
      heartbeat(mix, at + 0.18, 0.55);
    });
    chime(mix, cues.heal, [1046.5, 1568, 2093], 0.16, 1.6, 0.7);

    addVoice(mix, cues.arrowRelease, 0.5, (s) => 0.6 * Math.sin(TWO_PI * 140 * s) * Math.exp(-s / 0.08) + 0.3 * (random() * 2 - 1) * Math.exp(-s / 0.01), 0, 0.3);
    cues.ringPasses.forEach((at, i) => chime(mix, at, [2400 + i * 40, 3600 + i * 60], 0.1, 0.35, 0.4));

    chime(mix, cues.arrival, [660, 990, 1320], 0.3, 1.8, 0.8);
    shuttleClack(mix, cues.outroClack, 0.9, random, 0, 0.9);
    bassNote(mix, cues.logoSwell, 55, 2.6, 0.45);
    noiseSweep(mix, cues.paperSlide, 0.32, 1200, 4200, 0.35, random);
    woodKnock(mix, cues.woodKnock, 0.8, random);

    applyReverb(mix);
    applySilence(mix, [
      [cues.silence[0], cues.silence[1] - 0.05],
      [cues.storm.snap + 0.3, cues.shepard.resume],
      [cues.hardCut, cues.hardCut + 0.3],
      [cues.woodKnock + 0.6, cues.duration],
    ]);
    master(mix);
    return mix;
  }

  function encodeWav(mix) {
    const bytesPerSample = 2;
    const channels = 2;
    const dataBytes = mix.length * channels * bytesPerSample;
    const buffer = new ArrayBuffer(44 + dataBytes);
    const view = new DataView(buffer);
    const writeText = (offset, text) => [...text].forEach((char, i) => view.setUint8(offset + i, char.charCodeAt(0)));
    writeText(0, 'RIFF');
    view.setUint32(4, 36 + dataBytes, true);
    writeText(8, 'WAVE');
    writeText(12, 'fmt ');
    view.setUint32(16, 16, true);
    view.setUint16(20, 1, true);
    view.setUint16(22, channels, true);
    view.setUint32(24, mix.sampleRate, true);
    view.setUint32(28, mix.sampleRate * channels * bytesPerSample, true);
    view.setUint16(32, channels * bytesPerSample, true);
    view.setUint16(34, 16, true);
    writeText(36, 'data');
    view.setUint32(40, dataBytes, true);
    let offset = 44;
    for (let i = 0; i < mix.length; i++) {
      view.setInt16(offset, Math.round(clamp(mix.left[i], -1, 1) * 32767), true);
      view.setInt16(offset + 2, Math.round(clamp(mix.right[i], -1, 1) * 32767), true);
      offset += 4;
    }
    return new Uint8Array(buffer);
  }

  function renderWavBase64(cues) {
    const bytes = encodeWav(synthesize(cues));
    let binary = '';
    const chunk = 0x8000;
    for (let i = 0; i < bytes.length; i += chunk) binary += String.fromCharCode.apply(null, bytes.subarray(i, i + chunk));
    return btoa(binary);
  }

  window.LooperTrailerAudio = { SAMPLE_RATE, synthesize, encodeWav, renderWavBase64 };
})();
