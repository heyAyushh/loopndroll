// Export the film's timing so the mix lands on the picture:
//   segments  - real time <-> story time (title cards freeze story time; the rest is undercranked)
//   footsteps - every foot plant, in story time, from the same gait math the picture uses
// -> build/audio/timing.json
const { chromium } = require('playwright-core');
const fs = require('fs');
const path = require('path');

(async () => {
  const browser = await chromium.launch({ channel: 'chrome', args: ['--allow-file-access-from-files'] });
  const page = await browser.newPage({ viewport: { width: 1920, height: 1080 } });
  await page.goto('file://' + path.join(__dirname, '..', 'index.html') + '?w=1920&h=1080');
  await page.evaluate(() => window.filmReady);
  const timing = await page.evaluate(() => ({
    duration: window.FILM.DURATION,
    undercrank: window.FILM.undercrank,
    segments: window.FILM.timeSegments,
    footsteps: window.FILM.footstepEvents(),
  }));
  const out = path.join(__dirname, '..', 'build', 'audio', 'timing.json');
  fs.mkdirSync(path.dirname(out), { recursive: true });
  fs.writeFileSync(out, JSON.stringify(timing, null, 1));
  console.log(`${timing.footsteps.length} footsteps, ${timing.segments.length} segments, undercrank ${timing.undercrank.toFixed(3)} -> ${out}`);
  await browser.close();
})();
