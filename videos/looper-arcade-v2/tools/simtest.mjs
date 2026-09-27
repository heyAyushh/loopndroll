// Dev probe: run the pinball sim and summarise its story events.
import { simulatePinball } from "../src/pinball.js";

const started = Date.now();
const sim = simulatePinball();
const counts = {};
for (const e of sim.events) counts[e.type] = (counts[e.type] || 0) + 1;
console.log("ms", Date.now() - started, "frames", sim.frames.length, counts);
const quiet = new Set(["bumper", "rail", "flip", "sling", "clack"]);
console.log(
  sim.events
    .filter((e) => !quiet.has(e.type))
    .map((e) => `${e.t.toFixed(2)} ${e.type}${e.task != null ? ":" + e.task : ""}`)
    .join("\n"),
);
