export type RgbColor = Readonly<{
  red: number;
  green: number;
  blue: number;
}>;

export type CoverTheme = Readonly<{
  base100: string;
  base200: string;
  base300: string;
  baseContent: string;
  primary: string;
  primaryContent: string;
  secondary: string;
  secondaryContent: string;
  accent: string;
  accentContent: string;
  neutral: string;
  neutralContent: string;
}>;

export type DynamicColorScheme = "light" | "dark";

type ColorBucket = {
  red: number;
  green: number;
  blue: number;
  count: number;
  score: number;
};

type HslColor = {
  hue: number;
  saturation: number;
  lightness: number;
};

const COVER_SAMPLE_SIZE = 64;
const OPAQUE_ALPHA_THRESHOLD = 128;

const COVER_THEME_PROPERTIES = {
  base100: "--color-base-100",
  base200: "--color-base-200",
  base300: "--color-base-300",
  baseContent: "--color-base-content",
  primary: "--color-primary",
  primaryContent: "--color-primary-content",
  secondary: "--color-secondary",
  secondaryContent: "--color-secondary-content",
  accent: "--color-accent",
  accentContent: "--color-accent-content",
  neutral: "--color-neutral",
  neutralContent: "--color-neutral-content",
} as const satisfies Record<keyof CoverTheme, string>;

export async function extractCoverTheme(coverUrl: string): Promise<CoverTheme | null> {
  if (!coverUrl || typeof document === "undefined" || typeof Image === "undefined") {
    return null;
  }

  try {
    const image = await loadImage(coverUrl);
    const scale = Math.min(1, COVER_SAMPLE_SIZE / Math.max(image.naturalWidth, image.naturalHeight));
    const width = Math.max(1, Math.round(image.naturalWidth * scale));
    const height = Math.max(1, Math.round(image.naturalHeight * scale));
    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;
    const context = canvas.getContext("2d", { willReadFrequently: true });
    if (!context) {
      return null;
    }

    context.drawImage(image, 0, 0, width, height);
    const dominantColor = extractDominantColor(context.getImageData(0, 0, width, height).data);
    return dominantColor
      ? buildCoverTheme(dominantColor, preferredColorScheme())
      : null;
  } catch {
    // Remote artwork without CORS access can still be displayed, but cannot be sampled.
    return null;
  }
}

export function extractDominantColor(pixelData: ArrayLike<number>): RgbColor | null {
  const visibleBuckets = new Map<number, ColorBucket>();
  const nonExtremeBuckets = new Map<number, ColorBucket>();

  for (let index = 0; index + 3 < pixelData.length; index += 4) {
    const alpha = pixelData[index + 3] ?? 0;
    if (alpha < OPAQUE_ALPHA_THRESHOLD) {
      continue;
    }

    const red = pixelData[index] ?? 0;
    const green = pixelData[index + 1] ?? 0;
    const blue = pixelData[index + 2] ?? 0;
    const saturation = rgbToHsl({ red, green, blue }).saturation;
    const score = 1 + saturation * 0.35;
    addToBucket(visibleBuckets, red, green, blue, score);

    const darkestChannel = Math.min(red, green, blue);
    const lightestChannel = Math.max(red, green, blue);
    if (lightestChannel > 12 && darkestChannel < 244) {
      addToBucket(nonExtremeBuckets, red, green, blue, score);
    }
  }

  return strongestBucketColor(nonExtremeBuckets) ?? strongestBucketColor(visibleBuckets);
}

export function buildCoverTheme(
  source: RgbColor,
  colorScheme: DynamicColorScheme = "light",
): CoverTheme {
  const sourceHsl = rgbToHsl(source);
  const isMonochrome = sourceHsl.saturation < 0.08;
  const saturation = isMonochrome ? 0 : clamp(sourceHsl.saturation, 0.35, 0.82);
  const lightness = clamp(sourceHsl.lightness, 0.34, 0.66);
  const surfaceSaturation = isMonochrome
    ? 0
    : clamp(sourceHsl.saturation * 0.55, 0.18, 0.42);
  const surfaceLightness = colorScheme === "dark"
    ? {
        base100: 0.09 + sourceHsl.lightness * 0.04,
        base200: 0.14 + sourceHsl.lightness * 0.05,
        base300: 0.2 + sourceHsl.lightness * 0.07,
        content: 0.92,
      }
    : {
        base100: 0.94 + sourceHsl.lightness * 0.02,
        base200: 0.86 + sourceHsl.lightness * 0.035,
        base300: 0.76 + sourceHsl.lightness * 0.05,
        content: 0.14,
      };
  const base100 = hslToRgb({
    hue: sourceHsl.hue,
    saturation: surfaceSaturation,
    lightness: surfaceLightness.base100,
  });
  const base200 = hslToRgb({
    hue: sourceHsl.hue,
    saturation: clamp(surfaceSaturation * 1.08, 0, 1),
    lightness: surfaceLightness.base200,
  });
  const base300 = hslToRgb({
    hue: sourceHsl.hue,
    saturation: clamp(surfaceSaturation * 1.16, 0, 1),
    lightness: surfaceLightness.base300,
  });
  const baseContent = hslToRgb({
    hue: sourceHsl.hue,
    saturation: surfaceSaturation * 0.45,
    lightness: surfaceLightness.content,
  });
  const primary = hslToRgb({ hue: sourceHsl.hue, saturation, lightness });
  const secondary = hslToRgb({
    hue: sourceHsl.hue + (isMonochrome ? 0 : 32),
    saturation: isMonochrome ? 0 : clamp(saturation * 0.88, 0.3, 0.76),
    lightness: clamp(lightness + 0.07, 0.38, 0.7),
  });
  const accent = hslToRgb({
    hue: sourceHsl.hue - (isMonochrome ? 0 : 34),
    saturation: isMonochrome ? 0 : clamp(saturation + 0.08, 0.38, 0.88),
    lightness: clamp(lightness + 0.02, 0.36, 0.68),
  });
  const neutral = hslToRgb({
    hue: sourceHsl.hue,
    saturation: isMonochrome ? 0 : clamp(sourceHsl.saturation * 0.42, 0.12, 0.34),
    lightness: colorScheme === "dark" ? 0.3 : 0.25,
  });

  return {
    base100: formatRgb(base100),
    base200: formatRgb(base200),
    base300: formatRgb(base300),
    baseContent: formatRgb(baseContent),
    primary: formatRgb(primary),
    primaryContent: readableContentColor(primary),
    secondary: formatRgb(secondary),
    secondaryContent: readableContentColor(secondary),
    accent: formatRgb(accent),
    accentContent: readableContentColor(accent),
    neutral: formatRgb(neutral),
    neutralContent: readableContentColor(neutral),
  };
}

export function applyCoverTheme(element: HTMLElement, theme: CoverTheme) {
  for (const [key, property] of Object.entries(COVER_THEME_PROPERTIES)) {
    element.style.setProperty(property, theme[key as keyof CoverTheme]);
  }
}

export function clearCoverTheme(element: HTMLElement) {
  for (const property of Object.values(COVER_THEME_PROPERTIES)) {
    element.style.removeProperty(property);
  }
}

function loadImage(url: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const image = new Image();
    if (/^https?:\/\//i.test(url)) {
      image.crossOrigin = "anonymous";
    }
    image.decoding = "async";
    image.onload = () => {
      if (image.naturalWidth && image.naturalHeight) {
        resolve(image);
      } else {
        reject(new Error("Cover image has no visible pixels"));
      }
    };
    image.onerror = () => reject(new Error("Cover image could not be loaded"));
    image.src = url;
  });
}

function preferredColorScheme(): DynamicColorScheme {
  return typeof window !== "undefined"
    && typeof window.matchMedia === "function"
    && window.matchMedia("(prefers-color-scheme: dark)").matches
    ? "dark"
    : "light";
}

function addToBucket(
  buckets: Map<number, ColorBucket>,
  red: number,
  green: number,
  blue: number,
  score: number,
) {
  const key = ((red >> 5) << 6) | ((green >> 5) << 3) | (blue >> 5);
  const bucket = buckets.get(key) ?? { red: 0, green: 0, blue: 0, count: 0, score: 0 };
  bucket.red += red;
  bucket.green += green;
  bucket.blue += blue;
  bucket.count += 1;
  bucket.score += score;
  buckets.set(key, bucket);
}

function strongestBucketColor(buckets: Map<number, ColorBucket>): RgbColor | null {
  let strongest: ColorBucket | null = null;
  for (const bucket of buckets.values()) {
    if (!strongest || bucket.score > strongest.score) {
      strongest = bucket;
    }
  }
  if (!strongest) {
    return null;
  }
  return {
    red: Math.round(strongest.red / strongest.count),
    green: Math.round(strongest.green / strongest.count),
    blue: Math.round(strongest.blue / strongest.count),
  };
}

function rgbToHsl({ red, green, blue }: RgbColor): HslColor {
  const normalizedRed = red / 255;
  const normalizedGreen = green / 255;
  const normalizedBlue = blue / 255;
  const max = Math.max(normalizedRed, normalizedGreen, normalizedBlue);
  const min = Math.min(normalizedRed, normalizedGreen, normalizedBlue);
  const delta = max - min;
  const lightness = (max + min) / 2;

  if (delta === 0) {
    return { hue: 0, saturation: 0, lightness };
  }

  const saturation = delta / (1 - Math.abs(2 * lightness - 1));
  let hue: number;
  if (max === normalizedRed) {
    hue = 60 * (((normalizedGreen - normalizedBlue) / delta) % 6);
  } else if (max === normalizedGreen) {
    hue = 60 * ((normalizedBlue - normalizedRed) / delta + 2);
  } else {
    hue = 60 * ((normalizedRed - normalizedGreen) / delta + 4);
  }

  return { hue: hue < 0 ? hue + 360 : hue, saturation, lightness };
}

function hslToRgb({ hue, saturation, lightness }: HslColor): RgbColor {
  const normalizedHue = ((hue % 360) + 360) % 360;
  const chroma = (1 - Math.abs(2 * lightness - 1)) * saturation;
  const section = normalizedHue / 60;
  const secondary = chroma * (1 - Math.abs((section % 2) - 1));
  let red = 0;
  let green = 0;
  let blue = 0;

  if (section < 1) [red, green] = [chroma, secondary];
  else if (section < 2) [red, green] = [secondary, chroma];
  else if (section < 3) [green, blue] = [chroma, secondary];
  else if (section < 4) [green, blue] = [secondary, chroma];
  else if (section < 5) [red, blue] = [secondary, chroma];
  else [red, blue] = [chroma, secondary];

  const match = lightness - chroma / 2;
  return {
    red: Math.round((red + match) * 255),
    green: Math.round((green + match) * 255),
    blue: Math.round((blue + match) * 255),
  };
}

function readableContentColor(background: RgbColor) {
  const black = { red: 0, green: 0, blue: 0 };
  const white = { red: 255, green: 255, blue: 255 };
  return contrastRatio(background, black) >= contrastRatio(background, white)
    ? formatRgb(black)
    : formatRgb(white);
}

function contrastRatio(first: RgbColor, second: RgbColor) {
  const lighter = Math.max(relativeLuminance(first), relativeLuminance(second));
  const darker = Math.min(relativeLuminance(first), relativeLuminance(second));
  return (lighter + 0.05) / (darker + 0.05);
}

function relativeLuminance({ red, green, blue }: RgbColor) {
  const linear = [red, green, blue].map((channel) => {
    const value = channel / 255;
    return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * linear[0] + 0.7152 * linear[1] + 0.0722 * linear[2];
}

function formatRgb({ red, green, blue }: RgbColor) {
  return `rgb(${red} ${green} ${blue})`;
}

function clamp(value: number, minimum: number, maximum: number) {
  return Math.min(maximum, Math.max(minimum, value));
}
