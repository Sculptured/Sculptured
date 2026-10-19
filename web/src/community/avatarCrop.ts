export const AVATAR_MAX_BYTES = 262144;
export const AVATAR_TYPES = ['image/png', 'image/jpeg', 'image/webp'] as const;

/** The centre square of a picture at `size`×`size`, as WebP (Community re-encodes it again). */
export async function cropToSquare(file: File, size = 256): Promise<Blob> {
  const img = await createImageBitmap(file);
  const side = Math.min(img.width, img.height);
  const canvas = document.createElement('canvas');
  canvas.width = size;
  canvas.height = size;
  canvas.getContext('2d')!.drawImage(img, (img.width - side) / 2, (img.height - side) / 2, side, side, 0, 0, size, size);
  return new Promise((resolve, reject) => canvas.toBlob((b) => (b ? resolve(b) : reject(new Error('Could not read that picture'))), 'image/webp', 0.9));
}
