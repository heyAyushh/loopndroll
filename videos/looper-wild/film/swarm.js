// Kinetic type made of lines: agents drift in a flow field and swarm into each caption's letterforms,
// boil while they hold, and scatter back into the flow. Words are never typed; they assemble.
import { H, W, clamp, easeInOut, prng } from "./story.js";

const AGENTS = 3200;
const FONT = '-apple-system, "SF Pro Display", "Helvetica Neue", sans-serif';

// Sample the filled pixels of a (multi-line) caption as target points for the agents.
function letterPoints(lines, centreY) {
  const canvas = document.createElement("canvas");
  canvas.width = W; canvas.height = H;
  const g = canvas.getContext("2d");
  g.fillStyle = "#fff"; g.textAlign = "center"; g.textBaseline = "middle";
  const fitted = lines.map(([text, size]) => {
    let px = size;
    g.font = `900 ${px}px ${FONT}`;
    while (g.measureText(text).width > W * 0.9 && px > 20) { px -= 4; g.font = `900 ${px}px ${FONT}`; }
    return [text, px];
  });
  const total = fitted.reduce((sum, [, px]) => sum + px * 1.02, 0);
  let y = centreY * H - total / 2;
  for (const [text, px] of fitted) { g.font = `900 ${px}px ${FONT}`; g.fillText(text, W / 2, y + px * 0.51); y += px * 1.02; }
  const data = g.getImageData(0, 0, W, H).data;
  const found = [];
  for (let yy = 0; yy < H; yy += 4) for (let xx = 0; xx < W; xx += 4) if (data[(yy * W + xx) * 4 + 3] > 128) found.push([xx, yy]);
  const random = prng(lines.length * 97 + Math.round(centreY * 1000) + lines[0][0].length);
  for (let i = found.length - 1; i > 0; i -= 1) { const j = Math.floor(random() * (i + 1)); [found[i], found[j]] = [found[j], found[i]]; }
  return Array.from({ length: AGENTS }, (_, i) => found[i % Math.max(1, found.length)] ?? [W / 2, centreY * H]);
}

export function createSwarm(p, captions) {
  const random = prng(5);
  const agents = Array.from({ length: AGENTS }, () => ({ x: random() * W, y: random() * H, delay: random(), speed: 0.6 + random() * 0.8 }));
  const shapes = captions.map((c) => letterPoints(c.lines, c.y));
  const flow = (agent, t) => {
    const n = p.noise(agent.x * 0.002, agent.y * 0.002, t * 0.15) * Math.PI * 4;
    const drift = t * 38 * agent.speed;
    return [((agent.x + Math.cos(n) * drift) % W + W) % W, ((agent.y + Math.sin(n) * drift - t * 20) % H + H) % H];
  };
  return {
    draw(t, { energy = 0 } = {}) {
      const index = captions.findIndex((c) => t >= c.start && t < c.end + 0.3);
      const c = captions[index];
      const colour = c?.colour ?? [245, 245, 247];
      p.push();
      p.blendMode(p.ADD);
      for (let i = 0; i < AGENTS; i += 1) {
        const agent = agents[i];
        let [x, y] = flow(agent, t);
        let [px, py] = flow(agent, t - 0.05);
        let formed = 0;
        if (c) {
          const into = easeInOut(clamp((t - c.start - agent.delay * 0.2) / 0.26));
          const out = easeInOut(clamp((t - c.end + 0.05 - agent.delay * 0.2) / 0.25));
          formed = into * (1 - out);
          const [tx, ty] = shapes[index][i];
          const boil = 1.8 + energy * 5;
          const frame = Math.floor(t * 12);
          const jx = (p.noise(i * 0.7, frame * 0.3) - 0.5) * boil, jy = (p.noise(i * 0.7 + 50, frame * 0.3) - 0.5) * boil;
          x += (tx + jx - x) * formed; y += (ty + jy - y) * formed;
          px += (tx + jx - 1 - px) * formed; py += (ty + jy - 5 - py) * formed;
        }
        p.stroke(colour[0], colour[1], colour[2], 26 + formed * 210);
        p.strokeWeight(formed > 0.5 ? 3.4 : 1.3);
        p.line(px, py, x, y);
      }
      p.pop();
    },
  };
}
