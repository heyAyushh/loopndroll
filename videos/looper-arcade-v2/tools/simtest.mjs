// Dev probe + self-check: run the pinball sim, summarise its story, and assert
// the invariants the video relies on.
import assert from "node:assert/strict";
import { simulatePinball, TASKS } from "../src/pinball.js";

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

const first = (type) => sim.events.find((e) => e.type === type)?.t;
const last = sim.frames[sim.frames.length - 1];
// story order
assert.ok(first("agentStop") < first("save") && first("save") < first("kickback") && first("kickback") < first("jackpot"), "story beats out of order");
// the pinball total must equal the finale's high score, even with multiball still running
assert.equal(last.score, 9999990, "final score drifted from the high-score table");
assert.equal(last.tasksDone, TASKS.length, "not every task got smashed");
assert.ok(last.message.text.startsWith("JACKPOT"), `panel message overwritten after jackpot: ${last.message.text}`);
// nothing scores after the jackpot
const jackpotAt = first("jackpot");
assert.ok(sim.events.every((e) => e.t <= jackpotAt || !e.pts), "points awarded after the jackpot");
// never two balls parked in the plunger lane at once
for (const f of sim.frames) assert.ok(f.balls.filter((b) => b.held).length <= 1, `two balls in the lane at ${f.t}`);
// ball ids are unique per frame (trails look up by id)
for (const f of sim.frames) assert.equal(new Set(f.balls.map((b) => b.id)).size, f.balls.length);
console.log("sim checks passed");
