// Live canvas textures: the monitor, phone, LED clock, sticky note, rain, city, and lens reflection.
// Each draw function is a pure function of t.
import { BAR, BEAT, BEATS, at, clamp, clockMinutes, clockText, looperOn, lerp, prng, smooth, stopCount } from "./story.js";

const MONO = '"SF Mono", Menlo, monospace';
const SANS = '-apple-system, "SF Pro Display", "Helvetica Neue", sans-serif';
const HAND = '"Marker Felt", "Bradley Hand", cursive';
export const ACCENT = "#b9a4ff";
const RUNNING_GREEN = "#7fe0a4";

function canvas(width, height) {
  const element = document.createElement("canvas");
  element.width = width;
  element.height = height;
  return element;
}

// ---------------------------------------------------------------- monitor

export const SCREEN_SIZE = { width: 2048, height: 860 };
const MENU_BAR = 40;

const SESSIONS = [
  {
    name: "codex · looper",
    work: [
      "› make reconnect resume by seq; keep tests green",
      "  reading crates/looper-client-core/src/session_transport.rs",
      "  editing state_mini_recovery.rs          +84 −31",
      "  editing local_store/persister.rs        +22 −9",
      "  cargo test -p looper-client-core        ok",
      "  editing tests/e2e_client_core.rs        +57 −4",
      "  next: run the h3 reconnect e2e, then lint",
    ],
  },
  { name: "claude-code · api", work: ["› migrate sessions table to v7", "  writing migration 0042_sessions_v7.sql", "  updating store/sessions.rs      +140 −62", "  next: backfill + run migration tests"] },
  { name: "cursor · web", work: ["› wire the pairing screen to the new API", "  editing app/pair/page.tsx       +96 −40", "  next: typecheck and lint"] },
  { name: "zed · docs", work: ["› document the launch checklist", "  editing docs/launch.md          +210", "  next: build the docs site"] },
];
const RESUME = ["↻ looper · keep going until the checks pass", "  test ✓", "  lint ✓", "  typecheck ✓", "  e2e ✓", "  committed · pushed"];

// How many seconds each pane's agent has actually been running by time t (it only advances while running).
function paneRunSeconds(pane, t) {
  const windows = pane === 0 ? [[0, 8], [11, 12], [12.5, 13], [13.5, 14], [14.5, 15], [27, 27.25]] : [[0, 6 - pane]];
  let total = 0;
  for (const [a, b] of windows) total += clamp(t - at(a), 0, at(b) - at(a));
  return total;
}
const resumeAt = (pane) => BEATS.click + pane * BEAT;

function paneLayout(t) {
  const top = MENU_BAR;
  const { width, height } = SCREEN_SIZE;
  const count = t < at(13) ? 1 : t < at(14) ? 2 : 4;
  if (count === 1) return [{ x: 0, y: top, w: width, h: height - top }];
  if (count === 2) return [{ x: 0, y: top, w: width / 2, h: height - top }, { x: width / 2, y: top, w: width / 2, h: height - top }];
  const w = width / 2, h = (height - top) / 2;
  return [0, 1, 2, 3].map((i) => ({ x: (i % 2) * w, y: top + Math.floor(i / 2) * h, w, h }));
}

function paneState(pane, t) {
  if (t >= BEATS.dawn - BAR * (1 - pane * 0.2)) return "done";
  if (t >= resumeAt(pane)) return "resumed";
  if (pane === 0 && agentsRunningPane0(t)) return "working";
  return "stopped";
}
const agentsRunningPane0 = (t) => [[0, 8], [11, 12], [12.5, 13], [13.5, 14], [14.5, 15], [27, 27.25]].some(([a, b]) => t >= at(a) && t < at(b));

function drawPane(g, pane, box, t, sub) {
  const session = SESSIONS[pane];
  const state = paneState(pane, t);
  const scale = box.h > 500 ? 1 : 0.78;
  const size = 30 * scale;
  const lineHeight = size * 1.55;
  g.save();
  g.beginPath(); g.rect(box.x, box.y, box.w, box.h); g.clip();
  g.fillStyle = "#0b0c17"; g.fillRect(box.x, box.y, box.w, box.h);
  // Pane title bar.
  g.fillStyle = state === "resumed" || state === "done" ? "rgba(185,164,255,0.16)" : "rgba(255,255,255,0.05)";
  g.fillRect(box.x, box.y, box.w, size * 1.6);
  g.font = `500 ${size * 0.72}px ${MONO}`;
  g.fillStyle = "rgba(200,202,230,0.85)";
  g.textBaseline = "middle";
  g.fillText(session.name, box.x + 24, box.y + size * 0.8);
  const status = { working: "running", stopped: `stopped · ${idleText(pane, t)}`, resumed: "running · looper", done: "done" }[state];
  g.textAlign = "right";
  g.fillStyle = state === "stopped" ? "rgba(255,255,255,0.55)" : state === "done" ? RUNNING_GREEN : state === "resumed" ? ACCENT : RUNNING_GREEN;
  g.fillText(status, box.x + box.w - 24, box.y + size * 0.8);
  g.textAlign = "left"; g.textBaseline = "top";
  // Body.
  g.font = `${size}px ${MONO}`;
  let row = 0;
  const lineY = (r) => box.y + size * 2.2 + r * lineHeight;
  const shown = Math.min(session.work.length, Math.floor(paneRunSeconds(pane, t) / (BEAT * 1.3)) + (pane === 0 ? 0 : session.work.length));
  session.work.slice(0, Math.max(0, shown)).forEach((line, i) => {
    g.fillStyle = line.startsWith("›") ? ACCENT : line.includes("ok") ? RUNNING_GREEN : i === session.work.length - 1 ? "rgba(160,164,200,0.8)" : "rgba(225,228,255,0.92)";
    g.fillText(line, box.x + 24, lineY(row++));
  });
  if (state === "stopped" || state === "resumed" || state === "done") {
    const promptAlpha = state === "stopped" ? 1 : 0.4;
    row += 0.4;
    g.fillStyle = `rgba(255,255,255,${promptAlpha})`;
    g.fillText("■ agent stopped. continue? [y/N]", box.x + 24, lineY(row++));
    if (state === "stopped") {
      const typedYes = [...BEATS.yeses, BEATS.habitYes].some((yes) => pane === 0 && t >= yes && t < yes + BEAT * 0.5);
      g.fillText(typedYes ? "› y" : "›", box.x + 24, lineY(row));
      if (Math.floor(t / (BEAT / 2)) % 2 === 0) { g.fillRect(box.x + 24 + size * (typedYes ? 2.4 : 1.2), lineY(row) + size * 0.1, size * 0.55, size); }
    }
  }
  if (state === "resumed" || state === "done") {
    const shownResume = Math.floor((t - resumeAt(pane)) / (BEAT * 0.9)) + 1;
    RESUME.slice(0, Math.max(0, shownResume)).forEach((line, i) => {
      const fresh = clamp(1 - (t - resumeAt(pane) - i * BEAT * 0.9) / (BEAT * 0.6));
      g.fillStyle = i === 0 ? ACCENT : RUNNING_GREEN;
      g.globalAlpha = 0.7 + 0.3 * fresh + 0.2 * sub;
      g.fillText(line, box.x + 24, lineY(row++));
      g.globalAlpha = 1;
    });
    if (state === "done") {
      g.font = `600 ${size * 1.25}px ${SANS}`; g.fillStyle = RUNNING_GREEN;
      g.fillText("✓ all checks passed", box.x + 24, lineY(row + 0.4));
    }
  }
  g.restore();
  // Pane border: squares, until they are running again.
  g.strokeStyle = state === "stopped" ? "rgba(255,255,255,0.22)" : "rgba(185,164,255,0.25)";
  g.lineWidth = 3; g.strokeRect(box.x + 1.5, box.y + 1.5, box.w - 3, box.h - 3);
}

function idleText(pane, t) {
  const stoppedAt = pane === 0 ? 167 : 150 + pane * 12;
  const idle = Math.max(1, clockMinutes(t) - stoppedAt);
  return idle >= 60 ? `${Math.floor(idle / 60)}h ${String(idle % 60).padStart(2, "0")}m` : `${idle}m`;
}

// The macOS menu bar and the Looper menu.
const POINTER_PATH = [[BEATS.pointer, 1020, 520], [BEATS.menuOpen - 0.1, 1846, 20], [BEATS.menuOpen + BEAT * 1.4, 1700, 222], [BEATS.click + 0.4, 1700, 222]];
export function pointerPosition(t) {
  for (let i = 1; i < POINTER_PATH.length; i += 1) {
    const [t0, x0, y0] = POINTER_PATH[i - 1];
    const [t1, x1, y1] = POINTER_PATH[i];
    if (t < t1) { const k = smooth(t0, t1, t); return [lerp(x0, x1, k), lerp(y0, y1, k)]; }
  }
  return POINTER_PATH[POINTER_PATH.length - 1].slice(1);
}

function drawMenuBar(g, t, sub, orbImage) {
  const { width } = SCREEN_SIZE;
  g.fillStyle = "rgba(30,30,40,0.96)"; g.fillRect(0, 0, width, MENU_BAR);
  g.font = `600 22px ${SANS}`; g.fillStyle = "rgba(235,235,245,0.9)"; g.textBaseline = "middle";
  g.fillText("Terminal", 28, MENU_BAR / 2);
  g.font = `400 22px ${SANS}`;
  ["Shell", "Edit", "View", "Window"].forEach((label, i) => g.fillText(label, 140 + i * 90, MENU_BAR / 2));
  g.textAlign = "right"; g.fillText(clockText(clockMinutes(t)), width - 24, MENU_BAR / 2); g.textAlign = "left";
  // Looper's orb in the menu bar: grey while off; lavender once it keeps the loop.
  const on = looperOn(t);
  g.save();
  g.globalAlpha = on ? 1 : 0.45;
  g.filter = on ? "none" : "grayscale(1)";
  if (on) { g.shadowColor = ACCENT; g.shadowBlur = 16 + 18 * sub; }
  g.drawImage(orbImage, 1834, 6, 28, 28);
  g.restore();
  const menuVisible = t >= BEATS.menuOpen && t < BEATS.click + BEAT * 0.6;
  if (menuVisible) {
    const x = 1560, y = MENU_BAR + 4, w = 440, h = 300;
    g.fillStyle = "rgba(40,40,54,0.97)"; g.beginPath(); g.roundRect(x, y, w, h, 14); g.fill();
    g.strokeStyle = "rgba(255,255,255,0.12)"; g.lineWidth = 2; g.stroke();
    g.font = `600 22px ${SANS}`; g.fillStyle = "#ffffff"; g.fillText("Looper", x + 24, y + 34);
    g.font = `400 20px ${SANS}`; g.fillStyle = "rgba(255,255,255,0.6)"; g.fillText("4 sessions stopped · MacBook Pro", x + 24, y + 66);
    g.fillStyle = "rgba(255,255,255,0.12)"; g.fillRect(x + 16, y + 92, w - 32, 2);
    const hover = t > BEATS.menuOpen + BEAT * 1.2;
    const picked = t >= BEATS.click;
    const rows = [["○  Stop when the agent stops", false], ["●  Keep going until checks pass", true], ["○  Ask me on iPhone", false]];
    rows.forEach(([label, target], i) => {
      const rowY = y + 110 + i * 56;
      if (target && hover) { g.fillStyle = picked ? ACCENT : "rgba(185,164,255,0.45)"; g.beginPath(); g.roundRect(x + 10, rowY, w - 20, 48, 10); g.fill(); }
      g.fillStyle = target && picked ? "#140f26" : "rgba(255,255,255,0.92)";
      g.font = `${target ? 600 : 400} 21px ${SANS}`; g.fillText(label, x + 26, rowY + 25);
    });
  }
  if (t >= BEATS.pointer && t < BEATS.click + 1.2) {
    const [px, py] = pointerPosition(t);
    g.fillStyle = "#ffffff"; g.strokeStyle = "#000"; g.lineWidth = 2;
    g.beginPath(); g.moveTo(px, py); g.lineTo(px, py + 34); g.lineTo(px + 9, py + 26); g.lineTo(px + 16, py + 40); g.lineTo(px + 22, py + 37); g.lineTo(px + 15, py + 24); g.lineTo(px + 26, py + 24); g.closePath(); g.fill(); g.stroke();
  }
  g.textBaseline = "alphabetic";
}

export function createScreen(orbImage) {
  const element = canvas(SCREEN_SIZE.width, SCREEN_SIZE.height);
  const g = element.getContext("2d");
  return {
    canvas: element,
    draw(t, sub) {
      g.fillStyle = "#07080f"; g.fillRect(0, 0, SCREEN_SIZE.width, SCREEN_SIZE.height);
      paneLayout(t).forEach((box, pane) => drawPane(g, pane, box, t, sub));
      drawMenuBar(g, t, sub, orbImage);
      g.fillStyle = "rgba(0,0,0,0.1)";
      for (let y = 0; y < SCREEN_SIZE.height; y += 4) g.fillRect(0, y, SCREEN_SIZE.width, 1);
    },
  };
}

// ---------------------------------------------------------------- phone

export const PHONE_SIZE = { width: 600, height: 1300 };
const REPLY = "yes, then ship";

export function createPhone(orbImage) {
  const element = canvas(PHONE_SIZE.width, PHONE_SIZE.height);
  const g = element.getContext("2d");
  const card = (y, head, title, body, iconIsOrb) => {
    g.fillStyle = "rgba(70,66,100,0.85)"; g.beginPath(); g.roundRect(30, y, 540, 170, 34); g.fill();
    if (iconIsOrb) g.drawImage(orbImage, 54, y + 24, 46, 46);
    else { g.fillStyle = "#e8e6f0"; g.beginPath(); g.roundRect(54, y + 24, 46, 46, 12); g.fill(); g.fillStyle = "#d64545"; g.font = `700 16px ${SANS}`; g.fillText("CAL", 60, y + 54); }
    g.font = `600 22px ${SANS}`; g.fillStyle = "rgba(240,238,250,0.8)"; g.fillText(head, 116, y + 54);
    g.font = `400 20px ${SANS}`; g.fillText("now", 500, y + 54);
    g.font = `600 27px ${SANS}`; g.fillStyle = "#ffffff"; g.fillText(title, 54, y + 108);
    g.font = `400 24px ${SANS}`; g.fillStyle = "rgba(255,255,255,0.82)"; g.fillText(body, 54, y + 146);
  };
  return {
    canvas: element,
    draw(t) {
      const { width, height } = PHONE_SIZE;
      const wallpaper = g.createLinearGradient(0, 0, 0, height);
      wallpaper.addColorStop(0, "#1d1a35"); wallpaper.addColorStop(1, "#0b0a16");
      g.fillStyle = wallpaper; g.fillRect(0, 0, width, height);
      g.textAlign = "center"; g.fillStyle = "#f2effc";
      g.font = `200 170px ${SANS}`; g.fillText(clockText(clockMinutes(t)), width / 2, 300);
      g.font = `500 28px ${SANS}`; g.fillStyle = "rgba(242,239,252,0.7)"; g.fillText("Friday — Launch day", width / 2, 350);
      g.textAlign = "left";
      if (t < at(20)) card(420, "CODE · CLAUDE-CODE", "Stopped: continue?", "api · waiting for input", false);
      else if (t < at(37)) card(420, "CALENDAR", "Launch Looper · 9:00", "in 2h 48m", false);
      else if (t < BEATS.wake) {
        card(420, "LOOPER", "claude-code asks", "Also run the migration?", true);
        const typed = Math.round(REPLY.length * smooth(BEATS.reply, BEATS.reply + BEAT * 1.8, t));
        g.fillStyle = "rgba(255,255,255,0.12)"; g.beginPath(); g.roundRect(30, 620, 540, 84, 42); g.fill();
        g.font = `400 28px ${SANS}`; g.fillStyle = typed ? "#ffffff" : "rgba(255,255,255,0.45)";
        g.fillText(typed ? REPLY.slice(0, typed) : "Reply…", 64, 672);
        const sent = t > BEATS.reply + BEAT * 2.2;
        g.fillStyle = sent ? ACCENT : "rgba(185,164,255,0.5)"; g.beginPath(); g.arc(528, 662, 28, 0, Math.PI * 2); g.fill();
        if (sent) { g.font = `500 24px ${SANS}`; g.fillStyle = RUNNING_GREEN; g.fillText("● same session · running", 64, 760); }
      } else card(420, "LOOPER", "Shipped · 08:52", "4 sessions done · all checks passed", true);
    },
  };
}

// ---------------------------------------------------------------- small props

export function createClock() {
  const element = canvas(512, 160);
  const g = element.getContext("2d");
  return {
    canvas: element,
    draw(t) {
      g.fillStyle = "#050507"; g.fillRect(0, 0, 512, 160);
      g.font = `500 116px ${MONO}`; g.textAlign = "center"; g.textBaseline = "middle";
      g.fillStyle = "rgba(255,255,255,0.05)"; g.fillText("88:88", 256, 84);
      g.shadowColor = "#e4ddff"; g.shadowBlur = 18; g.fillStyle = "#ece8ff";
      g.fillText(clockText(clockMinutes(t)), 256, 84);
      g.shadowBlur = 0;
    },
  };
}

export function createNote() {
  const element = canvas(256, 256);
  const g = element.getContext("2d");
  g.fillStyle = "#e9e1a6"; g.fillRect(0, 0, 256, 256);
  g.fillStyle = "rgba(0,0,0,0.06)"; g.fillRect(0, 0, 256, 34);
  g.fillStyle = "#23202a"; g.textAlign = "center";
  g.font = `600 50px ${HAND}`; g.fillText("LAUNCH", 128, 120);
  g.font = `700 70px ${HAND}`; g.fillText("9:00", 128, 196);
  g.strokeStyle = "#23202a"; g.lineWidth = 4; g.beginPath(); g.moveTo(50, 214); g.lineTo(206, 208); g.stroke();
  return { canvas: element };
}

// Rain on the window glass: streaks run and beads sit, until the rain stops.
export function createRain() {
  const element = canvas(1024, 768);
  const g = element.getContext("2d");
  const random = prng(8);
  const streaks = Array.from({ length: 140 }, () => ({ x: random(), speed: 0.25 + random() * 0.6, phase: random(), length: 0.03 + random() * 0.08 }));
  const beads = Array.from({ length: 500 }, () => ({ x: random(), y: random(), r: 0.6 + random() * 2.2 }));
  return {
    canvas: element,
    draw(t) {
      g.clearRect(0, 0, 1024, 768);
      const wet = 1 - smooth(BEATS.rainStops - BAR, BEATS.rainStops + BAR * 2, t);
      const falling = 1 - smooth(BEATS.rainStops - BAR, BEATS.rainStops, t);
      g.fillStyle = `rgba(200,210,255,${0.35 * wet})`;
      beads.forEach((b) => { g.beginPath(); g.arc(b.x * 1024, b.y * 768, b.r, 0, Math.PI * 2); g.fill(); });
      g.strokeStyle = `rgba(210,220,255,${0.45 * falling})`; g.lineWidth = 1.5;
      streaks.forEach((s) => {
        const y = ((s.phase + t * s.speed * 0.35) % 1.2) - 0.1;
        g.beginPath(); g.moveTo(s.x * 1024, y * 768); g.lineTo(s.x * 1024 + 2, (y + s.length) * 768); g.stroke();
      });
    },
  };
}

// The city across the street; one window holds another developer at another desk.
export function createCity(dawn, layer) {
  const element = canvas(4096, 1400);
  const g = element.getContext("2d");
  const random = prng(dawn ? 71 : 70);
  const sky = g.createLinearGradient(0, 0, 0, 1400);
  if (dawn) { sky.addColorStop(0, "#5c6aa8"); sky.addColorStop(0.5, "#e59a7a"); sky.addColorStop(0.85, "#ffd49e"); sky.addColorStop(1, "#ffe3b8"); }
  else { sky.addColorStop(0, "#070917"); sky.addColorStop(0.6, "#11142b"); sky.addColorStop(1, "#1f2140"); }
  if (layer === "sky") { g.fillStyle = sky; g.fillRect(0, 0, 4096, 1400); return { canvas: element }; }
  let x = 0;
  const rng = prng(3); // the same skyline at night and at dawn
  while (x < 4096) {
    const w = 120 + rng() * 260, h = 250 + rng() * 700;
    g.fillStyle = dawn ? "#3b2f47" : "#06060d"; g.fillRect(x, 1400 - h, w, h);
    for (let wy = 1400 - h + 30; wy < 1380; wy += 38) for (let wx = x + 18; wx < x + w - 20; wx += 32) {
      const lit = rng();
      if (!dawn && lit > 0.72) { g.fillStyle = `rgba(255,${190 + lit * 40},${120 + lit * 60},${0.35 + random() * 0.5})`; g.fillRect(wx, wy, 14, 18); }
      if (dawn && lit > 0.93) { g.fillStyle = "rgba(255,230,190,0.25)"; g.fillRect(wx, wy, 14, 18); }
    }
    x += w + 10;
  }
  return { canvas: element };
}

export function createNeighbour() {
  const element = canvas(512, 512);
  const g = element.getContext("2d");
  return {
    canvas: element,
    draw(t) {
      const awake = 1 - smooth(BEATS.dawn - BAR, BEATS.dawn, t);
      g.fillStyle = "#0a0a12"; g.fillRect(0, 0, 512, 512);
      const glow = g.createRadialGradient(300, 260, 10, 300, 260, 300);
      glow.addColorStop(0, `rgba(170,180,255,${0.75 * awake})`); glow.addColorStop(1, `rgba(40,44,80,${0.3 * awake})`);
      g.fillStyle = glow; g.fillRect(0, 0, 512, 512);
      g.fillStyle = `rgba(220,226,255,${0.9 * awake})`; g.fillRect(250, 200, 150, 90);  // their monitor
      // Their stop, too: a blinking square.
      if (Math.floor(t / (BEAT / 2)) % 2 === 0) { g.fillStyle = `rgba(20,20,30,${awake})`; g.fillRect(262, 256, 14, 18); }
      g.fillStyle = "#04040a"; // the figure
      g.beginPath(); g.arc(190, 250, 34, 0, Math.PI * 2); g.fill();
      g.beginPath(); g.moveTo(120, 512); g.quadraticCurveTo(130, 300, 190, 290); g.quadraticCurveTo(250, 300, 262, 512); g.fill();
      g.fillStyle = "#04040a"; g.fillRect(0, 0, 512, 16); g.fillRect(0, 496, 512, 16); g.fillRect(0, 0, 16, 512); g.fillRect(496, 0, 16, 512); g.fillRect(248, 0, 16, 512);
    },
  };
}

// What the glasses reflect: the terminal (mirrored), or at the end the sunrise in the window grid.
export function createLens(screen) {
  const element = canvas(512, 512);
  const g = element.getContext("2d");
  return {
    canvas: element,
    draw(t) {
      g.fillStyle = "#000"; g.fillRect(0, 0, 512, 512);
      if (t < BEATS.dawn) {
        // The monitor, mirrored and bent by the curved lens, brightened the way a reflection reads at night.
        g.save(); g.translate(512, 0); g.scale(-1, 1);
        g.beginPath(); g.arc(256, 256, 256, 0, Math.PI * 2); g.clip();
        for (let band = 0; band < 16; band += 1) {
          const y = band * 32;
          const bulge = Math.sin((band + 0.5) / 16 * Math.PI) * 60;
          g.drawImage(screen.canvas, 0, 60 + band * 38, 1100, 38, -bulge, y, 512 + bulge * 2, 32);
        }
        g.globalCompositeOperation = "lighter"; g.globalAlpha = 0.8;
        for (let band = 0; band < 16; band += 1) {
          const bulge = Math.sin((band + 0.5) / 16 * Math.PI) * 60;
          g.drawImage(screen.canvas, 0, 60 + band * 38, 1100, 38, -bulge, band * 32, 512 + bulge * 2, 32);
        }
        g.restore();
        g.fillStyle = "rgba(90,100,170,0.18)"; g.fillRect(0, 0, 512, 512);
      } else {
        const sky = g.createLinearGradient(0, 0, 0, 512);
        sky.addColorStop(0, "#6c6fa8"); sky.addColorStop(1, "#ffcf98"); g.fillStyle = sky; g.fillRect(0, 0, 512, 512);
        g.fillStyle = "#fff4dc"; g.shadowColor = "#ffd49e"; g.shadowBlur = 60;
        g.beginPath(); g.arc(256, 300, 90, 0, Math.PI * 2); g.fill(); g.shadowBlur = 0;
        g.fillStyle = "#1a1418"; for (let i = 0; i < 4; i += 1) { g.fillRect(i * 170 - 10, 0, 18, 512); g.fillRect(0, i * 170 - 10, 512, 18); }
      }
      const fresnel = g.createRadialGradient(256, 256, 80, 256, 256, 300);
      fresnel.addColorStop(0, "rgba(0,0,0,0.25)"); fresnel.addColorStop(1, "rgba(0,0,0,0.85)");
      g.fillStyle = fresnel; g.fillRect(0, 0, 512, 512);
    },
  };
}
