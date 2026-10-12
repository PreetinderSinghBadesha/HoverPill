import { useEffect, useState } from "react";

const cache = new Map<string, string | null>();

/**
 * The accent colour of a piece of album art, as space-separated "r g b" for `rgb(var(--accent) / a)`,
 * or null if there's no art or it can't be read (e.g. a cross-origin cover without CORS headers).
 * Weighted towards saturated pixels so a mostly-black cover with a yellow logo comes out yellow.
 */
export function useArtworkColor(src?: string): string | null {
  const [rgb, setRgb] = useState<string | null>(() => (src && cache.has(src) ? cache.get(src)! : null));

  useEffect(() => {
    if (!src) return setRgb(null);
    if (cache.has(src)) return setRgb(cache.get(src)!);
    let alive = true;
    const img = new Image();
    img.crossOrigin = "anonymous";
    img.onload = () => {
      const value = sample(img);
      cache.set(src, value);
      if (alive) setRgb(value);
    };
    img.onerror = () => {
      cache.set(src, null);
      if (alive) setRgb(null);
    };
    img.src = src;
    return () => {
      alive = false;
    };
  }, [src]);

  return rgb;
}

function sample(img: HTMLImageElement): string | null {
  try {
    const size = 24;
    const canvas = document.createElement("canvas");
    canvas.width = canvas.height = size;
    const ctx = canvas.getContext("2d", { willReadFrequently: true });
    if (!ctx) return null;
    ctx.drawImage(img, 0, 0, size, size);
    const data = ctx.getImageData(0, 0, size, size).data;
    let r = 0, g = 0, b = 0, total = 0;
    for (let i = 0; i < data.length; i += 4) {
      const [R, G, B] = [data[i], data[i + 1], data[i + 2]];
      const max = Math.max(R, G, B), min = Math.min(R, G, B);
      const saturation = max === 0 ? 0 : (max - min) / max;
      const weight = saturation * saturation * (max > 40 ? 1 : 0.05) + 0.002;
      r += R * weight;
      g += G * weight;
      b += B * weight;
      total += weight;
    }
    if (!total) return null;
    [r, g, b] = [r / total, g / total, b / total];
    // Lift dark results so the tint reads on black; keep the hue.
    const peak = Math.max(r, g, b, 1);
    const lift = peak < 200 ? 200 / peak : 1;
    return [r, g, b].map((c) => Math.min(255, Math.round(c * lift))).join(" ");
  } catch {
    return null; // tainted canvas
  }
}
