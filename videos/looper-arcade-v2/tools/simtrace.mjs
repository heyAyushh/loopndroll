// Dev probe: print ball positions every quarter second.
import { simulatePinball } from "../src/pinball.js";

const sim = simulatePinball();
for (let i = 0; i < sim.frames.length; i += 15) {
  const f = sim.frames[i];
  console.log(f.t.toFixed(2), f.balls.map((b) => `(${b.x.toFixed(0)},${b.y.toFixed(0)})`).join(" "));
}
