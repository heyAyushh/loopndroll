/* Legend of the Looper — the score, synthesized sample by sample.
 *
 * Plucked guzheng (Karplus–Strong), dizi flute, bowed erhu, taiko, gongs,
 * temple bells, jade chimes and foley — composed in D minor pentatonic and
 * resolving to D major. Every event time comes from the picture's beats.
 */
(function () {
  'use strict';

  const SAMPLE_RATE = 48000;
  const TWO_PI = Math.PI * 2;
  const MASTER_CEILING = 0.9;
  const DECLICK = 0.004;
  // Instrument calibration so a `gain` means roughly the same loudness everywhere.
  const GUZHENG_LEVEL = 3.5;
  const PAD_LEVEL = 4;

  const clamp = (value, low = 0, high = 1) => Math.min(high, Math.max(low, value));
  const progress = (t, start, end) => clamp((t - start) / (end - start));

  function seededRandom(seed) {
    let state = seed >>> 0;
    return function next() {
      state = (state + 0x6d2b79f5) >>> 0;
      let r = Math.imul(state ^ (state >>> 15), 1 | state);
      r = (r + Math.imul(r ^ (r >>> 7), 61 | r)) ^ r;
      return ((r ^ (r >>> 14)) >>> 0) / 4294967296;
    };
  }

  const NOTE_INDEX = { C: 0, 'C#': 1, D: 2, 'D#': 3, E: 4, F: 5, 'F#': 6, G: 7, 'G#': 8, A: 9, 'A#': 10, B: 11 };
  function hz(name) {
    const match = /^([A-G]#?)(-?\d)$/.exec(name);
    const midi = (Number(match[2]) + 1) * 12 + NOTE_INDEX[match[1]];
    return 440 * Math.pow(2, (midi - 69) / 12);
  }
  const MINOR = ['D', 'F', 'G', 'A', 'C'];
  const MAJOR = ['D', 'E', 'F#', 'A', 'B'];
  function scaleRun(scale, fromOctave, count, descending = false) {
    const notes = [];
    for (let i = 0; i < count; i++) notes.push(`${scale[i % scale.length]}${fromOctave + Math.floor(i / scale.length)}`);
    return descending ? notes.reverse() : notes;
  }

  // ---------------------------------------------------------------- filters
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

  function lowPass(frequency, sampleRate) {
    const k = 1 - Math.exp((-TWO_PI * frequency) / sampleRate);
    let y = 0;
    return (x) => {
      y += k * (x - y);
      return y;
    };
  }

  function highPass(frequency, sampleRate) {
    const low = lowPass(frequency, sampleRate);
    return (x) => x - low(x);
  }

  // -------------------------------------------------------------------- mix
  function createMix(duration, sampleRate) {
    const length = Math.ceil(duration * sampleRate);
    return { sampleRate, length, left: new Float32Array(length), right: new Float32Array(length), sendLeft: new Float32Array(length), sendRight: new Float32Array(length) };
  }

  function addVoice(mix, start, length, voice, pan = 0, send = 0) {
    const first = Math.max(0, Math.round(start * mix.sampleRate));
    const last = Math.min(mix.length, Math.round((start + length) * mix.sampleRate));
    const skipped = Math.max(0, -Math.round(start * mix.sampleRate));
    const gainLeft = Math.cos(((clamp(pan, -1, 1) + 1) * Math.PI) / 4);
    const gainRight = Math.sin(((clamp(pan, -1, 1) + 1) * Math.PI) / 4);
    for (let i = first; i < last; i++) {
      const value = voice((i - first + skipped) / mix.sampleRate);
      mix.left[i] += value * gainLeft;
      mix.right[i] += value * gainRight;
      if (send) {
        mix.sendLeft[i] += value * gainLeft * send;
        mix.sendRight[i] += value * gainRight * send;
      }
    }
  }

  // ------------------------------------------------------------ instruments
  /** Guzheng pluck: Karplus–Strong with a fractional delay so high notes stay in tune. */
  function guzheng(mix, at, frequency, gain, options = {}) {
    const { decay = 2.6, pan = 0, brightness = 0.55, send = 0.35, seed = 1 } = options;
    const random = seededRandom(Math.round(frequency * 13 + at * 1000) + seed);
    const period = mix.sampleRate / frequency - 0.5;
    const size = Math.ceil(period) + 4;
    const buffer = new Float32Array(size);
    const soften = lowPass(800 + brightness * 6000, mix.sampleRate);
    let peak = 1e-6;
    for (let i = 0; i < size; i++) {
      buffer[i] = soften(random() * 2 - 1);
      peak = Math.max(peak, Math.abs(buffer[i]));
    }
    for (let i = 0; i < size; i++) buffer[i] /= peak;
    const loss = Math.pow(0.001, 1 / (decay * frequency));
    let write = size;
    let last = 0;
    addVoice(mix, at, decay * 1.1, (s) => {
      const read = write - period;
      const index = Math.floor(read);
      const fraction = read - index;
      const a = buffer[((index % size) + size) % size];
      const b = buffer[(((index + 1) % size) + size) % size];
      const delayed = a + (b - a) * fraction;
      const out = (delayed + last) * 0.5 * loss;
      last = delayed;
      buffer[write % size] = out;
      write++;
      const pick = s < 0.004 ? (1 - s / 0.004) * 0.3 : 0;
      return gain * GUZHENG_LEVEL * (out + pick * (random() - 0.5));
    }, pan, send);
  }

  function glissando(mix, at, notes, spacing, gain, options = {}) {
    notes.forEach((note, i) => guzheng(mix, at + i * spacing, hz(note), gain * (0.7 + 0.3 * (i / notes.length)), { decay: 2.2, pan: ((i / notes.length) - 0.5) * 0.8, brightness: 0.7, ...options }));
  }

  function tremolo(mix, start, end, note, gain, rate = 16, options = {}) {
    for (let t = start, k = 0; t < end; t += 1 / rate, k++) {
      const swell = Math.sin(Math.PI * progress(t, start, end));
      guzheng(mix, t, hz(note), gain * (0.4 + 0.6 * swell), { decay: 0.9, pan: k % 2 ? 0.2 : -0.2, brightness: 0.8, ...options });
    }
  }

  /** Dizi: a breathy flute tone with delayed vibrato and a soft chiff. */
  function dizi(mix, at, frequency, duration, gain, options = {}) {
    const { pan = 0.1, vibrato = 0.007, send = 0.5, seed = 3 } = options;
    const random = seededRandom(seed + Math.round(at * 100));
    const breath = bandPass(frequency * 2, 4, mix.sampleRate);
    const chiff = bandPass(frequency * 3, 2, mix.sampleRate);
    let phase = 0;
    addVoice(mix, at, duration + 0.5, (s) => {
      const attack = Math.min(1, s / 0.07);
      const release = s > duration ? Math.exp(-(s - duration) / 0.14) : 1;
      const depth = s > 0.3 ? vibrato * Math.min(1, (s - 0.3) / 0.4) : 0;
      phase += (TWO_PI * frequency * (1 + depth * Math.sin(TWO_PI * 5.3 * s))) / mix.sampleRate;
      const tone = Math.sin(phase) + 0.22 * Math.sin(2 * phase) + 0.07 * Math.sin(3 * phase);
      const noise = random() * 2 - 1;
      const air = breath(noise) * 0.35 + (s < 0.06 ? chiff(noise) * (1 - s / 0.06) * 1.4 : 0);
      return gain * attack * release * (tone * 0.62 + air);
    }, pan, send);
  }

  /** Erhu: a bowed line with portamento between notes, vibrato and bow noise. */
  function erhuLine(mix, start, notes, beat, gain, options = {}) {
    const { pan = -0.15, send = 0.5, seed = 5 } = options;
    const timeline = [];
    let cursor = 0;
    notes.forEach(([name, beats]) => {
      timeline.push({ at: cursor, frequency: hz(name) });
      cursor += beats * beat;
    });
    const total = cursor;
    const random = seededRandom(seed);
    const formantA = bandPass(900, 1.2, mix.sampleRate);
    const formantB = bandPass(2400, 1.6, mix.sampleRate);
    const bow = bandPass(3000, 1, mix.sampleRate);
    let phase = 0;
    let current = timeline[0].frequency;
    addVoice(mix, start, total + 0.6, (s) => {
      let target = timeline[0].frequency;
      let noteStart = 0;
      for (const note of timeline) {
        if (s >= note.at) {
          target = note.frequency;
          noteStart = note.at;
        }
      }
      current += (target - current) * 0.0022;
      const sinceNote = s - noteStart;
      const vib = 0.011 * Math.min(1, Math.max(0, sinceNote - 0.2) / 0.5) * Math.sin(TWO_PI * 6.1 * s);
      phase += (TWO_PI * current * (1 + vib)) / mix.sampleRate;
      let tone = 0;
      for (let h = 1; h <= 8; h++) tone += Math.sin(phase * h) / Math.pow(h, 0.9);
      const envelope = Math.min(1, s / 0.25) * (s > total ? Math.exp(-(s - total) / 0.2) : 1);
      const bowPressure = 0.85 + 0.15 * Math.sin(TWO_PI * 0.7 * s);
      return gain * envelope * bowPressure * (formantA(tone) * 1.3 + formantB(tone) * 0.8 + bow(random() - 0.5) * 0.25);
    }, pan, send);
  }

  /** Warm string pad: detuned additive saws, brightness shaped by harmonic count. */
  function pad(mix, start, end, notes, gain, options = {}) {
    const { attack = 1.5, release = 1.5, harmonics = 6, send = 0.4, tremoloRate = 0, pan = 0 } = options;
    const voices = [];
    notes.forEach((name) => [-0.004, 0, 0.005].forEach((detune) => voices.push({ frequency: hz(name) * (1 + detune), phase: Math.random() * 0 })));
    const length = end - start + release;
    addVoice(mix, start, length, (s) => {
      const envelope = Math.min(1, s / attack) * (s > end - start ? Math.max(0, 1 - (s - (end - start)) / release) : 1);
      const trem = tremoloRate ? 0.65 + 0.35 * Math.sin(TWO_PI * tremoloRate * s) : 1;
      let value = 0;
      for (const voice of voices) {
        const base = TWO_PI * voice.frequency * s;
        for (let h = 1; h <= harmonics; h++) value += Math.sin(base * h) / (h * 1.3);
      }
      return (gain * PAD_LEVEL * envelope * trem * value) / voices.length;
    }, pan, send);
  }

  function taiko(mix, at, gain, options = {}) {
    const { pitch = 1, pan = 0, send = 0.35, seed = 7 } = options;
    const random = seededRandom(seed + Math.round(at * 1000));
    const thump = lowPass(380, mix.sampleRate);
    let phase = 0;
    addVoice(mix, at, 1.9, (s) => {
      const frequency = (44 + 70 * Math.exp(-s / 0.06)) * pitch;
      phase += (TWO_PI * frequency) / mix.sampleRate;
      const envelope = Math.exp(-s / 0.55) * Math.min(1, s / 0.002);
      return gain * (Math.sin(phase) * envelope + thump(random() * 2 - 1) * Math.exp(-s / 0.028) * 3.2);
    }, pan, send);
  }

  function drumPattern(mix, start, end, bpm, gain, pattern, options = {}) {
    const step = 60 / bpm / 2;
    for (let t = start, k = 0; t < end; t += step, k++) {
      const accent = pattern[k % pattern.length];
      if (!accent) continue;
      const swell = options.crescendo ? 0.35 + 0.65 * progress(t, start, end) : 1;
      taiko(mix, t, gain * accent * swell, { pitch: accent > 0.8 ? 1 : 1.25, pan: k % 2 ? 0.25 : -0.25 });
    }
  }

  /** Tam-tam: inharmonic partials that swell, then shimmer. */
  function tamTam(mix, at, gain, swell = 1.4, ring = 5) {
    const partials = [62, 97, 131, 178, 233, 301, 388, 467, 620, 811].map((f, i) => ({ f, a: 1 / (1 + i * 0.35), phase: i }));
    const noise = bandPass(700, 0.8, mix.sampleRate);
    const random = seededRandom(Math.round(at * 100));
    addVoice(mix, at - swell, swell + ring, (s) => {
      const envelope = s < swell ? Math.pow(s / swell, 2.2) : Math.exp(-(s - swell) / (ring * 0.35));
      let value = 0;
      partials.forEach((p) => {
        value += p.a * Math.sin(TWO_PI * p.f * s * (1 + 0.002 * Math.sin(TWO_PI * 0.7 * s + p.phase)) + p.phase);
      });
      return gain * envelope * (value * 0.25 + noise(random() * 2 - 1) * 0.8);
    }, 0, 0.6);
  }

  /** Opera gong: a bright strike whose pitch sinks — the stamp's voice. */
  function operaGong(mix, at, gain, base = 420) {
    let phase = 0;
    addVoice(mix, at, 2.4, (s) => {
      const frequency = base * (1 - 0.18 * Math.min(1, s / 0.5));
      phase += (TWO_PI * frequency) / mix.sampleRate;
      return gain * Math.exp(-s / 0.6) * (Math.sin(phase) + 0.4 * Math.sin(phase * 2.71) + 0.25 * Math.sin(phase * 4.13));
    }, 0.1, 0.5);
  }

  function templeBell(mix, at, frequency, gain) {
    const partials = [[1, 1, 4.5], [2, 0.6, 3], [2.76, 0.4, 2.2], [5.4, 0.25, 1.2], [8.9, 0.1, 0.6]];
    addVoice(mix, at, 6, (s) => {
      let value = 0;
      partials.forEach(([ratio, amp, decay]) => {
        value += amp * Math.sin(TWO_PI * frequency * ratio * s) * Math.exp(-s / decay);
      });
      return gain * Math.min(1, s / 0.003) * value * 0.5;
    }, 0, 0.7);
  }

  function jadeChime(mix, at, frequency, gain, pan = 0) {
    addVoice(mix, at, 2.4, (s) => {
      const value = Math.sin(TWO_PI * frequency * s) * Math.exp(-s / 0.9) + 0.5 * Math.sin(TWO_PI * frequency * 2.76 * s) * Math.exp(-s / 0.35) + 0.2 * Math.sin(TWO_PI * frequency * 5.4 * s) * Math.exp(-s / 0.15);
      return gain * Math.min(1, s / 0.002) * value * 0.5;
    }, pan, 0.6);
  }

  function woodBlock(mix, at, gain, pitch = 1, pan = 0.2) {
    addVoice(mix, at, 0.25, (s) => gain * (Math.sin(TWO_PI * 820 * pitch * s) * Math.exp(-s / 0.035) + 0.4 * Math.sin(TWO_PI * 1340 * pitch * s) * Math.exp(-s / 0.02)), pan, 0.3);
  }

  function stampThump(mix, at, gain, random) {
    const body = lowPass(260, mix.sampleRate);
    addVoice(mix, at, 0.6, (s) => gain * (Math.sin(TWO_PI * 70 * s) * Math.exp(-s / 0.09) * 1.2 + body(random() * 2 - 1) * Math.exp(-s / 0.02) * 4), 0, 0.3);
  }

  function noiseBed(mix, start, end, gain, random, options = {}) {
    const { low = 200, high = 2000, fade = 0.8, swellRate = 0.1, pan = 0 } = options;
    const band = bandPass(Math.sqrt(low * high), Math.max(0.3, Math.sqrt(low * high) / (high - low)), mix.sampleRate);
    addVoice(mix, start, end - start, (s) => {
      const envelope = Math.min(1, s / fade, (end - start - s) / fade);
      const swell = 0.6 + 0.4 * Math.sin(TWO_PI * swellRate * s + start);
      return gain * envelope * swell * band(random() * 2 - 1) * 2.5;
    }, pan, 0.2);
  }

  function sea(mix, start, end, gain, random) {
    let brownLeft = 0;
    let brownRight = 0;
    const hiss = lowPass(2200, mix.sampleRate);
    const first = Math.round(start * mix.sampleRate);
    const last = Math.min(mix.length, Math.round(end * mix.sampleRate));
    for (let i = first; i < last; i++) {
      const t = i / mix.sampleRate;
      brownLeft = 0.996 * brownLeft + 0.03 * (random() * 2 - 1);
      brownRight = 0.996 * brownRight + 0.03 * (random() * 2 - 1);
      const swell = Math.pow(0.5 + 0.5 * Math.sin(TWO_PI * 0.11 * t + 1.1), 2);
      const envelope = gain * Math.min(progress(t, start, start + 1.2), 1 - progress(t, end - 1.2, end));
      const wash = hiss(random() * 2 - 1) * swell * 0.4;
      mix.left[i] += envelope * (brownLeft * 0.9 + wash);
      mix.right[i] += envelope * (brownRight * 0.9 + wash);
    }
  }

  function whoosh(mix, at, length, gain, random, fromPan = -0.6, toPan = 0.6) {
    let band = bandPass(400, 1.5, mix.sampleRate);
    let retune = -1;
    addVoice(mix, at, length, (s) => {
      const p = s / length;
      const step = Math.floor(p * 48);
      if (step !== retune) {
        band = bandPass(300 + Math.sin(Math.PI * p) * 1600, 1.5, mix.sampleRate);
        retune = step;
      }
      return gain * Math.pow(Math.sin(Math.PI * p), 2) * band(random() * 2 - 1) * 3;
    }, (fromPan + toPan) / 2, 0.3);
  }

  function paperRustle(mix, start, end, gain, random) {
    const crinkle = bandPass(3200, 1.2, mix.sampleRate);
    const body = bandPass(900, 0.9, mix.sampleRate);
    addVoice(mix, start, end - start, (s) => {
      const envelope = Math.min(1, s / 0.3, (end - start - s) / 0.4);
      const bursts = Math.pow(Math.max(0, Math.sin(TWO_PI * 7.3 * s) * Math.sin(TWO_PI * 2.1 * s + 1)), 3);
      const noise = random() * 2 - 1;
      return gain * envelope * (crinkle(noise) * (0.3 + bursts * 1.6) + body(noise) * 0.5);
    }, -0.2, 0.2);
  }

  function brushSwish(mix, at, length, gain, random) {
    const band = bandPass(1800, 1.1, mix.sampleRate);
    addVoice(mix, at, length, (s) => gain * Math.sin(Math.PI * clamp(s / length)) * band(random() * 2 - 1) * 2, 0.1, 0.3);
  }

  function flutter(mix, start, end, gain, random, rate = 9) {
    const band = bandPass(1400, 1.2, mix.sampleRate);
    addVoice(mix, start, end - start, (s) => {
      const flap = Math.pow(Math.max(0, Math.sin(TWO_PI * rate * s)), 6);
      const envelope = Math.min(1, s / 0.2, (end - start - s) / 0.3);
      return gain * envelope * flap * band(random() * 2 - 1) * 3;
    }, 0.3, 0.3);
  }

  function thunder(mix, at, gain, random) {
    const rumble = lowPass(150, mix.sampleRate);
    const crack = bandPass(2800, 1, mix.sampleRate);
    addVoice(mix, at, 3.2, (s) => {
      const noise = random() * 2 - 1;
      return gain * (rumble(noise) * Math.min(1, s / 0.1) * Math.exp(-s / 1.1) * 8 + crack(noise) * Math.exp(-s / 0.05));
    }, 0, 0.5);
  }

  function dragonGrowl(mix, start, end, gain, random) {
    let band = bandPass(110, 2, mix.sampleRate);
    let retune = -1;
    addVoice(mix, start, end - start, (s) => {
      const p = s / (end - start);
      const step = Math.floor(p * 40);
      if (step !== retune) {
        band = bandPass(90 + 60 * Math.sin(p * Math.PI * 3), 2, mix.sampleRate);
        retune = step;
      }
      const envelope = Math.min(1, s / 0.8) * Math.min(1, (end - start - s) / 0.1);
      const growl = 0.6 + 0.4 * Math.sin(TWO_PI * 31 * s);
      return gain * envelope * Math.tanh(band(random() * 2 - 1) * 14 * growl);
    }, 0, 0.4);
  }

  function rain(mix, start, end, gain, random) {
    const hiss = highPass(3000, mix.sampleRate);
    addVoice(mix, start, end - start, (s) => gain * Math.min(1, s / 0.6, (end - start - s) / 0.02) * hiss(random() * 2 - 1), 0, 0.1);
  }

  function stringSnap(mix, at, gain, random) {
    addVoice(mix, at, 0.35, (s) => {
      const frequency = 1500 - 800 * Math.min(1, s / 0.07);
      return gain * (Math.sin(TWO_PI * frequency * s) * Math.exp(-s / 0.05) + (random() * 2 - 1) * Math.exp(-s / 0.003));
    }, 0.1, 0.2);
  }

  function inkDropBoom(mix, at, gain) {
    addVoice(mix, at - 0.02, 0.06, (s) => gain * 0.4 * Math.sin(TWO_PI * (1800 - s * 20000) * s) * Math.exp(-s / 0.01), 0, 0.5);
    let phase = 0;
    addVoice(mix, at, 4, (s) => {
      phase += (TWO_PI * (38 + 30 * Math.exp(-s / 0.1))) / mix.sampleRate;
      return gain * Math.sin(phase) * Math.exp(-s / 1.4) * Math.min(1, s / 0.004);
    }, 0, 0.8);
  }

  function candleStrike(mix, at, gain, random) {
    const band = bandPass(2500, 0.8, mix.sampleRate);
    const flame = lowPass(600, mix.sampleRate);
    addVoice(mix, at, 1.4, (s) => {
      const noise = random() * 2 - 1;
      return gain * (band(noise) * Math.exp(-s / 0.06) * 2 + flame(noise) * Math.min(1, s / 0.2) * Math.exp(-s / 0.6) * 3);
    }, 0.4, 0.3);
  }

  function voicesPad(mix, start, end, gain, notes) {
    const vowels = [[700, 1200], [600, 1000]];
    const bands = notes.flatMap(() => vowels.map(([a, b]) => [bandPass(a, 3, mix.sampleRate), bandPass(b, 4, mix.sampleRate)]));
    let phases = notes.map(() => 0);
    addVoice(mix, start, end - start + 1, (s) => {
      const envelope = Math.min(1, s / 2) * Math.max(0, Math.min(1, (end - start + 0.8 - s) / 0.8));
      let value = 0;
      notes.forEach((name, i) => {
        const frequency = hz(name) * (1 + 0.006 * Math.sin(TWO_PI * (4.7 + i * 0.3) * s));
        phases[i] += (TWO_PI * frequency) / mix.sampleRate;
        let saw = 0;
        for (let h = 1; h <= 10; h++) saw += Math.sin(phases[i] * h) / h;
        const [a, b] = bands[i * 2];
        value += a(saw) * 1.2 + b(saw) * 0.7;
      });
      return (gain * envelope * value) / notes.length;
    }, 0, 0.7);
    phases = null;
  }

  function phrase(mix, start, bpm, notes, gain, play) {
    const beat = 60 / bpm;
    let cursor = start;
    notes.forEach(([name, beats]) => {
      if (name) play(mix, cursor, hz(name), beats * beat, gain);
      cursor += beats * beat;
    });
    return cursor;
  }

  // ------------------------------------------------------------------ space
  function applyReverb(mix) {
    const combs = [0.0413, 0.0487, 0.0567, 0.0617, 0.0719];
    const allPasses = [0.0089, 0.0031];
    const feedback = 0.86;
    const damping = 0.35;
    const wet = 0.3;
    [['sendLeft', 'left', 0], ['sendRight', 'right', 0.0027]].forEach(([sendKey, outKey, spread]) => {
      const input = mix[sendKey];
      const output = new Float32Array(mix.length);
      combs.forEach((seconds) => {
        const size = Math.round((seconds + spread) * mix.sampleRate);
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
      allPasses.forEach((seconds) => {
        const size = Math.round(seconds * mix.sampleRate);
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
      for (let i = 0; i < mix.length; i++) destination[i] += output[i] * wet * 0.2;
    });
  }

  function applySilence(mix, windows) {
    const ramp = Math.round(DECLICK * mix.sampleRate);
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

  function master(mix, fadeFrom, duration) {
    let peak = 0;
    for (let i = 0; i < mix.length; i++) {
      const t = i / mix.sampleRate;
      const fade = t > fadeFrom ? Math.max(0, 1 - (t - fadeFrom) / (duration - fadeFrom)) : 1;
      mix.left[i] = Math.tanh(mix.left[i] * 1.05) * fade;
      mix.right[i] = Math.tanh(mix.right[i] * 1.05) * fade;
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
    const B = cues.beat;
    const said = (id) => (cues.narration.find((line) => line.id === id) || { start: 0 }).start;
    const mix = createMix(cues.duration, sampleRate);
    const random = seededRandom(19);
    const play = {
      dizi: (m, at, f, d, g) => dizi(m, at, f, d, g),
      pluck: (m, at, f, d, g) => guzheng(m, at, f, g, { decay: Math.max(1.4, d * 1.5) }),
    };

    // I. The candle, the cord, the unrolling.
    candleStrike(mix, B.candle, 0.5, random);
    pad(mix, 1.0, B.dive[1] + 0.4, ['D2', 'A2'], 0.18, { attack: 3, harmonics: 4 });
    [0.2, 0.7, 1.2].forEach((offset, i) => guzheng(mix, B.cordUntie[0] + offset, hz(['D6', 'A5', 'D6'][i]), 0.12, { decay: 3, brightness: 0.3 }));
    paperRustle(mix, B.unroll[0], B.unroll[1], 0.16, random);
    glissando(mix, B.dive[0] + 0.3, scaleRun(MINOR, 4, 11), 0.075, 0.2);
    whoosh(mix, B.dive[0], 2.8, 0.35, random);
    tamTam(mix, B.dive[1], 0.28, 1.6, 4);

    // II. Title: the legend begins.
    taiko(mix, B.dive[1] - 0.02, 1.0);
    taiko(mix, B.dive[1] + 0.45, 0.5, { pitch: 1.2 });
    glissando(mix, B.dive[1], scaleRun(MINOR, 4, 11, true), 0.05, 0.22);
    [0, 0.9, 1.6, 2.05].forEach((offset) => brushSwish(mix, B.titleBrush + offset, 0.6, 0.12, random));
    pad(mix, B.dive[1], B.toIthaca[0] + 1, ['D3', 'A3', 'F4'], 0.08, { attack: 2, harmonics: 5 });
    phrase(mix, said('legend') + 0.3, 72, [['D5', 0.5], ['F5', 0.5], ['G5', 1], ['A5', 1.5], ['C6', 0.5], ['A5', 0.5], ['G5', 1], ['F5', 0.5], ['G5', 0.5], ['D5', 2]], 0.2, play.dizi);
    for (let t = B.dive[1] + 0.8, k = 0; t < B.toIthaca[0]; t += 60 / 72, k++) guzheng(mix, t, hz(['D3', 'A3', 'D4', 'F4'][k % 4]), 0.1, { decay: 2.4 });
    stampThump(mix, B.titleSeal, 0.7, random);
    operaGong(mix, B.titleSeal + 0.02, 0.18);

    // III. Ithaca and the red thread.
    whoosh(mix, B.toIthaca[0], B.toIthaca[1] - B.toIthaca[0] + 0.4, 0.3, random);
    const ostinato = ['D4', 'A4', 'D5', 'F5', 'A4', 'D5', 'C5', 'A4'];
    for (let t = B.toIthaca[1] - 0.2, k = 0; t < B.toSea[0]; t += 60 / 84 / 2, k++) guzheng(mix, t, hz(ostinato[k % ostinato.length]), 0.1, { decay: 1.8, pan: k % 2 ? 0.3 : -0.3 });
    pad(mix, B.toIthaca[1], B.toSea[1], ['D3', 'F3', 'A3'], 0.07, { attack: 2.5 });
    tremolo(mix, B.pairing[0], B.pairing[1], 'A5', 0.07, 18);
    [['D6', 0], ['A6', 0.18], ['D7', 0.36]].forEach(([note, offset]) => jadeChime(mix, B.pairing[1] - 0.2 + offset, hz(note), 0.2));
    templeBell(mix, B.pairing[1], hz('D4'), 0.14);
    erhuLine(mix, B.departWalk[0], [['A4', 2], ['C5', 1], ['D5', 3], ['F5', 1], ['D5', 3]], 60 / 72, 0.1);

    // IV. The sea, ten years in a breath.
    whoosh(mix, B.toSea[0], B.toSea[1] - B.toSea[0] + 0.4, 0.3, random);
    sea(mix, B.toSea[0] - 0.4, B.toCave[0] + 1.2, 0.34, random);
    for (let t = B.years[0], k = 0; t < B.years[1]; t += 0.45, k++) {
      woodBlock(mix, t, 0.2 * (0.6 + 0.4 * (k % 2)), k % 2 ? 1.2 : 1);
      if (k % 2 === 0) guzheng(mix, t, hz(scaleRun(MINOR, 4, 12)[Math.floor(k / 2) % 12]), 0.08, { decay: 1.6 });
    }
    stampThump(mix, B.yearsSeal, 0.6, random);
    tamTam(mix, B.yearsSeal + 0.1, 0.14, 0.4, 3);
    erhuLine(mix, B.years[0] + 0.3, [['D5', 3], ['C5', 1.5], ['A4', 3.5]], 60 / 72, 0.08, { pan: 0.2 });

    // V. The lotus: forgetting.
    whoosh(mix, B.toLotus[0], B.toLotus[1] - B.toLotus[0] + 0.3, 0.22, random);
    for (let t = B.toLotus[1], k = 0; t < B.toCave[0]; t += 0.7, k++) guzheng(mix, t, hz(scaleRun(MINOR, 5, 10)[(k * 3) % 10]), 0.06, { decay: 3.4, brightness: 0.35, pan: Math.sin(k) * 0.6 });
    phrase(mix, said('lotus') + 0.8, 60, [['A5', 2], ['G5', 1], ['F5', 1], ['D5', 3]], 0.13, play.dizi);
    pad(mix, B.toLotus[1], B.toCave[0] + 1, ['F3', 'A3', 'C4'], 0.06, { attack: 2 });

    // VI. The giant's cave.
    whoosh(mix, B.toCave[0], B.toCave[1] - B.toCave[0] + 0.3, 0.26, random);
    pad(mix, B.toCave[1] - 0.5, B.sunrise, ['D2', 'A2', 'D3'], 0.14, { attack: 2, harmonics: 4 });
    noiseBed(mix, B.toCave[1] - 0.5, B.sunrise + 0.5, 0.05, random, { low: 60, high: 300, swellRate: 0.2 });
    for (let t = B.toCave[1], k = 0; t < B.sunrise; t += 1.05 + (k % 3) * 0.2, k++) {
      const drip = 1600 + (k % 4) * 300;
      addVoice(mix, t, 0.3, (s) => 0.12 * Math.sin(TWO_PI * (drip - s * 3000) * s) * Math.exp(-s / 0.05), (k % 2 ? 0.4 : -0.4), 0.8);
    }
    paperRustle(mix, B.craneQueued, B.craneQueued + 0.8, 0.12, random);
    glissando(mix, B.sunrise, scaleRun(MAJOR, 4, 13), 0.06, 0.2);
    pad(mix, B.sunrise, B.toCirce[1], ['D3', 'F#3', 'A3', 'D4'], 0.1, { attack: 1.2 });
    jadeChime(mix, B.sunrise + 0.8, hz('A6'), 0.18);
    flutter(mix, B.craneFlight[0], B.craneFlight[1], 0.1, random);

    // VII. Circe's hall.
    whoosh(mix, B.toCirce[0], B.toCirce[1] - B.toCirce[0] + 0.3, 0.24, random);
    const lilt = ['A3', 'D4', 'E4', 'F4', 'A4', 'F4', 'E4', 'D4', 'C4', 'D4', 'F4', 'A3'];
    for (let t = B.toCirce[1], k = 0; t < B.toSirens[0]; t += 60 / 66 / 1.5, k++) guzheng(mix, t, hz(lilt[k % lilt.length]), 0.1, { decay: 2.2, brightness: 0.4, pan: k % 3 === 0 ? -0.3 : 0.2 });
    for (let t = B.toCirce[1], k = 0; t < B.toSirens[0]; t += 0.9 + (k % 3) * 0.37, k++) jadeChime(mix, t, hz(['D7', 'A6', 'F6', 'C7'][k % 4]), 0.03, (k % 2 ? 0.6 : -0.6));
    pad(mix, B.toCirce[1], B.toSirens[0], ['D3', 'A3', 'C4'], 0.06);
    paperRustle(mix, B.askScroll, B.askScroll + 0.8, 0.1, random);
    stampThump(mix, B.approve, 0.7, random);
    operaGong(mix, B.approve + 0.02, 0.18, 460);
    [['D6', 0.2], ['F#6', 0.34], ['A6', 0.48]].forEach(([note, offset]) => jadeChime(mix, B.approve + offset, hz(note), 0.14));

    // VIII. The sirens.
    whoosh(mix, B.toSirens[0], B.toSirens[1] - B.toSirens[0] + 0.3, 0.28, random);
    sea(mix, B.toSirens[0], B.toProteus[0], 0.22, random);
    voicesPad(mix, B.toSirens[1], B.muteSeal, 0.16, ['D4', 'A4', 'C5', 'F5']);
    pad(mix, B.song[0], B.muteSeal, ['D3', 'D4'], 0.06, { tremoloRate: 11, attack: 3 });
    for (let t = B.song[0] - 1, interval = 0.5; t < B.muteSeal - 0.05; interval = Math.max(0.1, interval * 0.93)) {
      woodBlock(mix, t, 0.18, 1.3, 0.35);
      t += interval;
    }
    taiko(mix, B.muteSeal, 1.0);
    stampThump(mix, B.muteSeal, 0.9, random);
    tamTam(mix, B.muteSeal + 0.02, 0.3, 0.05, 3.5);
    pad(mix, B.muteSeal + 0.6, B.toProteus[1], ['D3', 'A3'], 0.05, { attack: 1.5 });

    // IX. The shape-shifter.
    whoosh(mix, B.toProteus[0], B.toProteus[1] - B.toProteus[0] + 0.3, 0.28, random);
    pad(mix, B.toProteus[1], B.toStorm[0], ['D2', 'A2', 'D3'], 0.08, { tremoloRate: 3.2, attack: 1 });
    B.proteusHits.forEach((at, i) => {
      taiko(mix, at, 1.0);
      taiko(mix, at + 0.2, 0.45, { pitch: 1.3 });
      glissando(mix, at - 0.05, scaleRun(MINOR, 3, 8, true), 0.02, 0.12);
      noiseBed(mix, at - 0.1, at + 0.35, 0.12, random, { low: 200, high: 1200, fade: 0.1 });
      void i;
    });
    stampThump(mix, B.sameSeal, 0.8, random);
    operaGong(mix, B.sameSeal + 0.02, 0.2, 380);
    [['D5', 0], ['A5', 0.1], ['D6', 0.2]].forEach(([note, offset]) => jadeChime(mix, B.sameSeal + 0.2 + offset, hz(note), 0.12));

    // X. The dragon of the storm.
    whoosh(mix, B.toStorm[0], B.toStorm[1] - B.toStorm[0] + 0.4, 0.3, random);
    tamTam(mix, B.dragon[0] + 0.3, 0.34, 2.2, 3);
    rain(mix, B.toStorm[0] + 0.5, B.snap, 0.05, random);
    noiseBed(mix, B.toStorm[0], B.snap, 0.12, random, { low: 80, high: 600, swellRate: 0.4 });
    dragonGrowl(mix, B.dragon[0] + 0.2, B.snap, 0.18, random);
    B.lightning.forEach((at) => thunder(mix, at, 0.5, random));
    drumPattern(mix, B.dragon[0] + 1.2, B.snap - 0.05, 132, 0.55, [1, 0.4, 0.6, 0.4, 1, 0.5, 0.8, 0.6], { crescendo: true });
    erhuLine(mix, B.dragon[0] + 1.6, [['D6', 2], ['C6', 1], ['D6', 1.5], ['F6', 2]], 60 / 90, 0.08, { pan: 0.25 });
    pad(mix, B.dragon[0], B.snap, ['D2', 'D3', 'G#3'], 0.07, { tremoloRate: 14, attack: 2 });
    stringSnap(mix, B.snap, 0.8, random);
    const landing = B.inkDrop[0] + 0.6 * (B.inkDrop[1] - B.inkDrop[0]);
    inkDropBoom(mix, landing, 0.8);

    // XI. Night at home. "Wait."
    pad(mix, B.toNight[0] + 0.4, B.grab, ['D3', 'A3', 'F4'], 0.06, { attack: 2.5 });
    noiseBed(mix, B.toNight[1], B.toBow[0], 0.02, random, { low: 3500, high: 6000, swellRate: 1.7 });
    erhuLine(mix, B.toNight[1] - 0.2, [['A4', 2], ['C5', 1], ['D5', 2.5], ['F5', 1], ['D5', 1.5], ['C5', 1], ['A4', 3]], 60 / 76, 0.09);
    pad(mix, B.reach[0], B.grab, ['D2', 'A2'], 0.1, { attack: 2.5, tremoloRate: 5 });
    for (let t = B.reach[0] + 0.5; t < B.grab - 0.1; t += 0.8) {
      taiko(mix, t, 0.22, { pitch: 0.8, send: 0.1 });
      taiko(mix, t + 0.2, 0.14, { pitch: 0.8, send: 0.1 });
    }
    templeBell(mix, B.grab + 0.1, hz('A3'), 0.18);
    tremolo(mix, B.reweave[0], B.reweave[1], 'D5', 0.05, 14);
    scaleRun(MAJOR, 4, 10).forEach((note, i) => guzheng(mix, B.reweave[0] + (i / 10) * (B.reweave[1] - B.reweave[0]), hz(note), 0.09, { decay: 2 }));
    for (let t = B.reweave[0]; t < B.reweave[1]; t += 0.26) woodBlock(mix, t, 0.07, 0.7, -0.4);
    ['D6', 'E6', 'F#6', 'A6'].forEach((note, i) => jadeChime(mix, B.lightsOn + i * 0.14, hz(note), 0.2, (i - 1.5) * 0.3));
    pad(mix, B.lightsOn, B.toBow[1], ['D3', 'F#3', 'A3'], 0.08, { attack: 1 });

    // XII. The bow and the twelve rings.
    whoosh(mix, B.toBow[0], B.toBow[1] - B.toBow[0] + 0.3, 0.28, random);
    drumPattern(mix, B.toBow[1], B.release - 0.1, 100, 0.5, [1, 0, 0.5, 0, 0.8, 0.4, 0.5, 0], { crescendo: true });
    pad(mix, B.draw[0], B.release, ['D2', 'A2', 'D3', 'A3'], 0.09, { attack: 4, tremoloRate: 8 });
    noiseBed(mix, B.draw[0], B.release, 0.04, random, { low: 200, high: 700, swellRate: 0.3 });
    whoosh(mix, B.release, 1.2, 0.5, random, 0.6, -0.6);
    taiko(mix, B.release, 1.0);
    stringSnap(mix, B.release, 0.3, random);
    drumPattern(mix, B.release + 0.3, cues.ringPasses[cues.ringPasses.length - 1], 120, 0.5, [1, 0.5, 0.7, 0.5], { crescendo: true });
    pad(mix, B.release, B.doneSeal + 2, ['D3', 'F#3', 'A3', 'D4'], 0.07, { attack: 3 });
    const ringNotes = scaleRun(MAJOR, 5, 12);
    cues.ringPasses.forEach((at, i) => {
      jadeChime(mix, at, hz(ringNotes[i]), 0.2, (i / 11 - 0.5) * 0.8);
      guzheng(mix, at, hz(ringNotes[i]), 0.14, { decay: 2.4, brightness: 0.8 });
    });
    taiko(mix, B.doneSeal, 1.0);
    taiko(mix, B.doneSeal + 0.18, 0.7, { pitch: 1.2 });
    stampThump(mix, B.doneSeal, 0.9, random);
    tamTam(mix, B.doneSeal + 0.02, 0.36, 0.05, 5);
    glissando(mix, B.doneSeal + 0.1, scaleRun(MAJOR, 4, 13, true), 0.045, 0.18);

    // XIII. One word, sent home.
    whoosh(mix, B.toContinue[0], B.toContinue[1] - B.toContinue[0] + 0.3, 0.22, random);
    sea(mix, B.toContinue[0], B.toLeave[1], 0.2, random);
    pad(mix, B.toContinue[1], B.toLeave[1], ['D3', 'A3', 'F#4'], 0.06, { attack: 2 });
    phrase(mix, said('word') + 0.5, 66, [['D5', 0.5], ['E5', 0.5], ['F#5', 1], ['A5', 1.5], ['B5', 0.5], ['A5', 0.5], ['F#5', 1], ['E5', 0.5], ['F#5', 0.5], ['D5', 2.5]], 0.18, play.dizi);
    flutter(mix, B.craneHome[0], B.craneHome[1], 0.08, random, 7);
    templeBell(mix, B.arrival, hz('D4'), 0.22);
    ['D6', 'F#6', 'A6', 'D7'].forEach((note, i) => jadeChime(mix, B.arrival + i * 0.12, hz(note), 0.14));

    // XIV. The work goes on. The scroll closes.
    whoosh(mix, B.toLeave[0], B.toLeave[1] - B.toLeave[0] + 0.3, 0.2, random);
    const arpeggio = ['D4', 'A4', 'D5', 'F#5', 'A5', 'F#5', 'D5', 'A4'];
    for (let t = B.toLeave[1], k = 0; t < B.rollUp[1]; t += 60 / 80 / 2, k++) guzheng(mix, t, hz(arpeggio[k % arpeggio.length]), 0.08 + 0.05 * progress(t, B.toLeave[1], B.pullBack[1]), { decay: 2, pan: k % 2 ? 0.3 : -0.3 });
    pad(mix, B.toLeave[1], B.title + 1, ['D3', 'A3', 'D4', 'F#4'], 0.09, { attack: 3, harmonics: 7 });
    phrase(mix, said('goes-on') + 0.4, 60, [['A5', 1], ['B5', 1], ['D6', 3], ['F#5', 1], ['A5', 3]], 0.17, play.dizi);
    paperRustle(mix, B.rollUp[0], B.rollUp[1], 0.14, random);
    glissando(mix, B.tie[0] + 0.6, scaleRun(MAJOR, 4, 13), 0.05, 0.16);
    taiko(mix, B.title, 1.0);
    taiko(mix, B.title + 0.25, 0.6, { pitch: 1.2 });
    tamTam(mix, B.title + 0.02, 0.3, 0.05, 5);
    ['D3', 'A3', 'D4', 'F#4', 'A4', 'D5'].forEach((note, i) => guzheng(mix, B.title + i * 0.035, hz(note), 0.14, { decay: 4 }));
    pad(mix, B.title, cues.duration, ['D2', 'D3', 'A3', 'F#4'], 0.1, { attack: 0.3, release: 0.1 });
    stampThump(mix, B.title + 0.9, 0.6, random);

    applyReverb(mix);
    applySilence(mix, [[B.snap + 0.3, landing - 0.03]]);
    master(mix, cues.duration - 2.6, cues.duration);
    return mix;
  }

  function encodeWav(mix) {
    const dataBytes = mix.length * 4;
    const buffer = new ArrayBuffer(44 + dataBytes);
    const view = new DataView(buffer);
    const text = (offset, value) => [...value].forEach((char, i) => view.setUint8(offset + i, char.charCodeAt(0)));
    text(0, 'RIFF');
    view.setUint32(4, 36 + dataBytes, true);
    text(8, 'WAVE');
    text(12, 'fmt ');
    view.setUint32(16, 16, true);
    view.setUint16(20, 1, true);
    view.setUint16(22, 2, true);
    view.setUint32(24, mix.sampleRate, true);
    view.setUint32(28, mix.sampleRate * 4, true);
    view.setUint16(32, 4, true);
    view.setUint16(34, 16, true);
    text(36, 'data');
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
    for (let i = 0; i < bytes.length; i += 0x8000) binary += String.fromCharCode.apply(null, bytes.subarray(i, i + 0x8000));
    return btoa(binary);
  }

  window.LegendScore = { SAMPLE_RATE, synthesize, encodeWav, renderWavBase64, instruments: { createMix, guzheng, dizi, pad, taiko } };
})();
