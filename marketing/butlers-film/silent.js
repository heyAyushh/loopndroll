/* Chaplin treatment: a silent film.
 * - Title cards cut in full-frame; story time freezes behind them, like the intertitles of the era.
 * - Outside the cards the story runs undercranked (sped up), except the end card which plays at speed for reading.
 * - Black-and-white print with grain, flicker, scratches, dust and gate weave, all seeded from the 24 fps frame number.
 * - The world stays monochrome until the Looper end card, where colour arrives.
 * Real time t maps to story time s; the scenes in film.js only ever see s. */
'use strict';

const REAL_DURATION = 82;
const STORY_DURATION = 75;
const END_CARD_STORY = 68.5;
const PROJECTOR_FPS = 24;

/** [story time the card cuts in at, seconds on screen, chapter label | null, chapter title | null, lines[], style] */
const TITLE_CARDS = [
  [4.8, 3.2, 'CHAPTER ONE', 'The Chair', ['The developer had not left', 'the chair in three days.']],
  [20.1, 3.4, null, null, ['They had done a great deal of work.', 'Then they stopped.', 'They would not proceed without permission.']],
  [31.8, 3.0, 'CHAPTER TWO', 'The Coconut', ['On the fourth day, a friend', 'returned from the beach.']],
  [36.2, 1.8, null, null, ['“You’re still here?”'], 'dialogue'],
  [37.6, 2.4, null, null, ['“They won’t proceed', 'without permission.”'], 'reply'],
  [39.75, 3.0, 'CHAPTER THREE', 'The Explanation', ['“Get Looper.', 'It’s an app on your Mac.”'], 'dialogue'],
  [60.5, 2.6, 'CHAPTER FOUR', 'The Door', ['The developer stood up.']],
  [64.05, 2.4, null, null, ['It was, by all accounts,', 'a lovely day.']],
];
const CARD_SECONDS = TITLE_CARDS.reduce((sum, card) => sum + card[1], 0);
/** Undercrank: how much faster than life the story plays between cards. */
const UNDERCRANK = END_CARD_STORY / (REAL_DURATION - (STORY_DURATION - END_CARD_STORY) - CARD_SECONDS);

/** Timeline segments: plays advance story time, cards hold it. */
const TIME_SEGMENTS = (() => {
  const segments = [];
  let real = 0, story = 0;
  const play = (toStory, speed) => { const seconds = (toStory - story) / speed; segments.push({ kind: 'play', realStart: real, realEnd: real + seconds, storyStart: story, storyEnd: toStory, speed }); real += seconds; story = toStory; };
  TITLE_CARDS.forEach((card, index) => {
    play(card[0], UNDERCRANK);
    segments.push({ kind: 'card', realStart: real, realEnd: real + card[1], storyStart: story, storyEnd: story, card: index });
    real += card[1];
  });
  play(END_CARD_STORY, UNDERCRANK);
  play(STORY_DURATION, 1);
  return segments;
})();

function storyAt(t) {
  const segment = TIME_SEGMENTS.find(s => t >= s.realStart && t < s.realEnd) || TIME_SEGMENTS[TIME_SEGMENTS.length - 1];
  if (segment.kind === 'card') return { story: segment.storyStart, card: segment.card, cardProgress: (t - segment.realStart) / (segment.realEnd - segment.realStart) };
  return { story: Math.min(segment.storyEnd, segment.storyStart + (t - segment.realStart) * segment.speed), card: -1 };
}

// ---------------------------------------------------------------- the print: reel wrapper, cards, film damage
(function buildSilentFilm() {
  // captions and chapter plaques are replaced by title cards
  SUBTITLES.length = 0;
  CHAPTERS.length = 0;
  const reel = document.createElement('div');
  reel.id = 'reel';
  // the reel carries its own backdrop so whip pans never expose the unfiltered (pink) stage behind it
  reel.style.cssText = 'position:absolute;inset:0;overflow:hidden;will-change:filter,transform;background:#EBA9A6';
  while (stage.firstChild) reel.appendChild(stage.firstChild);
  stage.appendChild(reel);

  const ink = '#EDE6D6';
  // double-rule frame with notched corner ornaments, drawn in pixels so nothing stretches in either aspect ratio
  const inset = Math.round(Math.min(W, H) * 0.05), gap = Math.round(10 * UNIT), notch = Math.round(34 * UNIT);
  const cornerOrnament = (cx, cy, sx, sy) => `<path d="M${cx},${cy + sy * notch} A${notch},${notch} 0 0 ${sx * sy > 0 ? 1 : 0} ${cx + sx * notch},${cy}" fill="none" stroke="${ink}" stroke-width="${2 * UNIT}"/>
      <path d="M${cx + sx * notch * 0.5},${cy + sy * notch * 0.5} l${sx * 7 * UNIT},${sy * -7 * UNIT} l${sx * 7 * UNIT},${sy * 7 * UNIT} l${sx * -7 * UNIT},${sy * 7 * UNIT} Z" fill="${ink}"/>`;
  const flourish = `<svg width="${W}" height="${H}" style="position:absolute;inset:0">
      <rect x="${inset}" y="${inset}" width="${W - inset * 2}" height="${H - inset * 2}" fill="none" stroke="${ink}" stroke-width="${2.5 * UNIT}"/>
      <rect x="${inset + gap}" y="${inset + gap}" width="${W - (inset + gap) * 2}" height="${H - (inset + gap) * 2}" fill="none" stroke="${ink}" stroke-width="${1 * UNIT}"/>
      ${cornerOrnament(inset + gap, inset + gap, 1, 1)}${cornerOrnament(W - inset - gap, inset + gap, -1, 1)}
      ${cornerOrnament(inset + gap, H - inset - gap, 1, -1)}${cornerOrnament(W - inset - gap, H - inset - gap, -1, -1)}</svg>`;
  const cardLayer = document.createElement('div');
  cardLayer.id = 'title-cards';
  cardLayer.className = 'layer';
  cardLayer.style.cssText = 'position:absolute;inset:0;background:#0E0D0C;display:none';
  cardLayer.innerHTML = `${flourish}
    <div id="card-text" style="position:absolute;inset:0;display:flex;flex-direction:column;align-items:center;justify-content:center;text-align:center;color:${ink};padding:0 ${px(PORTRAIT ? 90 : 220)}"></div>`;
  reel.appendChild(cardLayer);

  // silent-film title card for the opening
  const titleStyle = document.createElement('style');
  titleStyle.textContent = `#title-card{background:#0E0D0C !important}#title-card .card-frame{border-color:${ink} !important}#title-card .card-inner{color:${ink} !important}
    #title-card .title-main{font-family:Baskerville,'Didot',serif;letter-spacing:0.08em${PORTRAIT ? `;font-size:${px(78)} !important` : ''}}#title-card div[style*="background:#7C2935"]{background:${ink} !important}`;
  document.head.appendChild(titleStyle);

  // film damage overlay (never filtered, sits on top of the print)
  const fx = document.createElement('div');
  fx.id = 'film-fx';
  fx.style.cssText = 'position:absolute;inset:0;pointer-events:none;overflow:hidden';
  fx.innerHTML = `<svg width="${W}" height="${H}" style="position:absolute;inset:0;mix-blend-mode:overlay;opacity:0.34">
      <filter id="grain-filter" x="0" y="0" width="100%" height="100%"><feTurbulence id="grain-noise" type="fractalNoise" baseFrequency="${(0.9 / UNIT).toFixed(3)}" numOctaves="2" seed="1" stitchTiles="stitch"/><feColorMatrix type="saturate" values="0"/></filter>
      <rect width="100%" height="100%" filter="url(#grain-filter)"/></svg>
    ${Array.from({ length: 4 }, (_, i) => `<div class="scratch" id="scratch-${i}" style="position:absolute;top:0;bottom:0;width:${Math.max(1, Math.round(1.6 * UNIT))}px;opacity:0"></div>`).join('')}
    ${Array.from({ length: 7 }, (_, i) => `<div class="dust" id="dust-${i}" style="position:absolute;border-radius:50%;opacity:0"></div>`).join('')}
    <div id="film-vignette" style="position:absolute;inset:0;background:radial-gradient(ellipse at center, rgba(0,0,0,0) 55%, rgba(8,6,4,0.62) 100%)"></div>`;
  stage.appendChild(fx);
  // the old warm vignette is replaced by the print vignette; the iris closes to black
  $('vignette').style.display = 'none';
})();

function showTitleCard(index, cardProgress) {
  const layer = $('title-cards');
  if (index < 0) { layer.style.display = 'none'; return; }
  layer.style.display = 'block';
  const [, , chapter, title, lines, style] = TITLE_CARDS[index];
  const text = $('card-text');
  const key = String(index);
  if (text.dataset.card !== key) {
    const bodySize = px(PORTRAIT ? (lines.length > 2 ? 58 : 66) : (lines.length > 2 ? 58 : 70));
    text.innerHTML = `${chapter ? `<div style="font-family:Futura,sans-serif;font-weight:500;letter-spacing:0.42em;font-size:${px(28)};margin-bottom:${px(10)}">${chapter}</div>
        <div style="font-family:Baskerville,Didot,serif;font-size:${px(PORTRAIT ? 92 : 88)};line-height:1.05;margin-bottom:${px(24)}">${title}</div>
        <div style="width:${px(160)};height:2px;background:#EDE6D6;margin-bottom:${px(34)}"></div>` : ''}
      ${lines.map(line => `<div style="font-family:Baskerville,Didot,serif;font-style:${style === 'reply' ? 'italic' : 'normal'};font-size:${bodySize};line-height:1.28">${line}</div>`).join('')}`;
    text.dataset.card = key;
  }
  // a card breathes very slightly, like a card on a copy stand
  text.style.transform = `scale(${1 + cardProgress * 0.02})`;
}

function seeded24(frame, salt) { return seeded(frame * 17.13 + salt * 3.71); }

function applyFilmPrint(t, story) {
  const frame = Math.floor(t * PROJECTOR_FPS);
  const colour = EASE.inout(progress(story, END_CARD_STORY + 0.3, END_CARD_STORY + 1.3));   // colour arrives on the end card
  const mono = 1 - colour;
  const flicker = 1 + (seeded24(frame, 1) - 0.5) * 0.07 * mono;
  const reel = $('reel');
  reel.style.filter = `grayscale(${mono}) sepia(${0.22 * mono}) contrast(${1 + 0.14 * mono}) brightness(${flicker})`;
  const weave = mono * UNIT;
  reel.style.transform = `translate(${((seeded24(frame, 2) - 0.5) * 2.4 * weave).toFixed(2)}px,${((seeded24(frame, 3) - 0.5) * 3.2 * weave).toFixed(2)}px)`;
  $('grain-noise').setAttribute('seed', String(frame % 97));
  $('film-fx').style.opacity = String(0.35 + 0.65 * mono);
  for (let i = 0; i < 4; i++) {
    const el = $(`scratch-${i}`);
    const live = seeded24(frame, 10 + i) < 0.28 * mono;
    el.style.opacity = live ? String(0.25 + seeded24(frame, 20 + i) * 0.4) : '0';
    el.style.left = `${(seeded24(Math.floor(frame / 3), 30 + i) * 100).toFixed(2)}%`;
    el.style.background = i % 2 ? 'rgba(20,16,12,0.8)' : 'rgba(250,244,230,0.8)';
  }
  for (let i = 0; i < 7; i++) {
    const el = $(`dust-${i}`);
    const live = seeded24(frame, 40 + i) < 0.2 * mono;
    const size = (2 + seeded24(frame, 50 + i) * 7) * UNIT;
    el.style.opacity = live ? '0.75' : '0';
    el.style.width = `${size}px`; el.style.height = `${size * (0.6 + seeded24(frame, 60 + i))}px`;
    el.style.left = `${seeded24(frame, 70 + i) * 100}%`; el.style.top = `${seeded24(frame, 80 + i) * 100}%`;
    el.style.background = i % 3 ? '#15110D' : '#F4EEDF';
  }
}

// ---------------------------------------------------------------- seek: real time → story time
const storySeek = window.seek;
window.seek = function seek(t) {
  // the projector advances in whole frames
  const frameTime = Math.floor(t * PROJECTOR_FPS + 1e-6) / PROJECTOR_FPS;
  const { story, card, cardProgress } = storyAt(frameTime);
  storySeek(story);
  showTitleCard(card, cardProgress || 0);
  applyFilmPrint(frameTime, story);
};
window.FILM.DURATION = REAL_DURATION;
window.FILM.timeSegments = TIME_SEGMENTS;
window.FILM.undercrank = UNDERCRANK;
