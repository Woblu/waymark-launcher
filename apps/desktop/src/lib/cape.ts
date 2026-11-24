// Turns a picture into a cape texture.
//
// The layout and the cut-outs that shape the elytra come from Minecraft Cape
// Creator by James Harrison for MinecraftCapes (MIT License, copied in
// public/licenses/minecraft-cape-creator-MIT.txt). Waymark Launcher adds the
// crop, so the picture keeps its proportions instead of being squeezed.

/** A part of the picture, in the picture's own pixels. */
export interface Crop {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface CapeDesign {
  picture: ImageBitmap;
  /** What goes on the outside of the cape, 10 wide by 16 tall. */
  crop: Crop;
  /** For the edges, back and inside, or null to match the picture. */
  color: string | null;
  /** Whether the picture goes on the elytra too, or only the color does. */
  onElytra: boolean;
  /** 1 for the game's own 64 by 32, up to 32 for 2048 by 1024. */
  scale: number;
}

/** Width over height of the cape's outside. */
export const CAPE_SHAPE = 10 / 16;
export const CAPE_SCALES = [1, 2, 4, 8, 16, 32];

/** The same middle as the cape's crop, but as tall as the elytra's 10 by 20 needs. */
function elytraCrop(crop: Crop, picture: ImageBitmap): Crop {
  const height = Math.min(crop.width * 2, picture.height);
  const width = height / 2;
  const middleX = crop.x + crop.width / 2;
  const middleY = crop.y + crop.height / 2;
  return {
    x: Math.min(Math.max(middleX - width / 2, 0), picture.width - width),
    y: Math.min(Math.max(middleY - height / 2, 0), picture.height - height),
    width,
    height,
  };
}

/** The average of a region's visible pixels, as #rrggbb. */
function averageColor(context: CanvasRenderingContext2D, x: number, y: number, width: number, height: number): string {
  const { data } = context.getImageData(x, y, width, height);
  let red = 0;
  let green = 0;
  let blue = 0;
  let weight = 0;
  // Every fifth pixel is plenty, and see-through pixels don't count.
  for (let i = 0; i < data.length; i += 5 * 4) {
    const alpha = data[i + 3];
    red += data[i] * alpha;
    green += data[i + 1] * alpha;
    blue += data[i + 2] * alpha;
    weight += alpha;
  }
  if (!weight) return '#808080';
  const hex = (sum: number) => Math.round(sum / weight).toString(16).padStart(2, '0');
  return `#${hex(red)}${hex(green)}${hex(blue)}`;
}

/**
 * Draws the cape texture onto `canvas`, resizing it to 64 by 32 times the
 * scale, and returns the color it used for everything but the picture.
 */
export function drawCape(canvas: HTMLCanvasElement, design: CapeDesign): string {
  const { picture, crop, scale } = design;
  canvas.width = 64 * scale;
  canvas.height = 32 * scale;
  const context = canvas.getContext('2d', { willReadFrequently: true })!;
  context.clearRect(0, 0, canvas.width, canvas.height);

  // Pixel art stays sharp when it's blown up. Photos are smoothed.
  const place = (source: Crop, x: number, y: number, width: number, height: number) => {
    context.imageSmoothingEnabled = source.width * 2 > width * scale;
    context.imageSmoothingQuality = 'high';
    context.drawImage(picture, source.x, source.y, source.width, source.height, x * scale, y * scale, width * scale, height * scale);
  };
  const fill = (x: number, y: number, width: number, height: number) => context.fillRect(x * scale, y * scale, width * scale, height * scale);
  const clear = (x: number, y: number, width: number, height: number) => context.clearRect(x * scale, y * scale, width * scale, height * scale);

  // The cape's outside.
  place(crop, 1, 1, 10, 16);
  const color = design.color ?? averageColor(context, scale, scale, 10 * scale, 16 * scale);
  context.fillStyle = color;

  // The elytra's outside.
  if (design.onElytra) place(elytraCrop(crop, picture), 36, 2, 10, 20);
  else fill(36, 2, 10, 20);

  // The cape's edges and inside.
  fill(0, 1, 1, 16);
  fill(1, 0, 10, 1);
  fill(11, 1, 1, 16);
  fill(11, 0, 10, 1);
  fill(12, 1, 10, 16);

  // The elytra's edges.
  fill(22, 11, 1, 11);
  fill(31, 0, 3, 1);
  fill(32, 1, 2, 1);
  fill(34, 0, 6, 1);
  fill(34, 2, 1, 2);
  fill(35, 2, 1, 9);

  // Cut the wing's shape out of the elytra's outside.
  clear(36, 16, 1, 6);
  clear(37, 19, 1, 3);
  clear(38, 21, 1, 1);
  clear(42, 2, 1, 1);
  clear(43, 2, 1, 2);
  clear(44, 2, 1, 5);
  clear(45, 2, 1, 9);
  return color;
}

/** The biggest crop of the cape's shape that fits the picture, in its middle. */
export function wholeCrop(picture: ImageBitmap): Crop {
  const width = Math.min(picture.width, picture.height * CAPE_SHAPE);
  const height = width / CAPE_SHAPE;
  return { x: (picture.width - width) / 2, y: (picture.height - height) / 2, width, height };
}

/** Moves and resizes a crop around its middle, keeping it inside the picture. */
export function fitCrop(picture: ImageBitmap, middleX: number, middleY: number, width: number): Crop {
  const largest = wholeCrop(picture).width;
  const w = Math.min(Math.max(width, Math.min(8, largest)), largest);
  const h = w / CAPE_SHAPE;
  return {
    x: Math.min(Math.max(middleX - w / 2, 0), picture.width - w),
    y: Math.min(Math.max(middleY - h / 2, 0), picture.height - h),
    width: w,
    height: h,
  };
}
