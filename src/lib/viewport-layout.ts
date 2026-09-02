export type ViewportMenuPosition = { x: number; y: number };

const DEFAULT_VIEWPORT_GAP = 8;

function safeAreaInset(property: string) {
  if (typeof document === "undefined") return 0;
  const value = getComputedStyle(document.documentElement).getPropertyValue(property);
  const pixels = Number.parseFloat(value);
  return Number.isFinite(pixels) ? Math.max(0, pixels) : 0;
}

export function viewportMenuPosition(
  x: number,
  y: number,
  width: number,
  height: number,
): ViewportMenuPosition {
  const viewport = window.visualViewport;
  const viewportLeft = viewport?.offsetLeft ?? 0;
  const viewportTop = viewport?.offsetTop ?? 0;
  const viewportWidth = viewport?.width ?? window.innerWidth;
  const viewportHeight = viewport?.height ?? window.innerHeight;
  const minimumX = viewportLeft + safeAreaInset("--safe-area-left") + DEFAULT_VIEWPORT_GAP;
  const minimumY = viewportTop + safeAreaInset("--safe-area-top") + DEFAULT_VIEWPORT_GAP;
  const maximumX = Math.max(
    minimumX,
    viewportLeft
      + viewportWidth
      - safeAreaInset("--safe-area-right")
      - width
      - DEFAULT_VIEWPORT_GAP,
  );
  const maximumY = Math.max(
    minimumY,
    viewportTop
      + viewportHeight
      - safeAreaInset("--safe-area-bottom")
      - height
      - DEFAULT_VIEWPORT_GAP,
  );

  return {
    x: Math.max(minimumX, Math.min(x, maximumX)),
    y: Math.max(minimumY, Math.min(y, maximumY)),
  };
}
