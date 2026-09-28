// Debug probe: prints joint world positions at given times.
import { chromium } from "playwright-core";
import { readFileSync } from "node:fs";
import { serve } from "./serve.mjs";

const server = await serve(process.cwd());
const browser = await chromium.launch({ executablePath: "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", headless: true, args: ["--use-angle=metal"] });
const page = await browser.newPage({ viewport: { width: 1920, height: 1080 } });
await page.goto(`${server.url}film/index.html`);
await page.waitForFunction(() => window.filmReady === true);
const envelope = JSON.parse(readFileSync("assets/envelope.json", "utf8"));
const orb = `data:image/png;base64,${readFileSync("assets/orb.png").toString("base64")}`;
await page.evaluate(([e, o]) => window.setup(e, o), [envelope, orb]);
for (const t of process.argv.slice(2).map(Number)) {
  console.log(t, JSON.stringify(await page.evaluate((time) => window.probe(time), t)));
}
await browser.close();
server.close();
