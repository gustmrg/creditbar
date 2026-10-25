// The aquarium at the top of the DeepSeek pricing section, ported from DeepSeek Status
// (AquariumView.swift and WhaleScene.swift): at peak the whale swims back and forth trailing
// bubbles; off-peak it sleeps, tilted and breathing, under stars and a moon, sending up "z"s.
// It draws at 20 fps and only while it is on screen.

// DeepSeek whale logo, 24×24 (Simple Icons, CC0).
const WHALE_PATH = "M23.748 4.651c-.254-.124-.364.113-.512.233-.051.04-.094.09-.137.137-.372.397-.806.657-1.373.626-.829-.046-1.537.214-2.163.848-.133-.782-.575-1.248-1.247-1.548-.352-.155-.708-.311-.955-.65-.172-.24-.219-.509-.305-.774-.055-.16-.11-.323-.293-.35-.2-.031-.278.136-.356.276-.313.572-.434 1.202-.422 1.84.027 1.436.633 2.58 1.838 3.393.137.094.172.187.129.323-.082.28-.18.553-.266.833-.055.179-.137.218-.328.14a5.5 5.5 0 0 1-1.737-1.179c-.857-.828-1.631-1.743-2.597-2.46a12 12 0 0 0-.689-.47c-.985-.957.13-1.743.387-1.836.27-.098.094-.433-.778-.428-.872.003-1.67.295-2.687.685a3 3 0 0 1-.465.136 9.6 9.6 0 0 0-2.883-.101c-1.885.21-3.39 1.1-4.497 2.622C.082 8.776-.231 10.854.152 13.02c.403 2.284 1.568 4.175 3.36 5.653 1.857 1.533 3.997 2.284 6.438 2.14 1.482-.085 3.132-.284 4.994-1.86.47.234.962.328 1.78.398.629.058 1.235-.031 1.705-.129.735-.155.684-.836.418-.961-2.155-1.004-1.682-.595-2.112-.926 1.095-1.295 2.768-3.598 3.284-6.733.05-.346.115-.834.108-1.114-.004-.171.035-.238.23-.257a4.2 4.2 0 0 0 1.545-.475c1.397-.763 1.96-2.016 2.093-3.517.02-.23-.004-.467-.247-.588M11.58 18.168c-2.088-1.642-3.101-2.183-3.52-2.16-.39.024-.32.472-.234.763.09.288.207.487.371.74.114.167.192.416-.113.603-.673.416-1.842-.14-1.897-.168-1.361-.801-2.5-1.86-3.301-3.306-.775-1.393-1.225-2.888-1.299-4.482-.02-.385.094-.522.477-.592a4.7 4.7 0 0 1 1.53-.038c2.131.311 3.946 1.264 5.467 2.774.868.86 1.525 1.887 2.202 2.89.72 1.066 1.494 2.082 2.48 2.915.348.291.626.513.892.677-.802.09-2.14.109-3.055-.615zm1.001-6.44a.306.306 0 0 1 .415-.287.3.3 0 0 1 .113.074.3.3 0 0 1 .086.214c0 .17-.136.307-.308.307a.303.303 0 0 1-.306-.307m3.11 1.596c-.2.081-.4.151-.591.16a1.25 1.25 0 0 1-.798-.254c-.274-.23-.47-.358-.551-.758a1.7 1.7 0 0 1 .015-.588c.07-.327-.007-.537-.238-.727-.188-.156-.426-.199-.689-.199a.6.6 0 0 1-.254-.078.253.253 0 0 1-.114-.358 1 1 0 0 1 .192-.21c.356-.202.767-.136 1.146.016.352.144.618.408 1.001.782.392.451.462.576.685.915.176.264.336.536.446.848.066.194-.02.353-.25.45";

const Aquarium = (() => {
  const FRAME_MS = 1000 / 20;
  const HEIGHT = 120;
  const WHALE_WIDTH = 92;

  const rgb = (r, g, b) => `rgb(${r}, ${g}, ${b})`;
  const BRAND = rgb(77, 107, 254);
  const BRAND_DEEP = rgb(46, 76, 214);
  const BRAND_BRIGHT = rgb(122, 158, 255);
  const SLEEP = rgb(130, 145, 185);
  const SLEEP_LIGHT = rgb(176, 189, 219);
  const SLEEP_DEEP = rgb(96, 108, 145);

  const SCENES = {
    peak: {
      tank: [rgb(18, 30, 74), rgb(8, 14, 38)],
      light: 0.16,
      body: [rgb(158, 191, 255), BRAND, BRAND_DEEP],
      bubble: "rgba(255, 255, 255, 0.65)",
    },
    offPeak: {
      tank: [rgb(20, 26, 48), rgb(9, 12, 24)],
      light: 0.07,
      body: [SLEEP_LIGHT, SLEEP, SLEEP_DEEP],
      sleepMark: SLEEP_LIGHT,
    },
  };

  const whale = new Path2D(WHALE_PATH);
  // The logo doesn't fill its 24×24 box; measure it so the whale is placed by its real bounds.
  const bounds = (() => {
    const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    svg.setAttribute("style", "position: absolute; width: 0; height: 0; visibility: hidden");
    const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
    path.setAttribute("d", WHALE_PATH);
    svg.append(path);
    document.body.append(svg);
    const box = path.getBBox();
    svg.remove();
    return box.width > 0 ? box : { x: 0, y: 0, width: 24, height: 24 };
  })();
  const aspect = bounds.height / bounds.width;

  /** Fixed pseudo-random numbers, so stars and bubbles keep their places between frames. */
  function seeded(index, salt) {
    const value = Math.sin(index * 12.9898 + salt * 78.233) * 43758.5453;
    return value - Math.floor(value);
  }

  let canvas = null;
  let period = "peak";
  let timer = null;
  const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");

  const visible = () => canvas && canvas.isConnected && canvas.offsetParent !== null && !document.hidden;

  function drawWhale(ctx, scene, x, y, width, tiltDegrees, mirrored) {
    const height = width * aspect;
    ctx.save();
    ctx.translate(x + width / 2, y + height / 2);
    ctx.scale(mirrored ? -1 : 1, 1);
    ctx.rotate((tiltDegrees * Math.PI) / 180);
    ctx.translate(-width / 2, -height / 2);
    const gradient = ctx.createLinearGradient(0, 0, width * 0.35, height);
    scene.body.forEach((color, i) => gradient.addColorStop(i / (scene.body.length - 1), color));
    ctx.fillStyle = gradient;
    const scale = width / bounds.width;
    ctx.scale(scale, scale);
    ctx.translate(-bounds.x, -bounds.y);
    ctx.fill(whale);
    ctx.restore();
  }

  function drawBackground(ctx, scene, w, h) {
    const tank = ctx.createLinearGradient(0, 0, 0, h);
    tank.addColorStop(0, scene.tank[0]);
    tank.addColorStop(1, scene.tank[1]);
    ctx.fillStyle = tank;
    ctx.fillRect(0, 0, w, h);
    // Light coming down through the water surface.
    const light = ctx.createRadialGradient(w / 2, -h * 0.1, 4, w / 2, -h * 0.1, 190);
    light.addColorStop(0, `rgba(255, 255, 255, ${scene.light})`);
    light.addColorStop(1, "rgba(255, 255, 255, 0)");
    ctx.fillStyle = light;
    ctx.fillRect(0, 0, w, h);
  }

  function drawRisingBubbles(ctx, w, h, t) {
    ctx.strokeStyle = "rgba(255, 255, 255, 0.7)";
    ctx.lineWidth = 0.8;
    for (let i = 0; i < 9; i++) {
      const speed = 0.045 + seeded(i, 2) * 0.05;
      const radius = 1.2 + seeded(i, 3) * 2.4;
      const progress = (t * speed + seeded(i, 4)) % 1;
      const x = w * (0.05 + seeded(i, 1) * 0.9) + Math.sin(t * 0.8 + i) * 3;
      const y = h + 6 - progress * (h + 16);
      ctx.globalAlpha = Math.sin(progress * Math.PI) * 0.4;
      ctx.beginPath();
      ctx.arc(x, y, radius, 0, Math.PI * 2);
      ctx.stroke();
    }
    ctx.globalAlpha = 1;
  }

  function drawNight(ctx, w, h, t) {
    ctx.fillStyle = "#fff";
    for (let i = 0; i < 26; i++) {
      const x = w * seeded(i, 11);
      const y = h * 0.78 * seeded(i, 12);
      const radius = 0.5 + seeded(i, 13) * 0.9;
      ctx.globalAlpha = 0.35 + 0.45 * (0.5 + 0.5 * Math.sin(t * (0.7 + seeded(i, 14)) + i));
      ctx.beginPath();
      ctx.arc(x, y, radius, 0, Math.PI * 2);
      ctx.fill();
    }
    // A crescent: the moon's disc with a second, offset disc cut out of it.
    const r = 12;
    const cx = w - 30;
    const cy = 28;
    ctx.save();
    ctx.beginPath();
    ctx.rect(0, 0, w, h);
    ctx.arc(cx + 6.5, cy - 3.5, r, 0, Math.PI * 2);
    ctx.clip("evenodd");
    ctx.globalAlpha = 0.85;
    ctx.fillStyle = rgb(230, 237, 255);
    ctx.beginPath();
    ctx.arc(cx, cy, r, 0, Math.PI * 2);
    ctx.fill();
    ctx.restore();
  }

  function drawSwimming(ctx, scene, w, h, t) {
    const margin = 2;
    const width = Math.min(WHALE_WIDTH, w - margin * 2);
    const height = width * aspect;
    // Back and forth, eased at the ends so the turn looks natural.
    const u = (t / 4.2) % 1;
    const triangle = u < 0.5 ? u * 2 : (1 - u) * 2;
    const eased = 0.5 - 0.5 * Math.cos(triangle * Math.PI);
    const facingLeft = u >= 0.5;
    const x = margin + Math.max(0, w - width - margin * 2) * eased;
    const room = Math.max(0, h - height);
    const y = room / 2 + Math.sin(t * 1.55) * room * 0.26;
    drawWhale(ctx, scene, x, y, width, Math.sin(t * 1.55 + 0.75) * 8, facingLeft);

    // Bubbles trailing from the tail.
    const unit = width / 20;
    const tailX = facingLeft ? x + width * 0.92 : x + width * 0.08;
    const direction = facingLeft ? 1 : -1;
    ctx.fillStyle = scene.bubble;
    for (let i = 0; i < 3; i++) {
      const progress = (t * 0.42 + i * 0.34) % 1;
      const radius = (0.75 + i * 0.4) * unit;
      const bx = tailX + direction * (2.5 + progress * 7) * unit;
      const by = y + height * 0.72 - progress * (height + 7 * unit);
      if (by < -4) continue;
      ctx.globalAlpha = (1 - progress) * 0.55;
      ctx.beginPath();
      ctx.arc(bx, by, radius, 0, Math.PI * 2);
      ctx.fill();
    }
    ctx.globalAlpha = 1;
  }

  function drawSleeping(ctx, scene, w, h, t) {
    const breath = 1 + Math.sin(t * 1.05) * 0.022;
    const width = Math.min(WHALE_WIDTH, w - 4) * breath;
    const height = width * aspect;
    const room = Math.max(0, h - height);
    // Slightly left of centre, leaving room for the rising "z"s.
    const x = (w - width) / 2 - w * 0.04;
    const y = room / 2 + Math.sin(t * 0.85) * room * 0.14;
    drawWhale(ctx, scene, x, y, width, -7 + Math.sin(t * 0.6) * 1.6, false);

    const markSize = Math.max(3.5, width * 0.15);
    const startX = x + width * 1.06;
    const startY = y + height * 0.3;
    ctx.strokeStyle = scene.sleepMark;
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    for (let i = 0; i < 3; i++) {
      const progress = (t * 0.3 + i / 3) % 1;
      const cx = startX + progress * width * 0.24;
      const cy = startY - progress * height * 0.6;
      const size = markSize * (1 + progress * 0.3);
      const half = size / 2;
      ctx.globalAlpha = Math.sin(progress * Math.PI) * 0.9;
      ctx.lineWidth = size * 0.2;
      ctx.beginPath();
      ctx.moveTo(cx - half, cy - half);
      ctx.lineTo(cx + half, cy - half);
      ctx.lineTo(cx - half, cy + half);
      ctx.lineTo(cx + half, cy + half);
      ctx.stroke();
    }
    ctx.globalAlpha = 1;
  }

  function draw(time) {
    const ratio = window.devicePixelRatio || 1;
    const w = canvas.clientWidth;
    const h = HEIGHT;
    if (canvas.width !== Math.round(w * ratio) || canvas.height !== Math.round(h * ratio)) {
      canvas.width = Math.round(w * ratio);
      canvas.height = Math.round(h * ratio);
    }
    const ctx = canvas.getContext("2d");
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
    const scene = SCENES[period];
    const t = time / 1000;
    drawBackground(ctx, scene, w, h);
    if (period === "peak") {
      drawRisingBubbles(ctx, w, h, t);
      drawSwimming(ctx, scene, w, h, t);
    } else {
      drawNight(ctx, w, h, t);
      drawSleeping(ctx, scene, w, h, t);
    }
  }

  function frame() {
    timer = null;
    if (!visible()) return;
    draw(performance.now());
    if (!reducedMotion.matches) timer = setTimeout(frame, FRAME_MS);
  }

  return {
    /** Draws into `element` (a canvas). */
    attach(element) {
      canvas = element;
      canvas.style.height = `${HEIGHT}px`;
    },
    setPeriod(value) {
      period = value === "offPeak" ? "offPeak" : "peak";
      this.wake();
    },
    /** Starts drawing if the aquarium is on screen; call after anything that may show it. */
    wake() {
      if (timer === null && visible()) frame();
    },
    /** Stops drawing; a later `wake` resumes. */
    sleep() {
      clearTimeout(timer);
      timer = null;
    },
  };
})();
