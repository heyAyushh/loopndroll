// One film, two frames. Every screen-space position lives here; the stage itself is authored once.
//   reel: 1080x1920 (Instagram)  — type on top, stage below, full-bleed
//   x:    1920x1080 (Twitter/X)  — type in the left column, stage and the loop on the right
const FORMAT = window.FILM_FORMAT === "x" ? "x" : "reel";

const LAYOUTS = {
  reel: {
    width: 1080, height: 1920,
    caption: { x: 540, maxWidth: 0.9 * 1080, yScale: 1, fixedY: null },
    stage: { scale: 1.5, anchorX: 520, anchorY: 1330, x: 540, y: 1290 },
    orb: { drop: [540, 900], after: [540, 560], end: [540, 700] },
    prompt: [540, 650],
    bubbles: [[60, 900], [1020, 1040]],
    sun: [540, 1100, 780],
    tagline: [540, 1390, 1470],
    captionSizeScale: 1,
  },
  x: {
    width: 1920, height: 1080,
    caption: { x: 560, maxWidth: 1000, yScale: 1, fixedY: 0.42 },
    stage: { scale: 1.22, anchorX: 520, anchorY: 1330, x: 1360, y: 610 },
    orb: { drop: [1360, 540], after: [1400, 420], end: [1360, 400] },
    prompt: [560, 760],
    bubbles: [[900, 360], [1880, 500]],
    sun: [1400, 700, 420],
    tagline: [560, 700, 780],
    captionSizeScale: 0.72,
  },
};

export const LAYOUT = { format: FORMAT, ...LAYOUTS[FORMAT] };
