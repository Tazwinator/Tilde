// Hand-rolled canvas confetti — fire and forget.
// confetti(x, y, count) bursts colored rects/circles with gravity + rotation.

interface Particle {
  x: number;
  y: number;
  vx: number;
  vy: number;
  size: number;
  color: string;
  rot: number;
  vr: number;
  shape: "rect" | "circle";
  life: number;
  ttl: number;
}

const COLORS = ["#a78bfa", "#fbbf24", "#34d399", "#fb7185", "#38bdf8", "#f472b6", "#facc15"];

let canvas: HTMLCanvasElement | null = null;
let particles: Particle[] = [];
let raf = 0;

function reducedMotion(): boolean {
  return typeof matchMedia !== "undefined" && matchMedia("(prefers-reduced-motion: reduce)").matches;
}

function ensureCanvas(): HTMLCanvasElement {
  if (!canvas) {
    canvas = document.createElement("canvas");
    canvas.style.cssText =
      "position:fixed;inset:0;width:100vw;height:100vh;pointer-events:none;z-index:9999";
    canvas.setAttribute("aria-hidden", "true");
    document.body.appendChild(canvas);
  }
  const dpr = Math.min(window.devicePixelRatio || 1, 2);
  if (canvas.width !== window.innerWidth * dpr) {
    canvas.width = window.innerWidth * dpr;
    canvas.height = window.innerHeight * dpr;
  }
  const c = canvas.getContext("2d");
  c?.setTransform(dpr, 0, 0, dpr, 0, 0);
  return canvas;
}

function tick(): void {
  const c = canvas?.getContext("2d");
  if (!canvas || !c) return;
  c.clearRect(0, 0, window.innerWidth, window.innerHeight);
  const now = performance.now();
  particles = particles.filter((p) => {
    if (now - p.life > p.ttl) return false;
    p.vy += 0.18; // gravity
    p.vx *= 0.99;
    p.x += p.vx;
    p.y += p.vy;
    p.rot += p.vr;
    const alpha = 1 - (now - p.life) / p.ttl;
    c.save();
    c.globalAlpha = Math.max(0, alpha);
    c.translate(p.x, p.y);
    c.rotate(p.rot);
    c.fillStyle = p.color;
    if (p.shape === "rect") {
      c.fillRect(-p.size / 2, -p.size / 4, p.size, p.size / 2);
    } else {
      c.beginPath();
      c.arc(0, 0, p.size / 2, 0, Math.PI * 2);
      c.fill();
    }
    c.restore();
    return p.y < window.innerHeight + 40;
  });
  if (particles.length > 0) {
    raf = requestAnimationFrame(tick);
  } else {
    cancelAnimationFrame(raf);
    raf = 0;
    canvas?.remove();
    canvas = null;
  }
}

export function confetti(x?: number, y?: number, count = 90): void {
  if (reducedMotion() || count <= 0) return;
  ensureCanvas();
  const cx = x ?? window.innerWidth / 2;
  const cy = y ?? window.innerHeight * 0.35;
  const now = performance.now();
  for (let i = 0; i < count; i++) {
    const angle = Math.random() * Math.PI * 2;
    const speed = 4 + Math.random() * 9;
    particles.push({
      x: cx,
      y: cy,
      vx: Math.cos(angle) * speed,
      vy: Math.sin(angle) * speed - 4,
      size: 6 + Math.random() * 8,
      color: COLORS[(Math.random() * COLORS.length) | 0],
      rot: Math.random() * Math.PI * 2,
      vr: (Math.random() - 0.5) * 0.35,
      shape: Math.random() < 0.65 ? "rect" : "circle",
      life: now,
      ttl: 1400 + Math.random() * 1200,
    });
  }
  if (!raf) raf = requestAnimationFrame(tick);
}

export function confettiCenter(count = 90): void {
  confetti(window.innerWidth / 2, window.innerHeight * 0.3, count);
}
