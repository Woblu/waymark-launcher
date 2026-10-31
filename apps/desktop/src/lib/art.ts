// Draws instance artwork as real low-resolution pixel art: a dithered dusk sky,
// flat stepped ridges with spruces, and a trail of lit stone markers. Each piece
// comes from the instance's name, so it's unique and stable, needs no image
// files, and uses nothing of Mojang's. It's the same technique as the website.

type Rgb = [number, number, number];

interface Palette {
  sky: string[];
  ridges: [string, string, string, string];
}

const PALETTES: Palette[] = [
  {
    sky: ['#0a0e15', '#0d141e', '#111b28', '#172433', '#223245', '#3a4556', '#6a5a5c', '#a8745a', '#d9925a', '#efb36c'],
    ridges: ['#35444f', '#26333f', '#1b2632', '#131b25'],
  },
  {
    sky: ['#070f14', '#0a1a22', '#0e2730', '#14363f', '#1d4a52', '#2e6066', '#4f7f7c', '#82a596', '#b7c7a6', '#dcd9ac'],
    ridges: ['#2a5356', '#1e4045', '#152f35', '#0e2026'],
  },
  {
    sky: ['#0c0a14', '#14101f', '#1f1730', '#2e2042', '#442a52', '#633659', '#8c465a', '#b65c58', '#d97c58', '#eba262'],
    ridges: ['#4d3350', '#3a2642', '#2a1b33', '#1b1224'],
  },
  {
    sky: ['#090d10', '#0e171a', '#142226', '#1c3033', '#284340', '#3d5a50', '#5f7563', '#8f9375', '#b9ab84', '#d6bf94'],
    ridges: ['#3a4a3a', '#2b3a2e', '#1e2b23', '#131d18'],
  },
  {
    sky: ['#080b16', '#0d1324', '#131c34', '#1b2846', '#27375a', '#3b4c70', '#5c6a86', '#8a8b98', '#b7a8a0', '#d8c2a8'],
    ridges: ['#333f5a', '#262f47', '#1a2236', '#111726'],
  },
  {
    sky: ['#100a0c', '#1a0f12', '#27141a', '#381a22', '#4f2229', '#6f2d2e', '#973e33', '#bf5a3a', '#df7d45', '#f0a657'],
    ridges: ['#523030', '#3f2428', '#2d1a1f', '#1d1115'],
  },
];

const PAGE = '#0d0f12';
const LIT = '#ffc36e';
const CORE = '#fff1d6';

const BAYER = [
  [0, 8, 2, 10],
  [12, 4, 14, 6],
  [3, 11, 1, 9],
  [15, 7, 13, 5],
];

const threshold = (x: number, y: number) => (BAYER[y % 4][x % 4] + 0.5) / 16;

function rgb(hex: string): Rgb {
  return [parseInt(hex.slice(1, 3), 16), parseInt(hex.slice(3, 5), 16), parseInt(hex.slice(5, 7), 16)];
}

const mix = (a: Rgb, b: Rgb, t: number): Rgb => [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];

function hash(text: string): number {
  let value = 2166136261;
  for (let i = 0; i < text.length; i++) value = Math.imul(value ^ text.charCodeAt(i), 16777619);
  return value >>> 0;
}

function generator(seed: number): () => number {
  let state = seed;
  return () => {
    state = (state + 0x6d2b79f5) | 0;
    let t = Math.imul(state ^ (state >>> 15), 1 | state);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function render(seed: string, width: number, height: number): ImageData {
  const image = new ImageData(width, height);
  const data = image.data;
  const base = hash(seed);
  const random = generator(base);
  const palette = PALETTES[base % PALETTES.length];
  const scale = height / 150;

  const put = (x: number, y: number, color: Rgb) => {
    if (x < 0 || y < 0 || x >= width || y >= height) return;
    const at = (y * width + x) * 4;
    data[at] = color[0];
    data[at + 1] = color[1];
    data[at + 2] = color[2];
    data[at + 3] = 255;
  };
  const get = (x: number, y: number): Rgb => {
    const at = (y * width + x) * 4;
    return [data[at], data[at + 1], data[at + 2]];
  };

  // Flat bands with a short dithered edge between each.
  // Short art gets fewer bands, or they'd read as stripes.
  const sky = palette.sky.filter((_, index) => height >= 130 || index % 2 === 1).map(rgb);
  const horizon = 96 * scale;
  for (let y = 0; y < height; y++) {
    const position = Math.min(1, y / horizon) * (sky.length - 1);
    const band = Math.min(sky.length - 2, Math.floor(position));
    const edge = Math.max(0, Math.min(1, (position - band - 0.4) / 0.6));
    for (let x = 0; x < width; x++) put(x, y, sky[edge > threshold(x, y) ? band + 1 : band]);
  }

  const stars = Math.round((width * height) / 400);
  const bright = rgb('#d6dde6');
  const dim = rgb('#5f6f80');
  for (let i = 0; i < stars; i++) {
    put(Math.floor(random() * width), Math.floor(random() * random() * 74 * scale), random() > 0.84 ? bright : dim);
  }

  const layers: [number, number, number, string, string | null, number][] = [
    [92, 12, 3, palette.ridges[0], '#ffffff', 0],
    [104, 11, 4, palette.ridges[1], null, 10],
    [115, 9, 5, palette.ridges[2], null, 26],
    [127, 7, 6, palette.ridges[3], null, 40],
    [140, 5, 8, PAGE, null, 34],
  ];

  const ridges = layers.map(([ground, amplitude, step, fill, rim, trees]) => {
    const color = rgb(fill);
    const rimColor = rim ? mix(color, rgb(rim), 0.12) : mix(color, rgb('#ffffff'), 0.05);
    const blockStep = Math.max(1, Math.round(step * Math.max(scale, 0.5)));
    const waves = [0, 1, 2, 3].map((i) => ({ f: (1.3 + random() * 1.5) * (i + 1), p: random() * 6.28, w: 1 / (i + 1) }));
    const heights: number[] = [];
    for (let x = 0; x < width; x++) {
      const snapped = Math.floor(x / blockStep) * blockStep;
      const noise = waves.reduce((sum, w) => sum + Math.sin((snapped / width) * 6.283 * w.f + w.p) * w.w, 0) / 1.9;
      heights.push(Math.round((ground + noise * amplitude) * scale));
    }
    for (let x = 0; x < width; x++) {
      for (let y = heights[x]; y < height; y++) put(x, y, color);
      if (fill !== PAGE) put(x, heights[x], rimColor);
    }
    const treeCount = Math.round((trees * width) / 480);
    for (let i = 0; i < treeCount && height > 60; i++) {
      const x = Math.floor(random() * width);
      const tall = Math.round((5 + random() * 5) * Math.max(scale, 0.6));
      for (let row = 0; row < tall; row++) {
        const half = Math.floor((row * 2.4) / tall);
        for (let dx = -half; dx <= half; dx++) put(x + dx, heights[x] - tall + row, color);
      }
    }
    return heights;
  });

  // The waymarks: a line of lit stones climbing back through the ridges.
  const lit = rgb(LIT);
  const core = rgb(CORE);
  const start = 0.8 + random() * 0.08;
  const stops = [
    { x: start, layer: 4, widths: [13, 10, 7, 4], light: 24 },
    { x: start - 0.134, layer: 3, widths: [9, 7, 5, 3], light: 17 },
    { x: start - 0.238, layer: 2, widths: [7, 5, 3], light: 12 },
    { x: start - 0.315, layer: 1, widths: [5, 3, 2], light: 8 },
    { x: start - 0.371, layer: 0, widths: [3, 2], light: 5 },
  ].slice(0, height < 60 ? 2 : 5);

  const stone = rgb('#5b6570');
  const shade = rgb('#3a424c');
  for (const stop of stops) {
    const cx = Math.round(stop.x * width);
    const widths = stop.widths.map((w) => Math.max(1, Math.round(w * Math.max(scale, 0.42))));
    let y = ridges[stop.layer][cx] + 1;
    const top = y - widths.reduce((sum, w) => sum + (w > 5 ? 3 : w > 2 ? 2 : 1), 0) - 1;
    const radius = Math.round(stop.light * Math.max(scale, 0.42));
    for (let py = top - radius; py <= top + radius; py++) {
      for (let px = cx - radius; px <= cx + radius; px++) {
        if (px < 0 || py < 0 || px >= width || py >= height) continue;
        const distance = Math.hypot(px - cx, py - top) / radius;
        if (distance < 1 && (1 - distance) ** 2 * 0.9 > threshold(px, py)) put(px, py, mix(get(px, py), lit, 0.13));
      }
    }
    widths.forEach((stoneWidth, index) => {
      const tall = stoneWidth > 5 ? 3 : stoneWidth > 2 ? 2 : 1;
      const left = cx - Math.floor(stoneWidth / 2);
      for (let row = 0; row < tall; row++) {
        y -= 1;
        for (let dx = 0; dx < stoneWidth; dx++) put(left + dx, y, dx > stoneWidth * 0.62 || row === 0 ? shade : stone);
      }
      if (index === widths.length - 1) {
        const glowing = Math.max(1, Math.floor(stoneWidth * 0.8));
        const from = cx - Math.floor(glowing / 2);
        for (let row = 0; row < (stoneWidth > 2 ? 2 : 1); row++) {
          y -= 1;
          for (let dx = 0; dx < glowing; dx++) put(from + dx, y, dx === Math.floor(glowing / 2) ? core : lit);
        }
      }
    });
  }

  return image;
}

const cache = new Map<string, string>();

/** A data URL for this seed at this size. Results are cached, so lists stay cheap. */
export function artUrl(seed: string, width: number, height: number): string {
  const key = `${width}x${height}:${seed}`;
  const cached = cache.get(key);
  if (cached) return cached;
  const canvas = document.createElement('canvas');
  canvas.width = width;
  canvas.height = height;
  canvas.getContext('2d')!.putImageData(render(seed, width, height), 0, 0);
  const url = canvas.toDataURL('image/png');
  cache.set(key, url);
  return url;
}
