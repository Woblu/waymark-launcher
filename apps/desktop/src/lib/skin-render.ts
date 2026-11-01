// Draws the skin tiles. The 3D library is most of a megabyte, so it only loads
// when the Skins page opens, and each tile is drawn once and kept as a picture
// so a page of skins doesn't need a live 3D scene for every one of them.

import type { SkinViewer } from 'skinview3d';
import type { SkinVariant } from './api';

export interface SkinPicture {
  front: string;
  back: string;
}

const WIDTH = 200;
const HEIGHT = 280;
const TURN = 0.4;

const pictures = new Map<string, SkinPicture>();
let baker: SkinViewer | undefined;
let queue: Promise<unknown> = Promise.resolve();

export const modelName = (variant: SkinVariant) => (variant === 'slim' ? 'slim' : 'default');

async function bake(url: string, variant: SkinVariant): Promise<SkinPicture> {
  if (!baker) {
    const { SkinViewer } = await import('skinview3d');
    const canvas = document.createElement('canvas');
    baker = new SkinViewer({ canvas, width: WIDTH, height: HEIGHT, renderPaused: true, preserveDrawingBuffer: true, enableControls: false, fov: 40, zoom: 0.92 });
  }
  await baker.loadSkin(url, { model: modelName(variant) });
  baker.playerWrapper.rotation.y = TURN;
  baker.render();
  const front = baker.canvas.toDataURL('image/png');
  baker.playerWrapper.rotation.y = TURN + Math.PI;
  baker.render();
  return { front, back: baker.canvas.toDataURL('image/png') };
}

/** One at a time, because they all share the same scene. */
export function skinPicture(url: string, variant: SkinVariant): Promise<SkinPicture> {
  const key = `${variant}:${url}`;
  const known = pictures.get(key);
  if (known) return Promise.resolve(known);
  const work = queue.then(() => bake(url, variant));
  queue = work.catch(() => {});
  return work.then((picture) => {
    pictures.set(key, picture);
    return picture;
  });
}

/** Works out the arm width the way the game does, from the skin's own pixels. */
export async function detectVariant(url: string): Promise<SkinVariant> {
  const { inferModelType, loadImage } = await import('skinview-utils');
  const image = await loadImage(url);
  const canvas = document.createElement('canvas');
  canvas.width = image.width;
  canvas.height = image.height;
  canvas.getContext('2d', { willReadFrequently: true })?.drawImage(image, 0, 0);
  return inferModelType(canvas) === 'slim' ? 'slim' : 'classic';
}

/** The player's face from a skin, with the hat layer over it. */
export async function skinHead(url: string): Promise<string> {
  const { loadImage } = await import('skinview-utils');
  const image = await loadImage(url);
  const canvas = document.createElement('canvas');
  canvas.width = canvas.height = 8;
  const context = canvas.getContext('2d');
  context?.drawImage(image, 8, 8, 8, 8, 0, 0, 8, 8);
  context?.drawImage(image, 40, 8, 8, 8, 0, 0, 8, 8);
  return canvas.toDataURL('image/png');
}
