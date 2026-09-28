/* Legend of the Looper — piying shadow puppets: translucent leather, cut-outs, rods. */
(function () {
  'use strict';

  const L = window.Legend;
  const { COLOR, RGB, clamp, lerp, seededRandom, createSurface, fillCircle, fillEllipse, polygonPath, fillPolygon, strokeLine, catmullRom, drawGlow, brushStroke } = L;

  const BODY = { thigh: 102, shin: 98, upperArm: 76, forearm: 70, torso: 150, headOffset: 36, hand: 9 };

  const COSTUME = {
    king: { robe: 'rgba(38, 54, 100, 0.84)', trim: 'rgba(196, 140, 52, 0.9)', skin: 'rgba(120, 72, 38, 0.92)', hat: 'rgba(30, 38, 70, 0.92)', head: 'plumes', beard: true },
    beggar: { robe: 'rgba(104, 76, 48, 0.86)', trim: 'rgba(70, 52, 34, 0.9)', skin: 'rgba(120, 72, 38, 0.92)', hat: 'rgba(80, 60, 40, 0.92)', head: 'hood', beard: true, ragged: true },
    queen: { robe: 'rgba(150, 82, 40, 0.8)', trim: 'rgba(38, 54, 100, 0.9)', skin: 'rgba(214, 170, 120, 0.75)', hat: 'rgba(26, 21, 18, 0.95)', head: 'bun', beard: false, female: true },
    prince: { robe: 'rgba(170, 118, 46, 0.82)', trim: 'rgba(92, 54, 30, 0.9)', skin: 'rgba(150, 96, 54, 0.9)', hat: 'rgba(26, 21, 18, 0.9)', head: 'cap', beard: false },
    sailor: { robe: 'rgba(92, 64, 42, 0.84)', trim: 'rgba(60, 44, 30, 0.9)', skin: 'rgba(120, 72, 38, 0.9)', hat: 'rgba(40, 30, 24, 0.92)', head: 'band', beard: true },
    enchantress: { robe: 'rgba(60, 42, 96, 0.8)', trim: 'rgba(210, 150, 60, 0.9)', skin: 'rgba(214, 170, 120, 0.75)', hat: 'rgba(210, 150, 60, 0.9)', head: 'phoenix', beard: false, female: true },
    giant: { robe: 'rgba(56, 40, 30, 0.9)', trim: 'rgba(150, 90, 40, 0.9)', skin: 'rgba(160, 96, 44, 0.9)', hat: 'rgba(30, 22, 18, 0.95)', head: 'demon', beard: true, cyclops: true },
  };

  let layerCanvas = null;
  function layer() {
    if (!layerCanvas) layerCanvas = createSurface(L.WIDTH, L.HEIGHT);
    return layerCanvas;
  }

  function withPuppetLayer(ctx, paint) {
    const canvas = layer();
    const g = canvas.getContext('2d');
    g.setTransform(1, 0, 0, 1, 0, 0);
    g.globalCompositeOperation = 'source-over';
    g.globalAlpha = 1;
    g.clearRect(0, 0, canvas.width, canvas.height);
    g.setTransform(ctx.getTransform());
    paint(g);
    ctx.save();
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.drawImage(canvas, 0, 0);
    ctx.restore();
  }

  function limb(x, y, angle, length, bend, length2) {
    const jx = x + Math.sin(angle) * length;
    const jy = y + Math.cos(angle) * length;
    return { ax: x, ay: y, jx, jy, ex: jx + Math.sin(angle + bend) * length2, ey: jy + Math.cos(angle + bend) * length2 };
  }

  function solveReach(sx, sy, target, length1, length2, bendSign) {
    const dx = target[0] - sx;
    const dy = target[1] - sy;
    const reach = clamp(Math.hypot(dx, dy), 1e-3, length1 + length2 - 1e-3);
    const base = Math.atan2(dx, dy);
    const inner = Math.acos(clamp((length1 * length1 + reach * reach - length2 * length2) / (2 * length1 * reach), -1, 1));
    const upper = base + bendSign * inner;
    const jx = sx + Math.sin(upper) * length1;
    const jy = sy + Math.cos(upper) * length1;
    return [upper, Math.atan2(target[0] - jx, target[1] - jy) - upper];
  }

  function leatherSegment(g, ax, ay, bx, by, radiusA, radiusB, fill, outline) {
    const angle = Math.atan2(by - ay, bx - ax);
    const nx = -Math.sin(angle);
    const ny = Math.cos(angle);
    g.beginPath();
    g.moveTo(ax + nx * radiusA, ay + ny * radiusA);
    g.lineTo(bx + nx * radiusB, by + ny * radiusB);
    g.arc(bx, by, radiusB, angle + Math.PI / 2, angle - Math.PI / 2, true);
    g.lineTo(ax - nx * radiusA, ay - ny * radiusA);
    g.arc(ax, ay, radiusA, angle - Math.PI / 2, angle + Math.PI / 2, true);
    g.closePath();
    g.fillStyle = fill;
    g.fill();
    g.strokeStyle = outline;
    g.lineWidth = 2.4;
    g.stroke();
  }

  function rivet(g, x, y, radius = 3.4) {
    g.fillStyle = 'rgba(20, 16, 14, 0.95)';
    fillCircle(g, x, y, radius);
    g.fillStyle = 'rgba(240, 220, 170, 0.8)';
    fillCircle(g, x - radius * 0.3, y - radius * 0.3, radius * 0.35);
  }

  /** Punches a lattice of tiny flower-shaped holes through whatever is clipped. */
  function punchLattice(g, left, top, right, bottom, spacing, seed) {
    const random = seededRandom(seed);
    g.save();
    g.globalCompositeOperation = 'destination-out';
    for (let y = top; y < bottom; y += spacing) {
      for (let x = left + ((Math.round((y - top) / spacing) % 2) * spacing) / 2; x < right; x += spacing) {
        const size = spacing * (0.16 + random() * 0.05);
        g.fillStyle = 'rgba(0, 0, 0, 0.92)';
        for (let petal = 0; petal < 4; petal++) {
          const angle = (petal / 4) * Math.PI * 2 + Math.PI / 4;
          fillEllipse(g, x + Math.cos(angle) * size, y + Math.sin(angle) * size, size * 0.8, size * 0.45, angle);
        }
      }
    }
    g.restore();
  }

  function paintOpenFace(g, costume, kind) {
    const outline = 'rgba(20, 16, 14, 0.95)';
    const profile = [[-26, -28], [-8, -36], [14, -30], [22, -14], [26, -6], [34, 4], [26, 8], [28, 14], [24, 18], [26, 24], [16, 32], [0, 34], [-20, 26], [-30, 6]];
    g.fillStyle = costume.skin;
    polygonPath(g, catmullRom(profile, 4, true));
    g.fill();
    g.strokeStyle = outline;
    g.lineWidth = 2.4;
    g.stroke();
    g.save();
    g.globalCompositeOperation = 'destination-out';
    g.translate(4, 1);
    g.scale(0.7, 0.72);
    polygonPath(g, catmullRom(profile, 4, true));
    g.fillStyle = 'rgba(0, 0, 0, 0.94)';
    g.fill();
    g.restore();
    g.strokeStyle = costume.skin;
    g.lineCap = 'round';
    g.lineWidth = 3;
    g.beginPath();
    g.moveTo(2, -16);
    g.quadraticCurveTo(12, -22, 22, -16);
    g.stroke();
    g.lineWidth = 2.6;
    if (kind.eyesClosed) {
      g.beginPath();
      g.arc(14, -6, 7, 0.2, Math.PI - 0.2);
      g.stroke();
    } else if (!costume.cyclops) {
      g.beginPath();
      g.moveTo(4, -6);
      g.quadraticCurveTo(14, -12, 24, -5);
      g.quadraticCurveTo(14, -1, 4, -6);
      g.stroke();
      g.fillStyle = costume.skin;
      fillCircle(g, 16, -6, 2.4);
    }
    g.beginPath();
    g.moveTo(22, 20);
    g.lineTo(kind.mouthOpen ? 16 : 18, kind.mouthOpen ? 26 : 21);
    g.stroke();
    if (costume.cyclops) {
      g.fillStyle = costume.skin;
      fillEllipse(g, 8, -18, 16, 11);
      g.save();
      g.globalCompositeOperation = 'destination-out';
      fillEllipse(g, 8, -18, 11, 7);
      g.restore();
      g.fillStyle = 'rgba(240, 200, 90, 0.95)';
      fillCircle(g, 11, -18, 4.5);
    }
  }

  function paintHeaddress(g, costume, t, sway) {
    const outline = 'rgba(20, 16, 14, 0.95)';
    g.strokeStyle = outline;
    g.lineWidth = 2.4;
    switch (costume.head) {
      case 'plumes': {
        g.fillStyle = costume.hat;
        g.beginPath();
        g.moveTo(-32, -18);
        g.quadraticCurveTo(-30, -58, 4, -60);
        g.quadraticCurveTo(28, -56, 26, -24);
        g.lineTo(-32, -18);
        g.fill();
        g.stroke();
        g.fillStyle = costume.trim;
        g.fillRect(-34, -26, 62, 9);
        g.strokeRect(-34, -26, 62, 9);
        fillCircle(g, 2, -62, 7);
        [0, 1].forEach((k) => {
          const plume = [];
          for (let s = 0; s <= 18; s++) {
            const u = s / 18;
            const whip = Math.sin(t * 1.6 + u * 3 + k) * 18 * u * u + sway * u * u * 60;
            plume.push([4 - u * 170 - k * 22 + whip * 0.3, -64 - Math.sin(u * Math.PI * 0.8) * (120 + k * 22) + u * 70 + whip * 0.6]);
          }
          brushStroke(g, plume, 10 - k * 2, { color: 'rgba(24, 20, 18, 0.95)', taperStart: 0.02, taperEnd: 0.75, dry: 0, wobble: 0.05, seed: 40 + k });
          g.fillStyle = 'rgba(210, 160, 70, 0.9)';
          for (let s = 3; s < plume.length - 3; s += 2) fillEllipse(g, plume[s][0], plume[s][1], 3.2, 1.8, 0.6);
        });
        break;
      }
      case 'bun': {
        g.fillStyle = costume.hat;
        fillEllipse(g, -18, -14, 26, 30);
        fillCircle(g, -24, -44, 20);
        g.strokeStyle = 'rgba(210, 160, 70, 0.95)';
        g.lineWidth = 3;
        strokeLine(g, -50, -54, 6, -34);
        strokeLine(g, -46, -30, 4, -58);
        g.fillStyle = 'rgba(210, 160, 70, 0.95)';
        fillCircle(g, 6, -34, 4.5);
        fillCircle(g, 4, -58, 4);
        const tassel = Math.sin(t * 2.2) * 4;
        for (let i = 0; i < 3; i++) strokeLine(g, 6 + i * 3, -34, 8 + i * 4 + tassel, -6 + i * 4);
        break;
      }
      case 'phoenix': {
        g.fillStyle = costume.hat;
        polygonPath(g, [[-30, -20], [-20, -54], [-2, -40], [10, -70], [22, -38], [34, -48], [28, -18]]);
        g.fill();
        g.stroke();
        g.fillStyle = 'rgba(26, 21, 18, 0.95)';
        fillEllipse(g, -18, -10, 22, 28);
        for (let i = 0; i < 5; i++) {
          const beads = Math.sin(t * 2 + i) * 3;
          g.strokeStyle = 'rgba(210, 150, 60, 0.9)';
          g.lineWidth = 2;
          strokeLine(g, -26 + i * 12, -20, -28 + i * 12 + beads, 10 + (i % 2) * 10);
          g.fillStyle = 'rgba(210, 150, 60, 0.95)';
          fillCircle(g, -28 + i * 12 + beads, 12 + (i % 2) * 10, 3);
        }
        break;
      }
      case 'cap': {
        g.fillStyle = costume.hat;
        g.beginPath();
        g.moveTo(-30, -14);
        g.quadraticCurveTo(-24, -52, 8, -48);
        g.quadraticCurveTo(26, -40, 24, -22);
        g.closePath();
        g.fill();
        g.stroke();
        brushStroke(g, [[-26, -30], [-58, -20 + sway * 10], [-80, -2 + sway * 16]], 9, { color: 'rgba(24, 20, 18, 0.9)', dry: 0, taperEnd: 0.8, seed: 11 });
        break;
      }
      case 'hood': {
        g.fillStyle = costume.hat;
        g.beginPath();
        g.moveTo(-36, 30);
        g.quadraticCurveTo(-50, -40, 0, -48);
        g.quadraticCurveTo(30, -46, 30, -18);
        g.lineTo(18, -22);
        g.quadraticCurveTo(-10, -30, -18, 34);
        g.closePath();
        g.fill();
        g.stroke();
        break;
      }
      case 'band': {
        g.fillStyle = costume.hat;
        fillEllipse(g, -12, -16, 24, 22);
        g.fillStyle = costume.trim;
        g.fillRect(-32, -24, 56, 7);
        break;
      }
      case 'demon': {
        g.fillStyle = costume.hat;
        fillEllipse(g, -14, -12, 28, 32);
        brushStroke(g, [[-6, -36], [-2, -70], [18, -96]], 12, { color: 'rgba(24, 20, 18, 0.95)', dry: 0, taperEnd: 0.9, seed: 5 });
        brushStroke(g, [[-26, -32], [-40, -64], [-30, -94]], 12, { color: 'rgba(24, 20, 18, 0.95)', dry: 0, taperEnd: 0.9, seed: 6 });
        break;
      }
      default:
        break;
    }
  }

  function paintBeard(g, costume, t) {
    if (!costume.beard) return;
    const flutter = Math.sin(t * 2.4) * 4;
    g.fillStyle = 'rgba(22, 18, 16, 0.95)';
    g.beginPath();
    g.moveTo(24, 20);
    g.quadraticCurveTo(34, 60, 22 + flutter, 96);
    g.lineTo(10 + flutter * 0.6, 92);
    g.quadraticCurveTo(6, 50, 4, 26);
    g.closePath();
    g.fill();
    g.save();
    g.globalCompositeOperation = 'destination-out';
    g.strokeStyle = 'rgba(0, 0, 0, 0.9)';
    g.lineWidth = 1.6;
    for (let i = 0; i < 3; i++) strokeLine(g, 12 + i * 5, 34, 14 + i * 5 + flutter * 0.5, 84);
    g.restore();
  }

  /**
   * Paints a jointed shadow puppet with feet at (x, y).
   * pose: lean, head, reachNear/reachFar ([x,y] local, or fn(anchors)), footNear/footFar,
   * walkPhase (drives a walk cycle), hip ([x,y] overrides the hip for sitting).
   * Returns world-space anchors.
   */
  function drawPuppet(ctx, spec) {
    const costume = typeof spec.costume === 'string' ? COSTUME[spec.costume] : spec.costume;
    const kind = spec.kind ?? {};
    const scale = spec.scale ?? 1;
    const facing = spec.facing ?? 1;
    const t = spec.t ?? 0;
    const pose = spec.pose ?? {};
    const outline = 'rgba(20, 16, 14, 0.95)';
    const handWobble = Math.sin(t * 2.3 + (spec.seed ?? 0)) * 0.04;

    let legNearAngles = pose.legNear ?? [0.05, -0.05];
    let legFarAngles = pose.legFar ?? [-0.06, -0.05];
    if (pose.walkPhase !== undefined) {
      const swing = Math.sin(pose.walkPhase);
      const lift = Math.cos(pose.walkPhase);
      legNearAngles = [0.4 * swing, -Math.max(0, -lift) * 0.8 - 0.05];
      legFarAngles = [-0.4 * swing, -Math.max(0, lift) * 0.8 - 0.05];
    }
    let legNear = limb(0, 0, legNearAngles[0], BODY.thigh, legNearAngles[1], BODY.shin);
    let legFar = limb(0, 0, legFarAngles[0], BODY.thigh, legFarAngles[1], BODY.shin);
    const hip = pose.hip ?? [0, -(Math.max(legNear.ey, legFar.ey) + 12)];
    const legAt = (angles, target, sign) => {
      const solved = target ? solveReach(hip[0], hip[1], target, BODY.thigh, BODY.shin, sign) : angles;
      return limb(hip[0], hip[1], solved[0], BODY.thigh, solved[1], BODY.shin);
    };
    legNear = legAt(legNearAngles, pose.footNear, pose.kneeNear ?? 1);
    legFar = legAt(legFarAngles, pose.footFar, pose.kneeFar ?? 1);

    const lean = (pose.lean ?? 0) + (pose.walkPhase !== undefined ? 0.05 : 0);
    const along = (length) => [hip[0] + Math.sin(lean) * length, hip[1] - Math.cos(lean) * length];
    const shoulder = along(BODY.torso - 16);
    const neck = along(BODY.torso);
    const headAngle = lean + (pose.head ?? 0);
    const headCenter = [neck[0] + Math.sin(headAngle) * BODY.headOffset, neck[1] - Math.cos(headAngle) * BODY.headOffset];
    const anchors = { hip, shoulder, neck, headCenter };
    const armAt = (origin, angles, target, sign) => {
      const resolved = typeof target === 'function' ? target(anchors) : target;
      const solved = resolved ? solveReach(origin[0], origin[1], resolved, BODY.upperArm, BODY.forearm, sign) : angles;
      return limb(origin[0], origin[1], solved[0] + handWobble, BODY.upperArm, solved[1], BODY.forearm);
    };
    const walkArm = pose.walkPhase !== undefined ? Math.sin(pose.walkPhase) * 0.35 : 0;
    const armNear = armAt(shoulder, pose.armNear ?? [0.25 - walkArm, -0.5], pose.reachNear, pose.elbowNear ?? -1);
    const armFar = armAt([shoulder[0] - 6, shoulder[1] + 3], pose.armFar ?? [-0.1 + walkArm, -0.4], pose.reachFar, pose.elbowFar ?? -1);
    const hemSwing = (pose.walkPhase !== undefined ? Math.sin(pose.walkPhase * 2) * 6 : 0) + Math.sin(t * 1.4 + (spec.seed ?? 0)) * 3;
    const seated = Boolean(pose.hip);

    const toWorld = ([lx, ly]) => [spec.x + facing * scale * lx, spec.y + scale * ly];
    const chest = toWorld([(shoulder[0] + hip[0]) / 2, (shoulder[1] + hip[1]) / 2]);
    if (spec.glow !== 0) drawGlow(ctx, chest[0], chest[1], 380 * scale, RGB.candle, 0.3 * (spec.glow ?? 1));

    withPuppetLayer(ctx, (g) => {
      g.save();
      g.translate(spec.x, spec.y);
      g.scale(facing * scale, scale);
      g.lineJoin = 'round';

      const paintArm = (arm, near) => {
        leatherSegment(g, arm.ax, arm.ay, arm.jx, arm.jy, 11, 9, costume.robe, outline);
        leatherSegment(g, arm.jx, arm.jy, arm.ex, arm.ey, 9, 7, costume.skin, outline);
        const sleeveAngle = Math.atan2(arm.ey - arm.jy, arm.ex - arm.jx);
        g.fillStyle = costume.trim;
        g.beginPath();
        g.moveTo(arm.jx, arm.jy);
        g.quadraticCurveTo(arm.jx + Math.cos(sleeveAngle + 1.6) * 40, arm.jy + 60, arm.jx + Math.cos(sleeveAngle) * 20 - 10, arm.jy + 86 + hemSwing);
        g.lineTo(arm.jx + Math.cos(sleeveAngle) * 30 + 6, arm.jy + 40);
        g.closePath();
        g.globalAlpha = near ? 0.85 : 0.7;
        g.fill();
        g.globalAlpha = 1;
        g.strokeStyle = outline;
        g.lineWidth = 1.8;
        g.stroke();
        g.fillStyle = costume.skin;
        fillEllipse(g, arm.ex, arm.ey, BODY.hand + 2, BODY.hand, sleeveAngle);
        g.strokeStyle = outline;
        g.lineWidth = 2;
        g.beginPath();
        g.ellipse(arm.ex, arm.ey, BODY.hand + 2, BODY.hand, sleeveAngle, 0, Math.PI * 2);
        g.stroke();
        rivet(g, arm.jx, arm.jy);
        rivet(g, arm.ax, arm.ay, 3);
      };
      const paintLeg = (leg) => {
        leatherSegment(g, leg.ax, leg.ay, leg.jx, leg.jy, 15, 11, costume.trim, outline);
        leatherSegment(g, leg.jx, leg.jy, leg.ex, leg.ey, 11, 8, 'rgba(26, 21, 18, 0.92)', outline);
        g.fillStyle = 'rgba(26, 21, 18, 0.95)';
        g.beginPath();
        g.moveTo(leg.ex - 10, leg.ey - 8);
        g.lineTo(leg.ex + 30, leg.ey + 2);
        g.quadraticCurveTo(leg.ex + 40, leg.ey - 6, leg.ex + 36, leg.ey + 10);
        g.lineTo(leg.ex - 12, leg.ey + 10);
        g.closePath();
        g.fill();
        rivet(g, leg.jx, leg.jy);
      };

      paintArm(armFar, false);
      paintLeg(legFar);

      g.save();
      g.translate(hip[0], hip[1]);
      g.rotate(lean);
      const hemY = seated ? 70 : -hip[1] - 26;
      const hemHalf = costume.female ? 70 : 56;
      const robe = [
        [-30, -(BODY.torso - 10)], [32, -(BODY.torso - 10)], [36, -60], [24, -8],
        [hemHalf + hemSwing, hemY], [hemHalf * 0.3 + hemSwing, hemY + 8], [-hemHalf * 0.4 + hemSwing, hemY + 6], [-hemHalf + hemSwing * 0.6, hemY], [-26, -8], [-40, -70],
      ];
      g.fillStyle = costume.robe;
      polygonPath(g, robe);
      g.fill();
      g.strokeStyle = outline;
      g.lineWidth = 2.6;
      g.stroke();
      g.save();
      polygonPath(g, robe);
      g.clip();
      punchLattice(g, -60, -BODY.torso + 40, 80, hemY - 10, 22, (spec.seed ?? 1) * 17);
      g.fillStyle = costume.trim;
      g.fillRect(-60, -22, 140, 14);
      g.fillRect(-80, hemY - 18, 180, 12);
      if (costume.ragged) {
        g.globalCompositeOperation = 'destination-out';
        const random = seededRandom(9);
        for (let i = 0; i < 6; i++) fillPolygon(g, [[-60 + i * 24, hemY + 10], [-50 + i * 24, hemY - 20 - random() * 20], [-40 + i * 24, hemY + 10]]);
        g.globalCompositeOperation = 'source-over';
      }
      g.restore();
      g.fillStyle = costume.trim;
      g.beginPath();
      g.moveTo(-26, -(BODY.torso - 12));
      g.quadraticCurveTo(4, -BODY.torso + 30, 30, -(BODY.torso - 12));
      g.lineTo(26, -(BODY.torso - 26));
      g.quadraticCurveTo(4, -BODY.torso + 44, -22, -(BODY.torso - 26));
      g.closePath();
      g.fill();
      g.stroke();
      g.restore();

      if (!seated) paintLeg(legNear);

      g.save();
      g.translate(headCenter[0], headCenter[1]);
      g.rotate(headAngle);
      g.scale(1.08, 1.08);
      paintHeaddress(g, costume, t, Math.sin(t * 0.9) * 0.3 + (pose.walkPhase !== undefined ? Math.sin(pose.walkPhase) * 0.2 : 0));
      paintOpenFace(g, costume, kind);
      paintBeard(g, costume, t);
      g.restore();
      rivet(g, neck[0], neck[1] + 6, 4);

      paintArm(armNear, true);
      if (spec.decorate) spec.decorate(g, { ...anchors, armNear, armFar, legNear, legFar });
      g.restore();
    });

    const handNear = toWorld([armNear.ex, armNear.ey]);
    const handFar = toWorld([armFar.ex, armFar.ey]);
    if (spec.rods !== false) {
      const rodBottom = spec.rodBottom ?? L.HEIGHT + 400;
      ctx.save();
      ctx.strokeStyle = 'rgba(30, 24, 20, 0.55)';
      ctx.lineWidth = 2.2;
      ctx.lineCap = 'round';
      const neckWorld = toWorld([neck[0], neck[1] + 8]);
      strokeLine(ctx, neckWorld[0], neckWorld[1], neckWorld[0] - facing * 40 * scale, rodBottom);
      ctx.lineWidth = 1.5;
      strokeLine(ctx, handNear[0], handNear[1], handNear[0] + facing * 60 * scale, rodBottom);
      strokeLine(ctx, handFar[0], handFar[1], handFar[0] - facing * 20 * scale, rodBottom);
      ctx.restore();
    }
    return { handNear, handFar, head: toWorld(headCenter), hip: toWorld(hip), shoulder: toWorld(shoulder), neck: toWorld(neck), chest, scale, facing };
  }

  /** A siren: a bird-bodied puppet with a woman's open face, wings on hinges. */
  function drawSiren(ctx, x, y, scale, t, facing, seed = 1) {
    const flap = Math.sin(t * 5 + seed * 1.7);
    const outline = 'rgba(20, 16, 14, 0.95)';
    drawGlow(ctx, x, y, 260 * scale, RGB.candle, 0.25);
    withPuppetLayer(ctx, (g) => {
      g.save();
      g.translate(x, y);
      g.scale(facing * scale, scale);
      g.lineJoin = 'round';
      const wing = (rotation, alpha) => {
        g.save();
        g.translate(-10, -20);
        g.rotate(rotation);
        g.globalAlpha = alpha;
        const feathers = [];
        for (let i = 0; i < 6; i++) feathers.push([[0, 0], [-40 - i * 16, -150 + i * 22], [-8 - i * 12, -150 + i * 26]]);
        g.fillStyle = 'rgba(150, 82, 40, 0.82)';
        g.beginPath();
        g.moveTo(0, 0);
        g.quadraticCurveTo(-90, -80, -120, -170);
        g.quadraticCurveTo(-40, -150, 36, -30);
        g.closePath();
        g.fill();
        g.strokeStyle = outline;
        g.lineWidth = 2.4;
        g.stroke();
        g.save();
        g.clip();
        g.globalCompositeOperation = 'destination-out';
        g.strokeStyle = 'rgba(0, 0, 0, 0.9)';
        g.lineWidth = 3;
        feathers.forEach((f) => L.strokePolyline(g, f));
        g.restore();
        g.restore();
      };
      wing(-0.3 - flap * 0.6, 0.8);
      g.fillStyle = 'rgba(38, 54, 100, 0.84)';
      fillEllipse(g, 0, 0, 70, 34, -0.1);
      g.strokeStyle = outline;
      g.lineWidth = 2.4;
      g.beginPath();
      g.ellipse(0, 0, 70, 34, -0.1, 0, Math.PI * 2);
      g.stroke();
      g.save();
      g.beginPath();
      g.ellipse(0, 0, 70, 34, -0.1, 0, Math.PI * 2);
      g.clip();
      punchLattice(g, -70, -34, 70, 34, 16, seed);
      g.restore();
      for (let i = 0; i < 4; i++) {
        brushStroke(g, [[-60, -4 + i * 6], [-120 - i * 10, -30 + i * 20 + flap * 6], [-170 - i * 6, -50 + i * 28 + flap * 10]], 10, { color: 'rgba(26, 21, 18, 0.9)', dry: 0, taperEnd: 0.8, seed: seed + i });
      }
      g.strokeStyle = outline;
      g.lineWidth = 4;
      strokeLine(g, -6, 30, -2, 60);
      strokeLine(g, 16, 30, 20, 60);
      g.fillStyle = 'rgba(214, 170, 120, 0.75)';
      polygonPath(g, [[40, -20], [58, -48], [70, -38], [56, -8]]);
      g.fill();
      g.stroke();
      g.save();
      g.translate(64, -76);
      paintHeaddress(g, COSTUME.queen, t, 0);
      paintOpenFace(g, COSTUME.queen, { mouthOpen: true });
      g.restore();
      wing(0.1 + flap * 0.5, 0.9);
      g.restore();
    });
    return [x + facing * scale * 92, y - scale * 66];
  }

  /** A folded paper crane carrying a message; `flap` animates the wings. */
  function paperCrane(ctx, x, y, scale, angle, flap, options = {}) {
    const { glow = 0 } = options;
    if (glow > 0) drawGlow(ctx, x, y, 120 * scale, RGB.jade, glow);
    ctx.save();
    ctx.translate(x, y);
    ctx.rotate(angle);
    ctx.scale(scale, scale);
    ctx.lineJoin = 'round';
    ctx.strokeStyle = 'rgba(26, 21, 18, 0.9)';
    ctx.lineWidth = 2;
    const lift = Math.sin(flap) * 40;
    ctx.fillStyle = '#EFE6D0';
    polygonPath(ctx, [[-10, 0], [-60, -30 - lift], [10, -6]]);
    ctx.fill();
    ctx.stroke();
    ctx.fillStyle = '#F7F1E2';
    polygonPath(ctx, [[-44, 4], [0, -10], [40, 0], [0, 12]]);
    ctx.fill();
    ctx.stroke();
    polygonPath(ctx, [[36, 0], [62, -34], [70, -30], [44, 4]]);
    ctx.fill();
    ctx.stroke();
    polygonPath(ctx, [[-40, 4], [-74, -22], [-66, 6]]);
    ctx.fill();
    ctx.stroke();
    ctx.fillStyle = '#FBF6EA';
    polygonPath(ctx, [[-6, 0], [-40, -40 + lift * 0.6], [14, -4]]);
    ctx.fill();
    ctx.stroke();
    ctx.restore();
  }

  Object.assign(L, { drawPuppet, drawSiren, paperCrane, COSTUME, withPuppetLayer });
  void lerp;
})();
