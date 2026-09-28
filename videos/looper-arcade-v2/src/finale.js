// Finale: arcade high-score table (held long enough to read), then the orb
// resolves from chunky pixels into the Looper logo, then the tagline.
import { createCanvas } from "@napi-rs/canvas";
import { T, prog, blinkOn, hash, orb } from "./core.js";

const F = T.finale;
const [SCENE_START] = T.scenes.finale;
const FINAL_SCORE = "9,999,990";
const ROWS = [
  ["1ST", null, null],
  ["2ND", "YOU, NO LOOPER", "120,400"],
  ["3RD", 'TYPING "CONTINUE" x40', "45,000"],
  ["4TH", "BABYSITTING TABS", "12,000"],
  ["5TH", "2AM FOLLOW-UPS", "3,100"],
  ["6TH", "AGENT GAVE UP", "0"],
];
const RAINBOW = ["#ffd23f", "#ff4a3d", "#7dff6a", "#7fe3ff", "#d63fb8"];
const ORB_STEPS = [[F.logo, 4], [F.logo + 0.1, 8], [F.logo + 0.2, 16], [F.logo + 0.32, 32], [F.logo + 0.45, 0]];
const LAYOUT = {
  landscape: { rowFont: "PS2P", rowSize: 16, nameX: 72, rowStep: 30, rowTop: 110, headerY: 50, orbSize: 112, orbY: 40, wordY: 172, tagY: 246 },
  portrait: { rowFont: "VT", rowSize: 20, nameX: 48, rowStep: 34, rowTop: 196, headerY: 130, orbSize: 160, orbY: 150, wordY: 340, tagY: 420 },
};

export function createFinale(format) {
  const L = LAYOUT[format];
  const stars = Array.from({ length: 160 }, (_, i) => ({ a: hash(i) * Math.PI * 2, speed: 0.3 + hash(i + 500) * 0.7, offset: hash(i + 900) }));
  const pixelOrb = createCanvas(L.orbSize, L.orbSize);
  const po = pixelOrb.getContext("2d");
  po.imageSmoothingEnabled = false;

  return {
    draw(p, t) {
      p.clear("#02030d");
      const warp = t >= F.logo ? 2.4 : 0.4;
      stars.forEach((s) => {
        const u = (s.offset + (t - SCENE_START) * s.speed * warp * 0.35) % 1;
        const r = u * u * Math.hypot(p.W, p.H) * 0.6;
        p.rect(u > 0.5 ? "#ffffff" : "#56609a", p.W / 2 + Math.cos(s.a) * r, p.H / 2 + Math.sin(s.a) * r * 0.7, u > 0.7 ? 2 : 1, 1);
      });

      if (t < F.logo) {
        const fade = 1 - prog(t, F.logo - 0.2, F.logo);
        if (Math.floor(fade * 4) <= 0) return;
        const header = Math.floor(prog(t, SCENE_START + 0.05, SCENE_START + 0.3) * 5) / 5;
        p.text("HIGH SCORES", p.W / 2, L.headerY - (1 - header) * 30, "#ffd23f", { size: 24, align: "center", shadow: "#b3261e", shadowOffset: 3 });
        const newHigh = t >= F.newHigh;
        const left = format === "portrait" ? 16 : 40;
        const right = p.W - left;
        ROWS.forEach(([rank, who, pts], i) => {
          const at = F.rows[i];
          if (t < at) return;
          const slide = Math.round((1 - prog(t, at, at + 0.15)) * 3) * 20;
          const y = L.rowTop + i * L.rowStep;
          let color = "#c8ccda";
          let name = who;
          let score = pts;
          if (i === 0) {
            name = newHigh ? "YOU + LOOPER" : "- - - - - -";
            score = newHigh ? FINAL_SCORE : "-------";
            color = newHigh ? RAINBOW[Math.floor(t * 10) % RAINBOW.length] : "#6b7390";
          }
          const style = { size: L.rowSize, font: L.rowFont };
          p.text(rank, left - slide, y, color, style);
          p.text(name, left + L.nameX - slide, y, color, style);
          p.text(score, right - slide, y, color, { ...style, align: "right" });
        });
        if (newHigh && blinkOn(t, 3)) p.text("NEW HIGH SCORE!", p.W / 2, L.rowTop + ROWS.length * L.rowStep + 16, "#7dff6a", { size: 16, align: "center" });
        return;
      }

      // logo view
      let res = 0;
      for (const [ts, r] of ORB_STEPS) if (t >= ts) res = r;
      const bob = Math.round(Math.sin((t - F.logo) * 2) * 3);
      const ox = p.W / 2 - L.orbSize / 2;
      const oy = L.orbY + bob;
      for (let r = L.orbSize * 0.75; r > L.orbSize * 0.5; r -= 6) p.alpha(0.08, () => p.disc("#7fe3ff", p.W / 2, oy + L.orbSize / 2, r));
      if (res > 0) {
        po.clearRect(0, 0, L.orbSize, L.orbSize);
        po.drawImage(orb(res), 0, 0, L.orbSize, L.orbSize);
        p.image(pixelOrb, ox, oy);
      } else p.image(orb(L.orbSize), ox, oy);
      if (t >= F.logo && t < F.logo + 0.15) p.alpha(1 - (t - F.logo) / 0.15, () => p.clear("#ffffff"));

      const letters = "LOOPER".split("");
      const letterSize = 48;
      const total = letters.length * letterSize + (letters.length - 1) * 4;
      letters.forEach((letter, i) => {
        const start = F.wordmark + i * 0.06;
        if (t < start) return;
        const u = prog(t, start, start + 0.35);
        const bounce = u < 1 ? Math.abs(Math.cos(u * Math.PI * 1.5)) * (1 - u) * 60 : 0;
        const x = p.W / 2 - total / 2 + i * (letterSize + 4) + letterSize / 2;
        p.text(letter, x, L.wordY - bounce, "#ffffff", { size: letterSize, align: "center", shadow: "#245edb", shadowOffset: 4 });
      });
      ["YOUR AGENTS STOP.", "THE GAME DOESN'T."].forEach((line, i) => {
        if (t >= F.tagline[i]) p.text(line, p.W / 2, L.tagY + i * 26, i ? "#ffd23f" : "#ffffff", { size: 16, align: "center", shadow: "#b3261e", shadowOffset: 2 });
      });
    },
  };
}
