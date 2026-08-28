import { normalizeRasterImageSource } from "./media-model.js";

export function loadHtmlRasterImage(
  source,
  { anonymous = false, ImageCtor = globalThis.Image } = {}
) {
  if (typeof ImageCtor !== "function") return Promise.resolve(null);

  return new Promise((resolve) => {
    const image = new ImageCtor();
    if (anonymous) image.crossOrigin = "anonymous";
    image.referrerPolicy = "no-referrer";
    image.onload = () => resolve(image);
    image.onerror = () => resolve(null);
    image.src = source;
  });
}

export async function loadRasterImageSource(
  value,
  {
    fetchFn = globalThis.fetch?.bind(globalThis),
    createObjectURL = globalThis.URL?.createObjectURL?.bind(globalThis.URL),
    revokeObjectURL = globalThis.URL?.revokeObjectURL?.bind(globalThis.URL),
    loadImage = loadHtmlRasterImage,
    onFetchError = (error) => console.debug("Local raster source fetch fallback:", error),
  } = {}
) {
  const source = normalizeRasterImageSource(value);
  if (!source) return null;

  if (/^(?:data:image\/|blob:)/i.test(source)) {
    return loadImage(source);
  }

  if (
    typeof fetchFn === "function" &&
    typeof createObjectURL === "function" &&
    typeof revokeObjectURL === "function"
  ) {
    try {
      const response = await fetchFn(source, {
        cache: "no-store",
        credentials: "omit",
      });

      if (response?.ok) {
        const blob = await response.blob();
        if (blob?.size > 0) {
          const objectUrl = createObjectURL(blob);
          try {
            const image = await loadImage(objectUrl);
            if (image) return image;
          } finally {
            revokeObjectURL(objectUrl);
          }
        }
      }
    } catch (error) {
      onFetchError?.(error);
    }
  }

  return loadImage(source, {
    anonymous: /^https?:/i.test(source),
  });
}
