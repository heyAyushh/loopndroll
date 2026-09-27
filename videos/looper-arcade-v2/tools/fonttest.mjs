// Dev probe: find the glyph rasterisation that keeps Press Start 2P pixel-exact.
import { createCanvas } from "@napi-rs/canvas";
import { writeFileSync } from "node:fs";
import "../src/core.js";

const canvas = createCanvas(360, 120);
const c = canvas.getContext("2d");
c.fillStyle = "#000";
c.fillRect(0, 0, 360, 120);
c.fillStyle = "#fff";
let y = 4;
for (const [baseline, offset] of [["top", 0], ["alphabetic", 8], ["alphabetic", 7], ["middle", 4]]) {
  c.font = "8px PS2P";
  c.textBaseline = baseline;
  c.fillText(`LOOPER CADET ${baseline}${offset}`, 2, y + offset);
  y += 14;
}
c.font = "16px PS2P";
c.textBaseline = "alphabetic";
c.fillText("LOOPER 16", 2, y + 16);
const m = createCanvas(4, 4).getContext("2d");
m.font = "8px PS2P";
const metrics = m.measureText("L");
console.log(metrics.actualBoundingBoxAscent, metrics.actualBoundingBoxDescent, metrics.fontBoundingBoxAscent, metrics.fontBoundingBoxDescent);
const big = createCanvas(1080, 360);
const bc = big.getContext("2d");
bc.imageSmoothingEnabled = false;
bc.drawImage(canvas, 0, 0, 1080, 360);
writeFileSync("build/fonttest.png", big.toBuffer("image/png"));
